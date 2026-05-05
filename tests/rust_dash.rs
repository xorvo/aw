//! Tests for `aw hook` (state writer), `aw dash json|gc|status-line|park|next-ready`.
//!
//! No bash counterpart — the dashboard is new in the Rust port. Tests fake
//! the `$TMUX_PANE` env var so the hook code path activates without an
//! actual tmux server.

mod common;

use std::process::Command;

use common::{Bin, TestEnv};

/// Run `aw <args>` with $TMUX_PANE set so the hook path activates. We
/// can't reuse `env.run` directly because that one env_clears (no `TMUX_PANE`).
fn run_with_pane(env: &TestEnv, pane_id: &str, args: &[&str], stdin: &str) -> std::process::Output {
    use std::io::Write;
    use std::process::Stdio;
    let path = format!(
        "{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
        env.fake_bin.display()
    );
    let mut cmd = Command::new(Bin::Rust.path());
    cmd.args(args)
        .current_dir(&env.home)
        .env_clear()
        .env("HOME", &env.home)
        .env("PATH", path)
        .env("AW_INSTALL_DIR", &env.install_dir)
        .env("AW_WORKSPACES_DIR", &env.workspaces_dir)
        .env("AW_BIN_DIR", &env.bin_dir)
        .env("AW_CONFIG_FILE", &env.config_path)
        .env("AW_STATE_DIR", &env.state_dir)
        .env("TMUX_TMPDIR", env.tmp.path())
        .env("TMUX_PANE", pane_id)
        .env("AGENT_WORKSPACE_NAME", "test-ws")
        .env("AGENT_WORKSPACE", env.workspaces_dir.join("test-ws"))
        .env("AW_DASH_NOTIFY", "0") // suppress system notifications in tests
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .env("TZ", "UTC");
    if !stdin.is_empty() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    if !stdin.is_empty() {
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    }
    child.wait_with_output().unwrap()
}

fn read_state(env: &TestEnv, pane_id: &str) -> serde_json::Value {
    let path = env.state_dir.join("panes").join(format!("{}.json", pane_id));
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing state file at {}", path.display()));
    serde_json::from_str(&raw).unwrap()
}

// ---- hook → state file ----

#[test]
fn claude_user_prompt_submit_writes_working() {
    let env = TestEnv::new();
    let out = run_with_pane(
        &env,
        "%42",
        &["hook", "--agent", "claude", "--event", "UserPromptSubmit"],
        r#"{"prompt":"fix the auth middleware"}"#,
    );
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let s = read_state(&env, "%42");
    assert_eq!(s["status"], "working");
    assert_eq!(s["agent"], "claude");
    assert_eq!(s["last_event"], "UserPromptSubmit");
    assert_eq!(s["last_prompt"], "fix the auth middleware");
    assert_eq!(s["workspace"], "test-ws");
    assert_eq!(s["pane_id"], "%42");
}

#[test]
fn claude_stop_transitions_to_idle_preserving_prompt() {
    let env = TestEnv::new();
    let _ = run_with_pane(
        &env,
        "%42",
        &["hook", "--agent", "claude", "--event", "UserPromptSubmit"],
        r#"{"prompt":"keep me"}"#,
    );
    let _ = run_with_pane(
        &env,
        "%42",
        &["hook", "--agent", "claude", "--event", "Stop"],
        "",
    );
    let s = read_state(&env, "%42");
    assert_eq!(s["status"], "idle");
    assert_eq!(s["last_event"], "Stop");
    assert_eq!(s["last_prompt"], "keep me");
}

#[test]
fn claude_notification_transitions_to_waiting() {
    let env = TestEnv::new();
    let _ = run_with_pane(
        &env,
        "%42",
        &["hook", "--agent", "claude", "--event", "Notification"],
        "",
    );
    let s = read_state(&env, "%42");
    assert_eq!(s["status"], "waiting");
}

#[test]
fn codex_session_start_seeds_idle() {
    let env = TestEnv::new();
    let _ = run_with_pane(
        &env,
        "%99",
        &["hook", "--agent", "codex", "--event", "SessionStart"],
        "",
    );
    let s = read_state(&env, "%99");
    assert_eq!(s["status"], "idle");
    assert_eq!(s["agent"], "codex");
}

#[test]
fn pi_agent_start_writes_working() {
    let env = TestEnv::new();
    let _ = run_with_pane(
        &env,
        "%7",
        &["hook", "--agent", "pi", "--event", "agent_start"],
        "",
    );
    let s = read_state(&env, "%7");
    assert_eq!(s["status"], "working");
    assert_eq!(s["agent"], "pi");
}

#[test]
fn unknown_event_is_silent_noop() {
    let env = TestEnv::new();
    let out = run_with_pane(
        &env,
        "%42",
        &["hook", "--agent", "claude", "--event", "DoesNotExist"],
        "",
    );
    assert_eq!(out.status.code(), Some(0));
    let path = env.state_dir.join("panes/%42.json");
    assert!(!path.exists(), "no state file should be written for unknown events");
}

#[test]
fn hook_outside_tmux_is_silent_noop() {
    let env = TestEnv::new();
    // Use env.run (no TMUX_PANE) — hook should silently exit 0.
    let out = env.run(Bin::Rust, &["hook", "--agent", "claude", "--event", "Stop"]);
    assert_eq!(out.status.code(), Some(0));
    let panes = env.state_dir.join("panes");
    let count = std::fs::read_dir(&panes).map(|r| r.count()).unwrap_or(0);
    assert_eq!(count, 0, "no state files should be written outside tmux");
}

// ---- aw dash json ----

#[test]
fn dash_json_dumps_all_panes() {
    let env = TestEnv::new();
    let _ = run_with_pane(&env, "%1",
        &["hook", "--agent", "claude", "--event", "UserPromptSubmit"],
        r#"{"prompt":"a"}"#);
    let _ = run_with_pane(&env, "%2",
        &["hook", "--agent", "codex", "--event", "UserPromptSubmit"],
        r#"{"prompt":"b"}"#);
    let out = env.run(Bin::Rust, &["dash", "json"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    let panes: Vec<&str> = arr.iter().map(|e| e["pane_id"].as_str().unwrap()).collect();
    assert!(panes.contains(&"%1") && panes.contains(&"%2"));
}

// ---- aw dash status-line ----

#[test]
fn dash_status_line_summary() {
    let env = TestEnv::new();
    let _ = run_with_pane(&env, "%1",
        &["hook", "--agent", "claude", "--event", "UserPromptSubmit"], "");
    let _ = run_with_pane(&env, "%2",
        &["hook", "--agent", "claude", "--event", "Notification"], "");
    let _ = run_with_pane(&env, "%3",
        &["hook", "--agent", "claude", "--event", "Stop"], "");
    let out = env.run(Bin::Rust, &["dash", "status-line"]);
    assert_eq!(out.status.code(), Some(0));
    let line = String::from_utf8_lossy(&out.stdout);
    assert!(line.contains("1 working"), "{}", line);
    assert!(line.contains("1 waiting"), "{}", line);
    assert!(line.contains("1 idle"), "{}", line);
}

#[test]
fn dash_status_line_empty_when_no_state() {
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["dash", "status-line"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).trim().is_empty());
}

// ---- aw dash park ----

#[test]
fn dash_park_toggles_sentinel() {
    let env = TestEnv::new();
    let _ = run_with_pane(&env, "%5",
        &["hook", "--agent", "claude", "--event", "Stop"], "");
    let out = env.run(Bin::Rust, &["dash", "park", "--pane", "%5"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(env.state_dir.join("parked/%5").exists());
    let out2 = env.run(Bin::Rust, &["dash", "park", "--pane", "%5"]);
    assert_eq!(out2.status.code(), Some(0));
    assert!(!env.state_dir.join("parked/%5").exists());
}

#[test]
fn dash_park_no_pane_outside_tmux_fails() {
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["dash", "park"]);
    assert_ne!(out.status.code(), Some(0));
}

// ---- aw dash next-ready ----
//
// Without a real tmux server, `tmux switch-client` will fail silently. We
// only assert that the command exits 0 (it shouldn't error when tmux is
// missing). The pane-selection logic itself is tested via the snapshot.

#[test]
fn dash_next_ready_says_all_clear_when_empty() {
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["dash", "next-ready"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("All clear"), "{}", stdout);
}

#[test]
fn dash_park_excludes_from_status_counts() {
    let env = TestEnv::new();
    let _ = run_with_pane(&env, "%9",
        &["hook", "--agent", "claude", "--event", "Notification"], "");
    let before = String::from_utf8_lossy(
        &env.run(Bin::Rust, &["dash", "status-line"]).stdout,
    )
    .into_owned();
    assert!(before.contains("1 waiting"), "{}", before);

    let _ = env.run(Bin::Rust, &["dash", "park", "--pane", "%9"]);
    let after = String::from_utf8_lossy(
        &env.run(Bin::Rust, &["dash", "status-line"]).stdout,
    )
    .into_owned();
    assert!(!after.contains("1 waiting"), "parked panes should be excluded: {}", after);
}
