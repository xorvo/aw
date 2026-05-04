//! Thin tmux wrappers.

use std::process::{Command, Stdio};

/// Current pane id from `$TMUX_PANE`, or None if not in tmux.
pub fn current_pane() -> Option<String> {
    std::env::var("TMUX_PANE").ok().filter(|s| !s.is_empty())
}

/// Resolve session name for a pane id. Empty string if tmux/pane is gone.
pub fn pane_session(pane_id: &str) -> String {
    Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "-t",
            pane_id,
            "#{session_name}",
        ])
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// All live pane ids on the local tmux server. Empty if tmux not running.
pub fn list_pane_ids() -> Vec<String> {
    let out = Command::new("tmux")
        .args(["list-panes", "-a", "-F", "#{pane_id}"])
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// `tmux switch-client -t <pane>; tmux select-pane -t <pane>`.
pub fn switch_to_pane(pane_id: &str) {
    let _ = Command::new("tmux")
        .args(["switch-client", "-t", pane_id])
        .status();
    let _ = Command::new("tmux")
        .args(["select-pane", "-t", pane_id])
        .status();
}
