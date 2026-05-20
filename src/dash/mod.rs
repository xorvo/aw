//! `aw dash ...` — tmux-based agent dashboard.
//!
//! State is kept as one JSON file per tmux pane at
//! `~/.cache/aw/panes/<pane_id>.json` (overridable via `$AW_STATE_DIR`),
//! written by `aw hook` from agent hooks (Claude Code / Codex / pi).

use std::path::PathBuf;

use anyhow::Result;

pub mod gc;
pub mod notify;
pub mod render;
pub mod state;
pub mod tmux;
pub mod tui;

/// `~/.cache/aw/panes/` (or `$AW_STATE_DIR/panes/`).
pub fn panes_dir() -> Result<PathBuf> {
    Ok(state_root()?.join("panes"))
}

/// `~/.cache/aw/parked/` — sentinel files for parked panes.
pub fn parked_dir() -> Result<PathBuf> {
    Ok(state_root()?.join("parked"))
}

/// `~/.cache/aw/pinned/` — sentinel files for pinned workspaces.
/// Each file is named after the workspace; presence means pinned.
pub fn pinned_dir() -> Result<PathBuf> {
    Ok(state_root()?.join("pinned"))
}

fn state_root() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("AW_STATE_DIR") {
        return Ok(PathBuf::from(p));
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    Ok(home.join(".cache/aw"))
}

/// `aw dash json` — print the full snapshot.
pub fn cmd_json() -> Result<()> {
    let snap = state::Snapshot::load()?;
    println!("{}", serde_json::to_string_pretty(&snap.entries)?);
    Ok(())
}

/// `aw dash gc` — prune state files for dead panes.
pub fn cmd_gc() -> Result<()> {
    let removed = gc::run()?;
    println!("Removed {} stale state file(s)", removed);
    Ok(())
}

/// `aw dash park [--pane <id>]` — toggle parked sentinel.
pub fn cmd_park(pane_override: Option<&str>) -> Result<()> {
    let pane = match pane_override {
        Some(p) => p.to_string(),
        None => match tmux::current_pane() {
            Some(p) => p,
            None => {
                eprintln!("❌ Not inside tmux; specify --pane <id>.");
                std::process::exit(1);
            }
        },
    };
    let dir = parked_dir()?;
    std::fs::create_dir_all(&dir)?;
    let sentinel = dir.join(sanitize_pane(&pane));
    let glyph = crate::dash::render::parked_glyph();
    if sentinel.exists() {
        std::fs::remove_file(&sentinel)?;
        println!("{}  Unparked {}", glyph, pane);
    } else {
        std::fs::write(&sentinel, "")?;
        println!("{}  Parked {}", glyph, pane);
    }
    Ok(())
}

/// `aw dash pin <workspace>` — toggle the pinned sentinel for a workspace.
/// Pinned workspaces float to the top of the dash, both in their active and
/// dormant groups.
pub fn cmd_pin(workspace: &str) -> Result<()> {
    let dir = pinned_dir()?;
    std::fs::create_dir_all(&dir)?;
    let sentinel = dir.join(sanitize_workspace(workspace));
    let glyph = crate::dash::render::pinned_glyph();
    if sentinel.exists() {
        std::fs::remove_file(&sentinel)?;
        println!("{}  Unpinned {}", glyph, workspace);
    } else {
        std::fs::write(&sentinel, "")?;
        println!("{}  Pinned {}", glyph, workspace);
    }
    Ok(())
}

/// `aw dash next-ready` — `tmux switch-client` to oldest waiting pane (or
/// failing that, oldest non-parked idle pane).
pub fn cmd_next_ready() -> Result<()> {
    let snap = state::Snapshot::load()?;
    let target = pick_next_ready(&snap);
    match target {
        Some(pane) => {
            tmux::switch_to_pane(&pane);
        }
        None => {
            println!(
                "{} All clear — no agents waiting or idle.",
                crate::dash::render::status_glyph(state::Status::Idle)
            );
        }
    }
    Ok(())
}

/// Public alias exposed to `tui` so the popup loop can reuse the selection logic.
pub fn pick_next_ready_for(snap: &state::Snapshot) -> Option<String> {
    pick_next_ready(snap)
}

fn pick_next_ready(snap: &state::Snapshot) -> Option<String> {
    let mut waiting: Vec<&state::PaneState> = snap
        .entries
        .iter()
        .filter(|s| s.status == state::Status::Waiting && !s.parked)
        .collect();
    waiting.sort_by_key(|s| s.last_activity);
    if let Some(s) = waiting.first() {
        return Some(s.pane_id.clone());
    }
    let mut idle: Vec<&state::PaneState> = snap
        .entries
        .iter()
        .filter(|s| s.status == state::Status::Idle && !s.parked)
        .collect();
    idle.sort_by_key(|s| s.last_activity);
    idle.first().map(|s| s.pane_id.clone())
}

/// `aw dash status-line` — one-line summary for tmux status-right.
pub fn cmd_status_line() -> Result<()> {
    use crate::dash::render::status_glyph;
    use crate::dash::state::Status;

    let snap = state::Snapshot::load()?;
    let (working, waiting, idle) = snap.counts();
    if waiting == 0 && working == 0 && idle == 0 {
        return Ok(());
    }
    if waiting == 0 && working == 0 {
        print!("{} all clear", status_glyph(Status::Idle));
        return Ok(());
    }
    let mut parts = Vec::new();
    if working > 0 {
        parts.push(format!("{} {} working", status_glyph(Status::Working), working));
    }
    if waiting > 0 {
        parts.push(format!("{} {} waiting", status_glyph(Status::Waiting), waiting));
    }
    if idle > 0 {
        parts.push(format!("{} {} idle", status_glyph(Status::Idle), idle));
    }
    print!("{}", parts.join("  "));
    Ok(())
}

/// Sanitize a tmux pane id (`%42`) for use as a filename. Pane ids are safe
/// already — but defensively replace `/` if anyone hands us one.
fn sanitize_pane(p: &str) -> String {
    p.replace('/', "_")
}

/// Workspace names go through `aw create` which already forbids `/`, but
/// belt-and-suspenders for paths that flow through here.
fn sanitize_workspace(w: &str) -> String {
    w.replace('/', "_")
}
