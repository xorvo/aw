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

    // Load any existing state to preserve `last_prompt` across events that
    // don't carry one.
    let path = pane_state_path(&pane_id)?;
    let mut state = PaneState::read(&path).unwrap_or_else(|_| PaneState::new(&pane_id, agent_name));
    state.pane_id = pane_id.clone();
    state.agent = agent_name.to_string();
    state.status = status;
    state.last_event = event.to_string();
    state.last_activity = crate::dash::state::now_epoch();

    // CLI flag wins; then stdin payload; otherwise keep prior.
    if let Some(p) = prompt {
        if !p.is_empty() {
            state.last_prompt = p;
        }
    } else if let Some(p) = prompt_from_stdin {
        state.last_prompt = p;
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

    // Fire a notification on transition into `waiting`. Cheap and per-event;
    // we don't try to dedupe.
    if matches!(status, Status::Waiting) {
        crate::dash::notify::on_waiting(&state);
    }

    Ok(())
}

fn map_event(agent: &str, event: &str) -> Option<Status> {
    match (agent, event) {
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

        _ => None,
    }
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
