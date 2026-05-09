//! `aw install tmux-bindings [--config <path>]` — write our key-binding
//! block to the tmux config file the user is actually using.
//!
//! ## How tmux discovers config files (mirrored here)
//!
//! From tmux's source (`tmux.c::start_cfg`), tmux probes two user-level
//! paths and loads each that exists. The probe order is:
//!
//!   1. `~/.tmux.conf`
//!   2. `$XDG_CONFIG_HOME/tmux/tmux.conf` (default
//!      `~/.config/tmux/tmux.conf` when `XDG_CONFIG_HOME` is unset/empty)
//!
//! tmux **loads both** if both exist; it does not pick one. The XDG path
//! is loaded *second*, so its bindings override the legacy file's on
//! conflict. Effective bindings come from the last file loaded.
//!
//! ## Where we install
//!
//! Given that, the "winning" file — the one whose bindings tmux ends up
//! honoring — is:
//!
//!   - **XDG path**, if it exists (it loads second, overrides).
//!   - else **legacy `~/.tmux.conf`**, if it exists.
//!   - else **XDG path**, freshly created (modern default).
//!
//! Whichever non-target file exists, we strip our marker block out of it
//! so the user doesn't end up with stale or conflicting bindings in a file
//! tmux still loads.
//!
//! Override the auto-detection with `--config <path>`.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::install::marker;

// `-b rounded` matches the rounded corners ratatui draws on the inside,
// so the popup looks like one continuous frame. Requires tmux >= 3.3 —
// older versions ignore unknown `-b` values and may error; we document
// 3.3 as the minimum in CONTRIBUTING.md.
const TMUX_BLOCK: &str = "\
bind-key a display-popup -E -w 80% -h 60% -b rounded \"aw dash\"
bind-key / display-popup -E -w 80% -h 60% -b rounded \"aw dash --filter\"
bind-key N run-shell \"aw dash next-ready\"
bind-key C-p run-shell \"aw dash park\"
bind-key o run-shell \"aw dash sidebar\"
";
const LABEL: &str = "tmux bindings";

/// One of the two paths tmux probes for user config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidatePath {
    pub path: PathBuf,
    pub kind: CandidateKind,
    pub exists: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CandidateKind {
    /// `$XDG_CONFIG_HOME/tmux/tmux.conf` (or `~/.config/tmux/tmux.conf`).
    Xdg,
    /// `~/.tmux.conf`.
    Legacy,
}

pub fn install(override_path: Option<&Path>) -> Result<()> {
    let candidates = tmux_candidate_configs()?;

    let target = match override_path {
        Some(p) => p.to_path_buf(),
        None => pick_target(&candidates),
    };

    // Show the user how we made the call. This is the kind of decision
    // that's surprising when it goes wrong, so we log it loudly.
    println!("Detected tmux config candidates (in tmux's load order):");
    for c in &candidates {
        let mark = if c.exists { "✓" } else { "·" };
        let kind = match c.kind {
            CandidateKind::Legacy => "legacy",
            CandidateKind::Xdg => "xdg   ",
        };
        let target_marker = if c.path == target { " ← installing here" } else { "" };
        println!("  {} {} {}{}", mark, kind, c.path.display(), target_marker);
    }
    if override_path.is_some() {
        println!("  ↳ overridden via --config");
    }
    println!();

    marker::apply(&target, LABEL, TMUX_BLOCK)?;
    println!("✅ Tmux bindings written to {}", target.display());

    // Strip our block from any *other* candidate that exists, so we don't
    // leave a stale duplicate that tmux still loads.
    for c in &candidates {
        if c.path == target || !c.exists {
            continue;
        }
        if marker::remove(&c.path, LABEL).unwrap_or(false) {
            println!("ℹ️  Removed stale block from {}", c.path.display());
        }
    }

    println!("   Reload: tmux source-file {}", target.display());
    Ok(())
}

/// The exact set of config paths tmux would probe, in tmux's load order.
pub fn tmux_candidate_configs() -> Result<Vec<CandidatePath>> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    let xdg_root = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home.join(".config"),
    };
    let legacy = home.join(".tmux.conf");
    let xdg = xdg_root.join("tmux").join("tmux.conf");
    Ok(vec![
        CandidatePath { exists: legacy.is_file(), path: legacy, kind: CandidateKind::Legacy },
        CandidatePath { exists: xdg.is_file(),    path: xdg,    kind: CandidateKind::Xdg    },
    ])
}

/// Pick the candidate whose bindings tmux would actually honor.
///
/// tmux loads in `[legacy, xdg]` order, so on a conflict the XDG bindings
/// win. To install something the user actually experiences:
///
///   - if XDG exists: write to XDG (it overrides legacy at runtime)
///   - else if legacy exists: write to legacy
///   - else: write to XDG (creates the modern default)
fn pick_target(candidates: &[CandidatePath]) -> PathBuf {
    let xdg = candidates.iter().find(|c| c.kind == CandidateKind::Xdg);
    let legacy = candidates.iter().find(|c| c.kind == CandidateKind::Legacy);

    if let Some(c) = xdg.filter(|c| c.exists) {
        return c.path.clone();
    }
    if let Some(c) = legacy.filter(|c| c.exists) {
        return c.path.clone();
    }
    xdg.map(|c| c.path.clone()).expect("xdg candidate always present")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cands(legacy_exists: bool, xdg_exists: bool) -> (tempfile::TempDir, Vec<CandidatePath>) {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join(".tmux.conf");
        let xdg = tmp.path().join("xdg/tmux/tmux.conf");
        if legacy_exists {
            std::fs::write(&legacy, "# legacy\n").unwrap();
        }
        if xdg_exists {
            std::fs::create_dir_all(xdg.parent().unwrap()).unwrap();
            std::fs::write(&xdg, "# xdg\n").unwrap();
        }
        let v = vec![
            CandidatePath { exists: legacy_exists, path: legacy, kind: CandidateKind::Legacy },
            CandidatePath { exists: xdg_exists,    path: xdg,    kind: CandidateKind::Xdg    },
        ];
        (tmp, v)
    }

    #[test]
    fn xdg_wins_when_both_exist_because_tmux_loads_it_second() {
        let (_t, c) = cands(true, true);
        assert_eq!(pick_target(&c), c[1].path);
    }

    #[test]
    fn legacy_picked_when_xdg_absent() {
        let (_t, c) = cands(true, false);
        assert_eq!(pick_target(&c), c[0].path);
    }

    #[test]
    fn xdg_created_when_neither_exists() {
        let (_t, c) = cands(false, false);
        assert_eq!(pick_target(&c), c[1].path);
    }
}
