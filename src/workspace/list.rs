//! `aw list` — enumerate workspaces with base + creation timestamp.
//!
//! Mirrors the bash CLI's output:
//!   📂 Workspaces in: <dir>
//!
//!     • foo (base: default, created: <stamp>)
//!     ● bar (base: default, created: <stamp>)   <- green, has live aw-bar tmux
//!
//! When the workspaces directory is missing, bash prints "No workspaces
//! directory found"; when it's present but empty, "No workspaces found" plus
//! a hint. We keep both code paths.

use std::collections::HashSet;
use std::process::Command;

use anyhow::Result;

use crate::paths::Paths;
use crate::workspace::meta::WorkspaceMeta;

pub fn run() -> Result<()> {
    let paths = Paths::from_env()?;
    println!("📂 Workspaces in: {}", paths.workspaces_dir.display());
    println!();

    if !paths.workspaces_dir.is_dir() {
        println!("  No workspaces directory found");
        return Ok(());
    }

    let live_sessions = live_aw_sessions();
    let mut entries = Vec::new();
    for dirent in std::fs::read_dir(&paths.workspaces_dir)? {
        let dirent = match dirent {
            Ok(d) => d,
            Err(_) => continue,
        };
        if !dirent.path().is_dir() {
            continue;
        }
        if let Some(meta) = WorkspaceMeta::read(&dirent.path()) {
            entries.push(meta);
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    if entries.is_empty() {
        println!("  No workspaces found");
        println!("  💡 Run 'aw create <name>' to create your first workspace");
        return Ok(());
    }

    for m in &entries {
        let session = format!("aw-{}", m.name);
        let active = live_sessions.contains(&session);
        let bullet = if active { "\x1b[32m●\x1b[0m" } else { "•" };
        println!(
            "  {} {} (base: {}, created: {})",
            bullet, m.name, m.base, m.created
        );
    }
    Ok(())
}

/// Names of currently-live tmux sessions, restricted to ones starting with
/// `aw-`. Empty if tmux isn't on PATH or no server is running.
fn live_aw_sessions() -> HashSet<String> {
    let out = Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output();
    let raw = match out {
        Ok(o) if o.status.success() => o.stdout,
        _ => return HashSet::new(),
    };
    String::from_utf8_lossy(&raw)
        .lines()
        .filter(|s| s.starts_with("aw-"))
        .map(String::from)
        .collect()
}
