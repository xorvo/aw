//! State files: `~/.cache/aw/panes/<pane_id>.json`, one per active pane.
//!
//! All writes are atomic (tempfile + rename on the same filesystem).
//! Reads tolerate missing or malformed files (skip with a warning).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::dash::{panes_dir, parked_dir};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Working,
    Waiting,
    Idle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneState {
    pub schema_version: u32,
    pub pane_id: String,
    pub session: String,
    pub workspace: String,
    pub cwd: String,
    pub agent: String,
    pub status: Status,
    pub last_event: String,
    /// Unix epoch seconds.
    pub last_activity: u64,
    pub last_prompt: String,
    /// Filled in at load time from `parked/<pane>` sentinel; not persisted
    /// in the per-pane JSON.
    #[serde(skip)]
    pub parked: bool,
}

impl PaneState {
    pub fn new(pane_id: &str, agent: &str) -> Self {
        Self {
            schema_version: 1,
            pane_id: pane_id.to_string(),
            session: String::new(),
            workspace: String::new(),
            cwd: String::new(),
            agent: agent.to_string(),
            status: Status::Idle,
            last_event: String::new(),
            last_activity: now_epoch(),
            last_prompt: String::new(),
            parked: false,
        }
    }

    pub fn write_atomic(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir {}", parent.display()))?;
        }
        let tmp = path.with_extension("json.tmp");
        let raw = serde_json::to_string_pretty(self)? + "\n";
        std::fs::write(&tmp, raw)
            .with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))
    }

    pub fn read(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?;
        let s: Self = serde_json::from_str(&raw)
            .with_context(|| format!("parse {}", path.display()))?;
        Ok(s)
    }
}

#[derive(Debug)]
pub struct Snapshot {
    pub entries: Vec<PaneState>,
}

impl Snapshot {
    /// Build a snapshot. Authority split:
    ///
    /// - **tmux** is the source of truth for *which panes exist*, *which
    ///   session they're in*, *cwd*, and *foreground command*. These
    ///   fields are refreshed from tmux on every load — never trusted
    ///   from a stale state file.
    /// - **State files** at `<state_dir>/panes/*.json` enrich live panes
    ///   with hook-derived data (status, last event, last prompt).
    ///
    /// State files for panes tmux doesn't know about are **discarded**
    /// (and deleted as a side effect — auto-gc — so the cache doesn't
    /// grow unboundedly). This is the fix for stale "dead pane" rows
    /// that previously persisted until `aw dash gc` ran manually.
    ///
    /// When tmux is *unreachable* (no server, command missing), we fall
    /// back to file-only mode so the dashboard isn't empty just because
    /// you killed the tmux server. State files won't be auto-deleted in
    /// this mode — there's no authority to decide they're dead.
    pub fn load() -> Result<Self> {
        let parked_dir = parked_dir().ok();
        let panes_dir = panes_dir()?;

        // (1) Read every state file into a pane-id-keyed map. We keep a
        //     parallel map of pane-id → on-disk path so auto-gc can
        //     unlink the file later if tmux says the pane is dead.
        let mut hook_state: HashMap<String, PaneState> = HashMap::new();
        let mut hook_paths: HashMap<String, std::path::PathBuf> = HashMap::new();
        if let Ok(read) = std::fs::read_dir(&panes_dir) {
            for d in read.flatten() {
                if d.path().extension().map_or(true, |e| e != "json") {
                    continue;
                }
                if let Ok(s) = PaneState::read(&d.path()) {
                    hook_paths.insert(s.pane_id.clone(), d.path());
                    hook_state.insert(s.pane_id.clone(), s);
                }
            }
        }

        // (2) Ask tmux. Authoritative when reachable.
        let listing = crate::dash::tmux::list_panes_with_metadata();

        let mut entries = Vec::new();
        match listing {
            crate::dash::tmux::PaneListing::Tmux(panes) => {
                let live_ids: std::collections::HashSet<String> =
                    panes.iter().map(|p| p.pane_id.clone()).collect();

                // (3) For every live pane in an aw-* session, build a row,
                //     overlaying hook state when present. tmux fields
                //     always win over the file's stored values.
                for tp in &panes {
                    let workspace = match tp.session.strip_prefix("aw-") {
                        Some(w) => w.to_string(),
                        None => continue,
                    };
                    let parked_now = parked_dir
                        .as_ref()
                        .map(|d| d.join(&tp.pane_id).exists())
                        .unwrap_or(false);
                    let row = match hook_state.remove(&tp.pane_id) {
                        Some(mut s) => {
                            // Refresh ground-truth fields from tmux; keep
                            // hook-derived ones (status, last_event,
                            // last_activity, last_prompt, agent) intact.
                            s.session = tp.session.clone();
                            s.workspace = workspace;
                            s.cwd = tp.path.clone();
                            s.parked = parked_now;
                            s
                        }
                        None => PaneState {
                            schema_version: 1,
                            pane_id: tp.pane_id.clone(),
                            session: tp.session.clone(),
                            workspace,
                            cwd: tp.path.clone(),
                            // Stable label cascade: pane_title → window_name
                            // → pane_current_command. Matches the "good
                            // human friendly name" tmux's status bar shows,
                            // not the volatile foreground process.
                            agent: crate::dash::tmux::label_from_tmux(tp),
                            status: Status::Idle,
                            last_event: String::new(),
                            last_activity: 0,
                            last_prompt: String::new(),
                            parked: parked_now,
                        },
                    };
                    entries.push(row);
                }

                // (4) Auto-gc — but only when we got a non-empty live list.
                //     If tmux returned zero panes it's almost certainly a
                //     transient (server just restarted, or every aw-*
                //     session was just killed). Better to keep the hook
                //     files and rebuild the rows on the next tick than to
                //     wipe the cache during a blip.
                if !panes.is_empty() {
                    for (pane_id, path) in &hook_paths {
                        if !live_ids.contains(pane_id) {
                            let _ = std::fs::remove_file(path);
                            if let Some(ref pdir) = parked_dir {
                                let _ = std::fs::remove_file(pdir.join(pane_id));
                            }
                        }
                    }
                }
            }
            crate::dash::tmux::PaneListing::Unavailable => {
                // Fall back to file-only. Don't auto-gc — without tmux's
                // word we can't tell live from dead.
                for (_, mut s) in hook_state.drain() {
                    if let Some(ref pdir) = parked_dir {
                        s.parked = pdir.join(&s.pane_id).exists();
                    }
                    entries.push(s);
                }
            }
        }

        entries.sort_by(|a, b| {
            a.workspace
                .cmp(&b.workspace)
                .then_with(|| a.pane_id.cmp(&b.pane_id))
        });
        Ok(Self { entries })
    }

    /// Counts (working, waiting, idle). Parked panes are excluded — bash
    /// equivalent of "set aside, don't bug me about these."
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut w = 0;
        let mut wt = 0;
        let mut i = 0;
        for e in &self.entries {
            if e.parked {
                continue;
            }
            match e.status {
                Status::Working => w += 1,
                Status::Waiting => wt += 1,
                Status::Idle => i += 1,
            }
        }
        (w, wt, i)
    }
}

pub fn pane_state_path(pane_id: &str) -> Result<PathBuf> {
    let dir = panes_dir()?;
    Ok(dir.join(format!("{}.json", pane_id.replace('/', "_"))))
}

pub fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
