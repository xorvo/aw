//! Shared rendering helpers used by the popup TUI, sidebar, and status-line.
//!
//! The data side here; the visual side lives in `dash/tui/`.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::dash::state::{PaneState, Status};

/// Status icon + label.
pub fn status_glyph(s: Status) -> &'static str {
    match s {
        Status::Working => "⚡",
        Status::Waiting => "⏸",
        Status::Idle => "✓",
    }
}

/// "5s", "1m", "14m", "2h", "3d" — short relative time for the row line.
pub fn humanize_age(epoch: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let delta = now.saturating_sub(epoch);
    if delta < 60 {
        format!("{}s", delta)
    } else if delta < 3600 {
        format!("{}m", delta / 60)
    } else if delta < 86_400 {
        format!("{}h", delta / 3600)
    } else {
        format!("{}d", delta / 86_400)
    }
}

/// Group panes by workspace, returning a Vec of (workspace_name, panes-in-it).
/// Workspace order matches first-appearance in the input slice.
#[allow(dead_code)] // helper kept for future use; the TUI groups in `app`
pub fn group_by_workspace(panes: &[PaneState]) -> Vec<(String, Vec<&PaneState>)> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::BTreeMap<String, Vec<&PaneState>> =
        std::collections::BTreeMap::new();
    for p in panes {
        if !order.contains(&p.workspace) {
            order.push(p.workspace.clone());
        }
        groups.entry(p.workspace.clone()).or_default().push(p);
    }
    order
        .into_iter()
        .map(|k| (k.clone(), groups.remove(&k).unwrap_or_default()))
        .collect()
}
