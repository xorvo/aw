//! tmux control operations for the remote daemon: screen capture,
//! key/text/paste injection, and fit-to-phone window resizing.
//!
//! Security posture (mirrors the Node prototype): pane targets are
//! validated against the live snapshot by the caller before reaching
//! here, named keys go through an allowlist, and every tmux invocation
//! is exec-with-arg-array — no shell string is ever built from input.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};

fn tmux(args: &[&str]) -> Result<String> {
    let out = Command::new("tmux")
        .args(args)
        .output()
        .context("spawning tmux")?;
    if !out.status.success() {
        bail!("tmux {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Run tmux with `input` piped to stdin (for `load-buffer -`).
fn tmux_stdin(args: &[&str], input: &str) -> Result<()> {
    let mut child = Command::new("tmux")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawning tmux")?;
    // stdin is piped, so take() can't return None.
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).context("writing tmux stdin")?;
    }
    let out = child.wait_with_output().context("waiting for tmux")?;
    if !out.status.success() {
        bail!("tmux {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// `capture-pane` with ANSI colors and `lines` rows of scrollback
/// (clamped 10..=400, default 60 — same as the prototype).
pub fn capture_screen(pane: &str, lines: Option<&str>) -> Result<String> {
    let n = lines
        .and_then(|l| l.parse::<i64>().ok())
        .unwrap_or(60)
        .clamp(10, 400);
    tmux(&["capture-pane", "-t", pane, "-p", "-e", "-S", &format!("-{}", n)])
}

/// Named keys a client may send. Everything else must arrive as literal
/// text so a malicious client can't smuggle tmux key syntax.
const KEY_ALLOW: &[&str] = &[
    "Enter", "Escape", "Up", "Down", "Left", "Right", "Tab", "BSpace",
    "C-c", "C-d", "C-u", "Space", "y", "n", "1", "2", "3", "4",
];

/// The `/api/keys` body: any combination of paste / text / key / submit,
/// applied in that order (matching the prototype).
pub struct KeysRequest<'a> {
    pub text: Option<&'a str>,
    pub key: Option<&'a str>,
    pub submit: bool,
    pub paste: Option<&'a str>,
}

pub fn send_keys(pane: &str, req: &KeysRequest) -> Result<()> {
    if let Some(paste) = req.paste.filter(|p| !p.is_empty()) {
        // bracketed paste via a private buffer: multi-line / markdown
        // arrives as one paste (no premature submit, no shell involvement)
        tmux_stdin(&["load-buffer", "-b", "aw-remote", "-"], paste)?;
        tmux(&["paste-buffer", "-d", "-p", "-b", "aw-remote", "-t", pane])?;
    }
    if let Some(text) = req.text.filter(|t| !t.is_empty()) {
        tmux(&["send-keys", "-t", pane, "-l", "--", text])?;
    }
    if let Some(key) = req.key {
        if !KEY_ALLOW.contains(&key) {
            bail!("key not allowed: {}", key);
        }
        tmux(&["send-keys", "-t", pane, key])?;
    }
    if req.submit {
        tmux(&["send-keys", "-t", pane, "Enter"])?;
    }
    Ok(())
}

/// Fit-to-phone window sizing. Remembers each window's original
/// `window-size` option so `unfit` can restore it exactly ('' = the
/// option was unset). Shared across requests — one map per server.
#[derive(Default)]
pub struct FitState {
    orig: Mutex<HashMap<String, String>>,
}

fn window_for_pane(pane: &str) -> Result<String> {
    Ok(tmux(&["display-message", "-p", "-t", pane, "#{window_id}"])?.trim().to_string())
}

impl FitState {
    pub fn fit(&self, pane: &str, cols: i64, rows: i64) -> Result<(i64, i64)> {
        let win = window_for_pane(pane)?;
        {
            // `entry` only inserts on first fit, so re-fitting an already
            // manual-sized window can't overwrite the true original.
            let mut orig = self.orig.lock().unwrap_or_else(|e| e.into_inner());
            orig.entry(win.clone()).or_insert_with(|| {
                tmux(&["show-options", "-w", "-t", &win, "-v", "window-size"])
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default()
            });
        }
        let c = if cols > 0 { cols } else { 80 }.clamp(20, 500);
        let r = if rows > 0 { rows } else { 24 }.clamp(8, 200);
        tmux(&["set-option", "-w", "-t", &win, "window-size", "manual"])?;
        tmux(&["resize-window", "-t", &win, "-x", &c.to_string(), "-y", &r.to_string()])?;
        Ok((c, r))
    }

    pub fn unfit(&self, pane: &str) -> Result<()> {
        let win = window_for_pane(pane)?;
        let orig = {
            let mut map = self.orig.lock().unwrap_or_else(|e| e.into_inner());
            match map.remove(&win) {
                Some(o) => o,
                None => return Ok(()), // never fitted by us
            }
        };
        if orig.is_empty() {
            tmux(&["set-option", "-w", "-u", "-t", &win, "window-size"])?;
        } else {
            tmux(&["set-option", "-w", "-t", &win, "window-size", &orig])?;
        }
        Ok(())
    }
}
