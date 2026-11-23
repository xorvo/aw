//! `aw hook --agent <a> --event <e> [--prompt <p>]` — agent-facing state
//! writer. Called from Claude Code / Codex hook configs and from the pi
//! extension. Reads the agent's stdin payload (if any), maps the event to
//! a status, and atomically writes the per-pane state file.

use std::io::Read;

use anyhow::Result;

use crate::cli::AgentKind;
use crate::dash::state::{pane_state_path, PaneState, Status};
use crate::dash::tmux;

pub fn run(agent: AgentKind, event: &str, prompt: Option<String>) -> Result<()> {
    let agent_name = match agent {
        AgentKind::Claude => "claude",
        AgentKind::Codex => "codex",
        AgentKind::Pi => "pi",
        AgentKind::Opencode => "opencode",
        AgentKind::Kimi => "kimi",
        // `--agent all` doesn't make sense for a hook firing, but tolerate it
        // by treating as 'unknown'.
        AgentKind::All => "agent",
    };

    let pane_id = match tmux::current_pane() {
        Some(p) => p,
        None => {
            // Not in tmux — silently no-op so a misconfigured hook doesn't
            // break the agent.
            return Ok(());
        }
    };

    // Determine the new status for this (agent, event) pair.
    let status = match map_event(agent_name, event) {
        Some(s) => s,
        None => return Ok(()), // unknown event: do nothing
    };

    // Read JSON payload from stdin if any. Best-effort — agents that don't
    // write a payload (Claude on certain events, Codex SessionStart) work fine.
    let stdin_payload = read_stdin_lossy();
    let prompt_from_stdin = extract_prompt(&stdin_payload, agent_name);
    let session_id_from_stdin = extract_session_id(&stdin_payload);

    // Load any existing state to preserve `last_prompt` across events that
    // don't carry one.
    let path = pane_state_path(&pane_id)?;
    let mut state = PaneState::read(&path).unwrap_or_else(|_| PaneState::new(&pane_id, agent_name));
    state.pane_id = pane_id.clone();
    let agent_pid = agent_pid();
    if is_new_agent_run(&state, agent_name, agent_pid) {
        // What we knew belonged to the previous run: carrying its conversation
        // id forward would label a Codex pane with a Claude conversation, or a
        // fresh Codex with the last one's.
        forget_previous_agent(&mut state);
    }
    state.agent = agent_name.to_string();
    if agent_pid.is_some() {
        state.agent_pid = agent_pid;
    }
    // Compaction re-fires SessionStart mid-turn; it is not a return to idle.
    if !(event == "SessionStart" && payload_source(&stdin_payload).as_deref() == Some("compact")) {
        state.status = status;
    }
    state.last_event = event.to_string();
    state.last_activity = crate::dash::state::now_epoch();
    // Stamp the owning tmux server: only a process on this same server may ever
    // garbage-collect this file.
    state.server_pid = tmux::server_pid();

    // CLI flag wins; then stdin payload; otherwise keep prior.
    if let Some(p) = prompt {
        if !p.is_empty() {
            state.last_prompt = p;
        }
    } else if let Some(p) = prompt_from_stdin {
        state.last_prompt = p;
    }

    // Keep the last-seen conversation id across events that omit it.
    if let Some(id) = session_id_from_stdin {
        state.session_id = id;
    }

    // Resolve env-derived fields. These come "for free" from the env that
    // `aw start` sets up; if absent, we leave them as-is (or empty).
    if let Ok(name) = std::env::var("AGENT_WORKSPACE_NAME") {
        if !name.is_empty() {
            state.workspace = name;
        }
    }
    if let Ok(p) = std::env::var("AGENT_WORKSPACE") {
        if !p.is_empty() {
            state.cwd = p;
        }
    } else if state.cwd.is_empty() {
        if let Ok(p) = std::env::current_dir() {
            state.cwd = p.display().to_string();
        }
    }
    if state.session.is_empty() || state.session == "unknown" {
        let s = tmux::pane_session(&pane_id);
        if !s.is_empty() {
            state.session = s;
        }
    }

    state.write_atomic(&path)?;

    // Shadow the session into the durable manifest so `aw resurrect` can
    // rebuild it after a tmux-server death. Best-effort: state files are
    // the primary record, the manifest is recovery data.
    crate::manifest::record_pane(
        &state.session,
        &state.workspace,
        &state.cwd,
        agent_name,
        &pane_id,
        &state.session_id,
        &tmux::pane_window(&pane_id),
    );

    // Also stamp the pane itself, so tmux-side tooling can read what is running
    // here without knowing about our cache.
    tmux::stamp_pane(&pane_id, agent_name, &state.session_id, agent_pid);

    // Fire a notification on transition into `waiting`. Cheap and per-event;
    // we don't try to dedupe.
    if matches!(status, Status::Waiting) {
        crate::dash::notify::on_waiting(&state);
    }

    Ok(())
}

/// Has a different agent, or another process of the same one, taken the pane
/// since the last hook? An unknown pid on either side says nothing: state
/// seeded by `aw resurrect` has none, and must keep the conversation id it
/// restored.
fn is_new_agent_run(state: &PaneState, agent: &str, pid: Option<u32>) -> bool {
    if state.agent != agent {
        return true;
    }
    matches!((state.agent_pid, pid), (Some(old), Some(new)) if old != new)
}

/// Drop the per-agent fields of a pane's state when another agent takes it.
fn forget_previous_agent(state: &mut PaneState) {
    state.session_id.clear();
    state.last_prompt.clear();
}

fn map_event(agent: &str, event: &str) -> Option<Status> {
    match (agent, event) {
        // Fires on startup, resume and `/clear` — the moments the conversation
        // id can change — with Claude waiting for input. (Also on compaction,
        // where `run` keeps the prior status.)
        ("claude", "SessionStart") => Some(Status::Idle),
        ("claude", "UserPromptSubmit") => Some(Status::Working),
        ("claude", "PreToolUse") => Some(Status::Working),
        ("claude", "Notification") => Some(Status::Waiting),
        ("claude", "Stop") => Some(Status::Idle),

        ("codex", "SessionStart") => Some(Status::Idle),
        ("codex", "UserPromptSubmit") => Some(Status::Working),
        ("codex", "PreToolUse") => Some(Status::Working),
        ("codex", "Stop") => Some(Status::Idle),

        ("pi", "agent_start") => Some(Status::Working),
        ("pi", "input") => Some(Status::Working),
        ("pi", "agent_end") => Some(Status::Idle),

        // OpenCode and Kimi Code expose Claude-compatible hook events, so
        // they share Claude's event vocabulary.
        ("opencode" | "kimi", "UserPromptSubmit") => Some(Status::Working),
        ("opencode" | "kimi", "PreToolUse") => Some(Status::Working),
        ("opencode" | "kimi", "Notification") => Some(Status::Waiting),
        ("opencode" | "kimi", "Stop") => Some(Status::Idle),

        _ => None,
    }
}

/// Conversation id from the hook payload. Claude-style hooks use
/// `session_id`; tolerate the camelCase and Codex rollout spellings too.
///
/// For Claude this is the id to fork. Hooks fired inside a Task sub-agent
/// report the parent conversation's `session_id` (observed on Claude Code
/// 2.1.330), so it is safe to record.
fn extract_session_id(payload: &str) -> Option<String> {
    if payload.trim().is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    for key in ["session_id", "sessionId", "rollout_id"] {
        if let Some(s) = v.get(key).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// The agent process that fired this hook: our nearest ancestor that is not a
/// shell (agents run hook commands through `sh -c`). Stamped on the pane so a
/// reader such as a fork binding can tell stamps written by the agent running
/// there now from ones a previous process left behind.
fn agent_pid() -> Option<u32> {
    nearest_non_shell(std::os::unix::process::parent_id(), ps_parent)
}

/// Pure core of [`agent_pid`]; `parent` maps a pid to `(ppid, command)`.
fn nearest_non_shell(start: u32, parent: impl Fn(u32) -> Option<(u32, String)>) -> Option<u32> {
    let mut pid = start;
    // Agent → sh → aw is the usual depth; a few more covers wrapper shells.
    for _ in 0..4 {
        let (ppid, comm) = parent(pid)?;
        if !is_shell(&comm) {
            return Some(pid);
        }
        pid = ppid;
    }
    None
}

fn is_shell(comm: &str) -> bool {
    let base = comm
        .rsplit('/')
        .next()
        .unwrap_or(comm)
        .trim_start_matches('-');
    matches!(base, "sh" | "bash" | "zsh" | "dash" | "fish" | "ksh")
}

fn ps_parent(pid: u32) -> Option<(u32, String)> {
    let out = std::process::Command::new("ps")
        .args(["-o", "ppid=,comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&out.stdout);
    let (ppid, comm) = line.trim().split_once(char::is_whitespace)?;
    Some((ppid.trim().parse().ok()?, comm.trim().to_string()))
}

fn payload_source(payload: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    v.get("source")?.as_str().map(str::to_string)
}

fn read_stdin_lossy() -> String {
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    buf
}

fn extract_prompt(payload: &str, _agent: &str) -> Option<String> {
    if payload.trim().is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    // Try common fields used by Claude Code (`prompt`) and Codex
    // (`user_prompt` or `prompt` depending on version).
    for key in ["prompt", "user_prompt", "text", "input"] {
        if let Some(s) = v.get(key).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_agents_forgets_the_previous_conversation() {
        let mut s = PaneState::new("%1", "claude");
        s.session_id = "claude-sid".into();
        s.last_prompt = "hi".into();
        forget_previous_agent(&mut s);
        assert!(s.session_id.is_empty());
        assert!(s.last_prompt.is_empty());
    }

    #[test]
    fn a_new_run_is_another_agent_or_another_process() {
        let mut s = PaneState::new("%1", "codex");
        s.agent_pid = Some(10);
        assert!(is_new_agent_run(&s, "claude", Some(10)));
        assert!(is_new_agent_run(&s, "codex", Some(11)));
        assert!(!is_new_agent_run(&s, "codex", Some(10)));
        // Unknown either side: keep what we have (resurrect seeds no pid).
        assert!(!is_new_agent_run(&s, "codex", None));
        s.agent_pid = None;
        assert!(!is_new_agent_run(&s, "codex", Some(11)));
    }

    #[test]
    fn agent_pid_skips_the_hook_shell() {
        // aw (100) ← sh (200) ← claude (300) ← zsh (400)
        let tree = |pid| match pid {
            200 => Some((300, "/bin/sh".to_string())),
            300 => Some((
                400,
                "/Users/u/.local/share/claude/versions/2.1.330".to_string(),
            )),
            400 => Some((1, "-zsh".to_string())),
            _ => None,
        };
        assert_eq!(nearest_non_shell(200, tree), Some(300));
        assert_eq!(nearest_non_shell(300, tree), Some(300));
        // Nothing but shells up to where `ps` stops answering: unknown.
        assert_eq!(nearest_non_shell(400, tree), None);
    }

    #[test]
    fn claude_session_start_is_tracked() {
        // Without it, `/clear` leaves the old id on the pane until next prompt.
        assert!(matches!(
            map_event("claude", "SessionStart"),
            Some(Status::Idle)
        ));
    }

    #[test]
    fn sub_agent_payload_keeps_the_parent_session_id() {
        let payload = r#"{"session_id":"parent","agent_id":"a1","agent_type":"general-purpose","hook_event_name":"PreToolUse"}"#;
        assert_eq!(extract_session_id(payload).as_deref(), Some("parent"));
    }
}
