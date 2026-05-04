//! TUI rendering snapshots via ratatui's `TestBackend`. We construct an
//! `App` from a known snapshot, draw to a fixed-size buffer, and assert on
//! the rendered text content.
//!
//! This relies on `aw`'s internal modules being reachable from the integration
//! test. They aren't (binary crate), so we re-implement a minimal harness
//! that drives the binary's `dash sidebar` text renderer through stdout
//! and snapshots that.

mod common;

use common::{Bin, TestEnv};
use std::process::Command;

/// Drive the binary's text-mode sidebar renderer for one tick: spawn
/// `_sidebar-loop` for ~250ms, read its stdout, kill it. The renderer
/// repaints with ANSI escape codes; we strip them before snapshotting.
#[test]
fn sidebar_renders_workspaces_and_panes() {
    let env = TestEnv::new();
    // Seed three panes via the hook code path. Codex doesn't have a
    // waiting-state event in the hook map, so the waiting row uses Claude's
    // Notification event.
    fire_hook(&env, "%1", "ws-a", "claude", "UserPromptSubmit", r#"{"prompt":"do a"}"#);
    fire_hook(&env, "%2", "ws-a", "claude", "Notification", "");
    fire_hook(&env, "%3", "ws-b", "pi", "agent_end", "");

    // Run the sidebar-loop briefly via timeout(1).
    let path = format!(
        "{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
        env.fake_bin.display()
    );
    // No `timeout` on macOS by default; drive via a thread that kills.
    let child = std::process::Command::new(Bin::Rust.path())
        .args(["_sidebar-loop"])
        .current_dir(&env.home)
        .env_clear()
        .env("HOME", &env.home)
        .env("PATH", &path)
        .env("AW_INSTALL_DIR", &env.install_dir)
        .env("AW_WORKSPACES_DIR", &env.workspaces_dir)
        .env("AW_BIN_DIR", &env.bin_dir)
        .env("AW_CONFIG_FILE", &env.config_path)
        .env("AW_STATE_DIR", &env.state_dir)
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .env("TZ", "UTC")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => panic!("spawn _sidebar-loop: {}", e),
    };

    // Give it a moment to do the first paint, then kill.
    std::thread::sleep(std::time::Duration::from_millis(400));
    let _ = child.kill();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();

    // Strip ANSI escapes and only keep the LAST repaint (everything after
    // the last clear-screen sequence \x1b[2J\x1b[H).
    let last_paint = stdout
        .rsplit_once("\x1b[2J\x1b[H")
        .map(|(_, after)| after.to_string())
        .unwrap_or(stdout);
    let cleaned = strip_ansi(&last_paint);

    // Sanity-check key fragments rather than full byte-equality (timestamps
    // change). insta snapshot is fine here too; using assertions for clarity.
    assert!(cleaned.contains("ws-a"), "missing ws-a:\n{}", cleaned);
    assert!(cleaned.contains("ws-b"), "missing ws-b:\n{}", cleaned);
    assert!(cleaned.contains("claude"), "missing claude:\n{}", cleaned);
    assert!(cleaned.contains("pi"), "missing pi:\n{}", cleaned);
    // 1 working, 1 waiting (both claude on ws-a), 1 idle (pi on ws-b).
    assert!(cleaned.contains("⚡1"), "wrong counts:\n{}", cleaned);
    assert!(cleaned.contains("⏸1"), "wrong counts:\n{}", cleaned);
    assert!(cleaned.contains("✓1"), "wrong counts:\n{}", cleaned);
}

fn fire_hook(
    env: &TestEnv,
    pane_id: &str,
    workspace: &str,
    agent: &str,
    event: &str,
    stdin: &str,
) {
    use std::io::Write;
    let path = format!(
        "{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
        env.fake_bin.display()
    );
    let mut child = Command::new(Bin::Rust.path())
        .args(["hook", "--agent", agent, "--event", event])
        .current_dir(&env.home)
        .env_clear()
        .env("HOME", &env.home)
        .env("PATH", path)
        .env("AW_INSTALL_DIR", &env.install_dir)
        .env("AW_WORKSPACES_DIR", &env.workspaces_dir)
        .env("AW_BIN_DIR", &env.bin_dir)
        .env("AW_CONFIG_FILE", &env.config_path)
        .env("AW_STATE_DIR", &env.state_dir)
        .env("AGENT_WORKSPACE_NAME", workspace)
        .env("TMUX_PANE", pane_id)
        .env("AW_DASH_NOTIFY", "0")
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .env("TZ", "UTC")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    if !stdin.is_empty() {
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).ok();
    }
    let _ = child.wait();
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && matches!(chars.peek(), Some('[')) {
            chars.next();
            while let Some(&p) = chars.peek() {
                chars.next();
                if matches!(p, '\x40'..='\x7E') {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}
