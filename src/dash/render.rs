//! Shared rendering helpers used by the popup TUI, sidebar, and status-line.
//!
//! The data side here; the visual side lives in `dash/tui/`.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::dash::state::{PaneState, Status};

/// Which icon set to render. Nerd Font is the default — modern terminals
/// with a Nerd-Font-patched font (FiraCode Nerd Font, JetBrainsMono Nerd
/// Font, MesloLGS NF, etc.) get crisp single-cell glyphs that line up.
/// ASCII is a fallback for environments without a patched font; opt in via
/// `AW_DASH_ICONS=ascii`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IconSet {
    NerdFont,
    Ascii,
}

pub fn icon_set() -> IconSet {
    match std::env::var("AW_DASH_ICONS").as_deref() {
        Ok("ascii") => IconSet::Ascii,
        _ => IconSet::NerdFont,
    }
}

/// Status icon. Nerd Font codepoints below are FontAwesome glyphs in the
/// PUA range (rendered as single-cell by Nerd-Font-patched monospace
/// terminals), so `<glyph><space><digit>` always lines up.
///
///   working: nf-fa-bolt   (\u{F0E7})
///   waiting: nf-fa-bell   (\u{F0F3})
///   idle:    nf-fa-check  (\u{F00C})
pub fn status_glyph(s: Status) -> &'static str {
    match icon_set() {
        IconSet::NerdFont => match s {
            Status::Working => "\u{F0E7}",
            Status::Waiting => "\u{F0F3}",
            Status::Idle => "\u{F00C}",
        },
        IconSet::Ascii => match s {
            Status::Working => ">",
            Status::Waiting => "!",
            Status::Idle => ".",
        },
    }
}

/// Glyph for the parked indicator (a sentinel state, not a `Status`).
///
///   parked:  nf-fa-pause  (\u{F04C})
pub fn parked_glyph() -> &'static str {
    match icon_set() {
        IconSet::NerdFont => "\u{F04C}",
        IconSet::Ascii => "_",
    }
}

/// Glyph for dormant workspaces (on disk, no live tmux session).
///
///   dormant:  nf-fa-folder-o  (\u{F114})
pub fn dormant_glyph() -> &'static str {
    match icon_set() {
        IconSet::NerdFont => "\u{F114}",
        IconSet::Ascii => "o",
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
