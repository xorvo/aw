//! Prune state files for tmux panes that no longer exist.

use std::collections::HashSet;

use anyhow::Result;

use crate::dash::{panes_dir, parked_dir, tmux};

/// Returns the number of stale files removed.
pub fn run() -> Result<usize> {
    let live: HashSet<String> = tmux::list_pane_ids().into_iter().collect();
    let live_pid = tmux::server_pid();

    let mut removed = 0;

    let panes = panes_dir()?;
    if let Ok(read) = std::fs::read_dir(&panes) {
        for d in read.flatten() {
            let path = d.path();
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            // Skip non-`.json`.
            if path.extension().map_or(true, |e| e != "json") {
                continue;
            }
            // If tmux isn't running, `live` is empty. Don't nuke files just
            // because tmux is down — only prune when we have a valid live set.
            if live.is_empty() {
                continue;
            }
            // Exactly the dashboard's rule, via the same function: only state
            // written by the tmux server we are talking to may be collected,
            // and only once tmux confirms that pane specifically is gone. One
            // decision point, so the explicit command can never be laxer than
            // the automatic one.
            let recorded = crate::dash::state::PaneState::read(&path)
                .ok()
                .and_then(|s| s.server_pid);
            if crate::dash::state::should_drop(
                &stem,
                recorded,
                live_pid,
                &live,
                tmux::pane_is_gone,
                crate::dash::state::pid_is_alive,
                true, // explicit request: also clean up pre-stamp leftovers
            ) {
                if std::fs::remove_file(&path).is_ok() {
                    removed += 1;
                }
            }
        }
    }

    // Same logic for parked sentinels.
    let parked = parked_dir()?;
    if let Ok(read) = std::fs::read_dir(&parked) {
        if !live.is_empty() {
            for d in read.flatten() {
                let name = d.file_name().to_string_lossy().into_owned();
                if !live.contains(&name) {
                    let _ = std::fs::remove_file(d.path());
                }
            }
        }
    }

    Ok(removed)
}
