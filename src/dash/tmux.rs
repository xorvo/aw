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
///
/// `window_name` is what tmux's status bar shows (`#W`) — stable, often
/// human-friendly via tmux's automatic-rename heuristics. `pane_title` is
/// what programs can set via OSC-2 (`select-pane -T`). `command` is the
/// current foreground process; volatile (flickers as subprocesses come
/// and go), used as a last-resort label.
#[derive(Debug, Clone)]
pub struct PaneInfo {
    pub pane_id: String,
    pub session: String,
    pub window_name: String,
    pub pane_title: String,
    pub command: String,
    pub path: String,
}

/// Result of asking tmux for the live pane list. The distinction between
/// "tmux is healthy and there are zero panes" and "tmux didn't answer at
/// all" matters: the snapshot loader uses tmux as the authoritative source
/// for which panes exist, so it must not treat an unreachable tmux as
/// "everything is dead."
#[derive(Debug, Clone)]
pub enum PaneListing {
    /// We got a successful response. The Vec is the truth: it contains
    /// every pane on the local tmux server (possibly zero).
    Tmux(Vec<PaneInfo>),
    /// tmux command failed or wasn't on PATH. Caller should fall back to
    /// file-only state and accept the staleness risk.
    Unavailable,
}

/// Enumerate every pane on the local tmux server with metadata. Tabs
/// separate fields and don't appear in any of the fields we read, so a
/// simple split is safe.
pub fn list_panes_with_metadata() -> PaneListing {
    let out = Command::new("tmux")
        .args([
            "list-panes",
            "-a",
            "-F",
            // 6 tab-separated fields. Tabs don't appear in any of these.
            "#{pane_id}\t#{session_name}\t#{window_name}\t#{pane_title}\t#{pane_current_command}\t#{pane_current_path}",
        ])
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => PaneListing::Tmux(
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(parse_pane_line)
                .collect(),
        ),
        // Distinguish "tmux not running" (exit non-zero with the typical
        // "no server running" message on stderr) from "tmux missing": both
        // collapse to Unavailable here. Snapshot::load handles both the
        // same way (file-only fallback).
        _ => PaneListing::Unavailable,
    }
}

fn parse_pane_line(line: &str) -> Option<PaneInfo> {
    let parts: Vec<&str> = line.splitn(6, '\t').collect();
    if parts.len() != 6 {
        return None;
    }
    Some(PaneInfo {
        pane_id: parts[0].into(),
        session: parts[1].into(),
        window_name: parts[2].into(),
        pane_title: parts[3].into(),
        command: parts[4].into(),
        path: parts[5].into(),
    })
}

/// Resolve a stable, human-friendly label for a pane from its tmux info.
///
/// Matches what users see in their tmux status bar (`#W`). The cascade:
///
///   1. `window_name` — what tmux's status bar displays. Auto-rename
///      heuristics pick a stable parent process name (`claude`), not
///      whatever subprocess happens to be foregrounded *right now*
///      (`2.1.128`). Most users have nice window names because of this.
///   2. `pane_title` if it's been explicitly set (i.e. doesn't look like
///      the system hostname, which is tmux's default).
///   3. `pane_current_command` as last resort. Volatile but always
///      something.
pub fn label_from_tmux(p: &PaneInfo) -> String {
    let win = p.window_name.trim();
    if !win.is_empty() {
        return win.to_string();
    }
    let title = p.pane_title.trim();
    if !title.is_empty() && !looks_like_hostname(title) {
        return title.to_string();
    }
    p.command.trim().to_string()
}

/// Heuristic for "this pane_title is the unset default" — tmux defaults
/// pane_title to the result of `gethostname()`, which on macOS is usually
/// `<short>.local`, on Linux either FQDN or short. We accept either form.
fn looks_like_hostname(s: &str) -> bool {
    let hosts = [
        std::process::Command::new("hostname")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()),
        std::process::Command::new("hostname")
            .arg("-s")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()),
    ];
    hosts.iter().flatten().any(|h| !h.is_empty() && h == s)
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
