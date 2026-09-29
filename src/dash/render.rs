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

/// How long a `working` claim stays believable.
///
/// A working agent fires a hook every few seconds (each tool call is a
/// `PreToolUse`), so silence this long means the claim was never renewed —
/// typically the session errored out, which sends no `Stop`. Generous enough to
/// cover a single long model response.
pub const STALE_WORKING_AFTER: u64 = 10 * 60;

/// What we can honestly say a pane is doing *now*, as opposed to what it last
/// claimed.
///
/// [`Status`] is a latch: a hook sets it and nothing revises it. That is fine
/// for `waiting` and `idle`, which are resting states an agent stays in, but
/// `working` is a claim about ongoing activity and it expires.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Shown {
    Working,
    Waiting,
    Idle,
    /// Claimed `working`, then went silent past [`STALE_WORKING_AFTER`]. We are
    /// not guessing why — only that the claim is no longer good.
    Stalled,
}

/// Decay a recorded status into what is still true.
///
/// `last_activity == 0` means no hook ever fired, so there is no claim to
/// expire and nothing to downgrade.
pub fn shown_status(status: Status, last_activity: u64, now: u64) -> Shown {
    match status {
        Status::Waiting => Shown::Waiting,
        Status::Idle => Shown::Idle,
        Status::Working => {
            let expired = last_activity != 0
                && now.saturating_sub(last_activity) >= STALE_WORKING_AFTER;
            if expired { Shown::Stalled } else { Shown::Working }
        }
    }
}

/// Status icon for a derived state.
///
///   stalled: nf-fa-warning (\u{F071}) — said working, stopped saying it
pub fn shown_glyph(s: Shown) -> &'static str {
    match s {
        Shown::Working => status_glyph(Status::Working),
        Shown::Waiting => status_glyph(Status::Waiting),
        Shown::Idle => status_glyph(Status::Idle),
        Shown::Stalled => match icon_set() {
            IconSet::NerdFont => "\u{F071}",
            IconSet::Ascii => "!",
        },
    }
}

/// The colour a derived state is drawn in, as an RGB triple.
///
/// Returned as plain numbers rather than a ratatui type so the same choice
/// serves the TUI, the sidebar and the web client. Amber for working, red for
/// waiting because it needs you, magenta for stalled so it never reads as
/// either, green for idle.
pub fn shown_rgb(s: Shown) -> (u8, u8, u8) {
    match s {
        Shown::Working => (210, 153, 34),
        Shown::Waiting => (248, 81, 73),
        Shown::Stalled => (188, 116, 222),
        Shown::Idle => (63, 185, 80),
    }
}

/// Lowercase label, shared by every surface so they can't disagree.
pub fn shown_label(s: Shown) -> &'static str {
    match s {
        Shown::Working => "working",
        Shown::Waiting => "waiting",
        Shown::Idle => "idle",
        Shown::Stalled => "stalled",
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

/// Glyph for pinned workspaces.
///
///   pinned:  nf-fa-star  (\u{F005})
pub fn pinned_glyph() -> &'static str {
    match icon_set() {
        IconSet::NerdFont => "\u{F005}",
        IconSet::Ascii => "*",
    }
}

/// "5s", "1m", "14m", "2h", "3d" — short relative time for the row line.
/// `0` means "never recorded" (a pane no hook has fired in) → "—", not the
/// age of the Unix epoch.
pub fn humanize_age(epoch: u64) -> String {
    if epoch == 0 {
        return "—".to_string();
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_expires_but_resting_states_do_not() {
        let now = 1_800_000_000u64;
        let fresh = now - 5;
        let old = now - STALE_WORKING_AFTER - 1;

        // A working claim is only good while it keeps being renewed.
        assert_eq!(shown_status(Status::Working, fresh, now), Shown::Working);
        assert_eq!(shown_status(Status::Working, old, now), Shown::Stalled);

        // Waiting and idle are states an agent rests in, so age means nothing.
        assert_eq!(shown_status(Status::Waiting, old, now), Shown::Waiting);
        assert_eq!(shown_status(Status::Idle, old, now), Shown::Idle);

        // No hook ever fired: no claim exists, so there is nothing to expire.
        assert_eq!(shown_status(Status::Working, 0, now), Shown::Working);
    }

    #[test]
    fn exactly_at_the_threshold_counts_as_stale() {
        let now = 1_800_000_000u64;
        assert_eq!(
            shown_status(Status::Working, now - STALE_WORKING_AFTER, now),
            Shown::Stalled
        );
        assert_eq!(
            shown_status(Status::Working, now - STALE_WORKING_AFTER + 1, now),
            Shown::Working
        );
    }

    #[test]
    fn humanize_age_treats_zero_as_unknown() {
        assert_eq!(humanize_age(0), "—");
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        assert_eq!(humanize_age(now - 5), "5s");
        assert_eq!(humanize_age(now - 3 * 86_400), "3d");
    }
}
