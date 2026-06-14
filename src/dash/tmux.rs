//! Thin tmux wrappers.

use std::process::{Command, Stdio};

/// Build a `tmux` command with a guaranteed UTF-8 locale.
///
/// tmux strips control characters — including the TAB separators we put
/// in `-F` formats — from its output when the locale isn't UTF-8. Under
/// launchd or cron there is no `LANG` at all, so tmux output arrives
/// tab-less and every pane row fails to parse: `aw serve` (and `aw dash`
/// from any locale-less context) showed "no active agent sessions" even
/// with sessions live. Force a UTF-8 locale for our tmux children unless
/// the environment already supplies one. Every tmux invocation in `aw`
/// goes through here.
pub(crate) fn tmux_command() -> Command {
    let mut cmd = Command::new("tmux");
    if !env_locale_is_utf8() {
        cmd.env("LC_ALL", fallback_utf8_locale());
    }
    cmd
}

/// Whether the effective locale (LC_ALL > LC_CTYPE > LANG, per POSIX) is
/// UTF-8 — the only categories that affect tmux's output encoding.
fn env_locale_is_utf8() -> bool {
    ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .map(|v| {
            let u = v.to_ascii_uppercase();
            u.contains("UTF-8") || u.contains("UTF8")
        })
        .unwrap_or(false)
}

/// A UTF-8 locale present out of the box on the target OS: `en_US.UTF-8`
/// ships on macOS; `C.UTF-8` is the portable choice on Linux.
fn fallback_utf8_locale() -> &'static str {
    if cfg!(target_os = "macos") {
        "en_US.UTF-8"
    } else {
        "C.UTF-8"
    }
}

/// Current pane id from `$TMUX_PANE`, or None if not in tmux.
pub fn current_pane() -> Option<String> {
    std::env::var("TMUX_PANE").ok().filter(|s| !s.is_empty())
}

/// Resolve session name for a pane id. Empty string if tmux/pane is gone.
pub fn pane_session(pane_id: &str) -> String {
    tmux_command()
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
    let out = tmux_command()
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
    let out = tmux_command()
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

/// Move the user's focus to `pane_id`. Behaves correctly whether or not
/// the dashboard was launched from inside tmux:
///
///   - **Inside tmux** (`$TMUX` set) — `tmux switch-client -t <pane>` is
///     instant: the existing client jumps to the target pane's session
///     and pane in one step.
///   - **Outside tmux** — `switch-client` would silently no-op (no client
///     to switch). Instead we resolve the pane's session and run
///     `tmux attach-session -t <session>` chained with a `select-pane`,
///     so the user *enters* tmux landing on the right pane. Their
///     terminal becomes the tmux client.
pub fn switch_to_pane(pane_id: &str) {
    if std::env::var_os("TMUX").is_some() {
        let _ = tmux_command()
            .args(["switch-client", "-t", pane_id])
            .status();
        let _ = tmux_command()
            .args(["select-pane", "-t", pane_id])
            .status();
        return;
    }

    // Outside tmux: resolve session from the pane, then attach to it.
    let session = pane_session(pane_id);
    if session.is_empty() {
        eprintln!("could not resolve tmux session for pane {}", pane_id);
        return;
    }
    // `;` between commands chains them in tmux's command-line grammar so
    // the select-pane runs after the attach. Note: the leading semicolon
    // must be its own arg for the shell-free Command::args path.
    let _ = tmux_command()
        .args([
            "attach-session", "-t", &session,
            ";",
            "select-pane", "-t", pane_id,
        ])
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: run a closure with a scratch locale environment, restoring
    /// whatever was there before. Serialized by the caller.
    fn with_locale_env<F: FnOnce()>(lc_all: Option<&str>, lc_ctype: Option<&str>, lang: Option<&str>, f: F) {
        let saved: Vec<(&str, Option<String>)> = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .map(|k| (*k, std::env::var(k).ok()))
            .collect();
        for k in ["LC_ALL", "LC_CTYPE", "LANG"] {
            std::env::remove_var(k);
        }
        if let Some(v) = lc_all { std::env::set_var("LC_ALL", v); }
        if let Some(v) = lc_ctype { std::env::set_var("LC_CTYPE", v); }
        if let Some(v) = lang { std::env::set_var("LANG", v); }
        f();
        for (k, v) in saved {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn locale_detected_as_utf8_from_any_category() {
        with_locale_env(Some("en_US.UTF-8"), None, None, || assert!(env_locale_is_utf8()));
        with_locale_env(None, Some("en_US.UTF-8"), None, || assert!(env_locale_is_utf8()));
        with_locale_env(None, None, Some("en_US.UTF-8"), || assert!(env_locale_is_utf8()));
        // Case/spelling variants tmux/glibc accept.
        with_locale_env(None, None, Some("C.utf8"), || assert!(env_locale_is_utf8()));
    }

    #[test]
    #[serial_test::serial]
    fn non_utf8_or_empty_locale_is_not_utf8() {
        // The launchd/cron case: nothing set at all.
        with_locale_env(None, None, None, || assert!(!env_locale_is_utf8()));
        // Explicit C/POSIX locale (where tmux drops the tab separators).
        with_locale_env(Some("C"), None, None, || assert!(!env_locale_is_utf8()));
        with_locale_env(Some("POSIX"), None, None, || assert!(!env_locale_is_utf8()));
        // LC_ALL takes precedence over a UTF-8 LANG (POSIX ordering).
        with_locale_env(Some("C"), None, Some("en_US.UTF-8"), || assert!(!env_locale_is_utf8()));
    }

    #[test]
    fn fallback_locale_is_a_real_utf8_locale_for_the_platform() {
        let l = fallback_utf8_locale();
        assert!(l.to_ascii_uppercase().contains("UTF-8") || l.to_ascii_uppercase().contains("UTF8"));
        if cfg!(target_os = "macos") {
            assert_eq!(l, "en_US.UTF-8", "macOS always ships en_US.UTF-8");
        }
    }
}
