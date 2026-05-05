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

/// Per-pane info pulled from tmux. Used by the dashboard to surface panes
/// that haven't fired any agent hook (plain shells, vim, etc.) so the user
/// sees their entire `aw-*` workload — not just rows backed by state files.
#[derive(Debug, Clone)]
pub struct PaneInfo {
    pub pane_id: String,
    pub session: String,
    pub command: String,
    pub path: String,
}

/// Enumerate every pane on the local tmux server with metadata. Empty if
/// tmux isn't running. Tabs separate fields and don't appear in any of the
/// fields we read, so a simple split is safe.
pub fn list_panes_with_metadata() -> Vec<PaneInfo> {
    let out = Command::new("tmux")
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{pane_id}\t#{session_name}\t#{pane_current_command}\t#{pane_current_path}",
        ])
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter_map(parse_pane_line)
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_pane_line(line: &str) -> Option<PaneInfo> {
    let parts: Vec<&str> = line.splitn(4, '\t').collect();
    if parts.len() != 4 {
        return None;
    }
    Some(PaneInfo {
        pane_id: parts[0].into(),
        session: parts[1].into(),
        command: parts[2].into(),
        path: parts[3].into(),
    })
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
