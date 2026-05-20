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
    /// Display label resolved fresh from tmux (`window_name` →
    /// `pane_title` → `pane_current_command`) on every snapshot load.
    /// Surfaces a user-renamed Claude session (`/rename …` writes to both
    /// window_name and pane_title) in the row and the `/` filter haystack.
    ///
    /// Not persisted — the on-disk value would be stale by next load.
    #[serde(skip)]
    pub label: String,
    /// True iff `pinned/<workspace>` sentinel exists. Workspace-level pin
    /// (every pane in the same workspace shares the same value). Not
    /// persisted on the pane; we read the sentinel directory on load.
    #[serde(skip)]
    pub pinned: bool,
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
            label: String::new(),
            pinned: false,
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

/// A workspace that exists on disk but has no live `aw-<name>` tmux session.
/// Surfaced in the dashboard so users can pick a known workspace and open
/// it without dropping back to the shell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DormantWorkspace {
    pub name: String,
    pub base: String,
    /// Free-form (matches `WorkspaceMeta::created`); may be `"unknown"`.
    pub created: String,
    /// Filled at load time from the `pinned/<name>` sentinel; not persisted.
    #[serde(skip)]
    pub pinned: bool,
    /// mtime of the workspace dir (Unix epoch *milliseconds*), used to
    /// sort unpinned dormant workspaces by recency. Filled at load time;
    /// not persisted. Millisecond precision lets us distinguish
    /// workspaces created in rapid succession (sandbox tests, scripts).
    #[serde(skip)]
    pub mtime: u128,
}

#[derive(Debug)]
pub struct Snapshot {
    pub entries: Vec<PaneState>,
    /// Workspaces present on disk with no `aw-<name>` tmux session live.
    /// Empty when tmux is unreachable (we can't classify reliably without
    /// an authoritative session list).
    pub dormant: Vec<DormantWorkspace>,
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
        // Pin sentinels live at `pinned/<workspace>`. Load the set once
        // so every row read costs O(1).
        let pinned_workspaces: std::collections::HashSet<String> =
            match crate::dash::pinned_dir().ok().and_then(|d| std::fs::read_dir(&d).ok()) {
                Some(read) => read
                    .filter_map(|d| d.ok())
                    .map(|d| d.file_name().to_string_lossy().into_owned())
                    .collect(),
                None => std::collections::HashSet::new(),
            };
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
        let mut dormant: Vec<DormantWorkspace> = Vec::new();
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
                    let pinned_now = pinned_workspaces.contains(&workspace);
                    // `label` is always refreshed from tmux so a
                    // `/rename`'d Claude session (which writes to
                    // window_name + pane_title) shows up in the row and
                    // is searchable via `/`, even after the hook has
                    // stamped `agent = "claude"` over the JSON.
                    let label = crate::dash::tmux::label_from_tmux(tp);
                    let row = match hook_state.remove(&tp.pane_id) {
                        Some(mut s) => {
                            // Refresh ground-truth fields from tmux; keep
                            // hook-derived ones (status, last_event,
                            // last_activity, last_prompt, agent) intact.
                            s.session = tp.session.clone();
                            s.workspace = workspace;
                            s.cwd = tp.path.clone();
                            s.parked = parked_now;
                            s.label = label;
                            s.pinned = pinned_now;
                            s
                        }
                        None => PaneState {
                            schema_version: 1,
                            pane_id: tp.pane_id.clone(),
                            session: tp.session.clone(),
                            workspace,
                            cwd: tp.path.clone(),
                            // No hook fired in this pane yet, so we have
                            // no agent-type signal — fall back to the
                            // tmux label as the agent column too.
                            agent: label.clone(),
                            status: Status::Idle,
                            last_event: String::new(),
                            last_activity: 0,
                            last_prompt: String::new(),
                            parked: parked_now,
                            label,
                            pinned: pinned_now,
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

                // (5) Dormant workspaces: on-disk workspaces with no live
                //     `aw-<name>` session. Active set is derived from tmux
                //     session names so a session without any state file
                //     still counts as live.
                let active_workspaces: std::collections::HashSet<String> = panes
                    .iter()
                    .filter_map(|p| p.session.strip_prefix("aw-").map(String::from))
                    .collect();
                dormant = compute_dormant(&active_workspaces, &pinned_workspaces);
            }
            crate::dash::tmux::PaneListing::Unavailable => {
                // Fall back to file-only. Don't auto-gc — without tmux's
                // word we can't tell live from dead.
                for (_, mut s) in hook_state.drain() {
                    if let Some(ref pdir) = parked_dir {
                        s.parked = pdir.join(&s.pane_id).exists();
                    }
                    s.pinned = pinned_workspaces.contains(&s.workspace);
                    entries.push(s);
                }
            }
        }

        // Sort active entries by workspace, with workspace order driven by
        // (pinned first, then max-activity desc among workspace's panes,
        // then name alpha). Inside a workspace, keep stable pane_id order.
        let max_activity: std::collections::HashMap<String, u64> = {
            let mut m: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
            for e in &entries {
                let cur = m.entry(e.workspace.clone()).or_insert(0);
                if e.last_activity > *cur {
                    *cur = e.last_activity;
                }
            }
            m
        };
        entries.sort_by(|a, b| {
            // pinned first
            b.pinned.cmp(&a.pinned)
                // then recency desc
                .then_with(|| {
                    let ma = max_activity.get(&a.workspace).copied().unwrap_or(0);
                    let mb = max_activity.get(&b.workspace).copied().unwrap_or(0);
                    mb.cmp(&ma)
                })
                // then workspace name alpha (stable when activity is tied)
                .then_with(|| a.workspace.cmp(&b.workspace))
                // then pane id within the workspace
                .then_with(|| a.pane_id.cmp(&b.pane_id))
        });
        Ok(Self { entries, dormant })
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

/// Compute the dormant-workspace list: every on-disk workspace whose name
/// is not in the `active` set. Sorted by (pinned desc, dir mtime desc,
/// name asc) so pinned workspaces float to the top and recently-touched
/// dormant ones come next.
///
/// Extracted from `Snapshot::load` for direct unit testing — the loader
/// otherwise needs a live tmux server to exercise this branch.
pub fn compute_dormant(
    active: &std::collections::HashSet<String>,
    pinned: &std::collections::HashSet<String>,
) -> Vec<DormantWorkspace> {
    let paths = crate::paths::Paths::from_env().ok();
    let mut out: Vec<DormantWorkspace> = crate::workspace::listing::enumerate_workspaces()
        .into_iter()
        .filter(|m| !active.contains(&m.name))
        .map(|m| {
            let mtime = paths
                .as_ref()
                .and_then(|p| std::fs::metadata(p.workspace_dir(&m.name)).ok())
                .and_then(|md| md.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis())
                .unwrap_or(0);
            DormantWorkspace {
                pinned: pinned.contains(&m.name),
                name: m.name,
                base: m.base,
                created: m.created,
                mtime,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| b.mtime.cmp(&a.mtime))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::collections::HashSet;
    use tempfile::TempDir;

    fn seed(root: &std::path::Path, name: &str, base: &str, created: &str) {
        let dir = root.join(name).join(".agent-workspace");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("name"), format!("{}\n", name)).unwrap();
        std::fs::write(dir.join("base"), format!("{}\n", base)).unwrap();
        std::fs::write(dir.join("created"), format!("{}\n", created)).unwrap();
        // Small sleep so successive calls produce distinguishable mtimes on
        // the workspace dir — recency-sort tests rely on this.
        std::thread::sleep(std::time::Duration::from_millis(15));
    }

    #[test]
    #[serial]
    fn compute_dormant_excludes_active_workspaces() {
        let tmp = TempDir::new().unwrap();
        seed(tmp.path(), "alpha", "default", "2026-03-01T10:00:00Z");
        seed(tmp.path(), "beta", "python", "2026-03-02T10:00:00Z");
        seed(tmp.path(), "gamma", "default", "2026-03-03T10:00:00Z");
        std::env::set_var("AW_WORKSPACES_DIR", tmp.path());

        let mut active: HashSet<String> = HashSet::new();
        active.insert("beta".into());

        let out = compute_dormant(&active, &std::collections::HashSet::new());
        std::env::remove_var("AW_WORKSPACES_DIR");

        // gamma was seeded last (highest dir mtime) so it floats to the
        // top under the recency sort. alpha follows.
        let names: Vec<&str> = out.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["gamma", "alpha"]);
        assert_eq!(out[0].created, "2026-03-03T10:00:00Z");
        assert_eq!(out[1].base, "default");
    }

    #[test]
    #[serial]
    fn compute_dormant_returns_all_when_active_empty() {
        let tmp = TempDir::new().unwrap();
        seed(tmp.path(), "one", "default", "2026-03-01T10:00:00Z");
        seed(tmp.path(), "two", "default", "2026-03-02T10:00:00Z");
        std::env::set_var("AW_WORKSPACES_DIR", tmp.path());

        let active: HashSet<String> = HashSet::new();
        let out = compute_dormant(&active, &std::collections::HashSet::new());
        std::env::remove_var("AW_WORKSPACES_DIR");

        // two was seeded after one, so it floats to the top under the
        // recency sort.
        let names: Vec<&str> = out.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["two", "one"]);
    }

    #[test]
    #[serial]
    fn compute_dormant_returns_empty_when_all_active() {
        let tmp = TempDir::new().unwrap();
        seed(tmp.path(), "only", "default", "2026-03-01T10:00:00Z");
        std::env::set_var("AW_WORKSPACES_DIR", tmp.path());

        let mut active: HashSet<String> = HashSet::new();
        active.insert("only".into());

        let out = compute_dormant(&active, &std::collections::HashSet::new());
        std::env::remove_var("AW_WORKSPACES_DIR");
        assert!(out.is_empty());
    }

    #[test]
    #[serial]
    fn compute_dormant_pinned_floats_to_top() {
        let tmp = TempDir::new().unwrap();
        seed(tmp.path(), "alpha", "default", "2026-03-01T10:00:00Z");
        seed(tmp.path(), "beta", "default", "2026-03-02T10:00:00Z");
        seed(tmp.path(), "zeta", "default", "2026-03-03T10:00:00Z");
        std::env::set_var("AW_WORKSPACES_DIR", tmp.path());

        let active: HashSet<String> = HashSet::new();
        let mut pinned: HashSet<String> = HashSet::new();
        pinned.insert("zeta".into()); // last alphabetically

        let out = compute_dormant(&active, &pinned);
        std::env::remove_var("AW_WORKSPACES_DIR");

        let names: Vec<&str> = out.iter().map(|d| d.name.as_str()).collect();
        // zeta is pinned → first, regardless of name or mtime.
        assert_eq!(names[0], "zeta", "pinned workspace must come first");
        assert!(out[0].pinned);
        // Unpinned entries follow in mtime-desc order. seed() creates them
        // sequentially, so beta (seeded after alpha) comes first.
        assert_eq!(names[1..], vec!["beta", "alpha"]);
    }
}
