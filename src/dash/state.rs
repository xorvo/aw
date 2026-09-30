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
    /// Agent-reported conversation id from the hook payload (`session_id`
    /// in Claude-style hook JSON). Empty when never reported. Feeds the
    /// resurrect manifest so a restored pane resumes the exact conversation.
    #[serde(default)]
    pub session_id: String,
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
    /// Pid of the tmux server this pane's state was written under.
    ///
    /// The ownership token that makes deletion safe. A process may only judge
    /// state written by the server it is itself talking to; anything else
    /// belongs to a different server and is none of its business. The manifest
    /// has always worked this way (see `manifest::prune_with_live_server`) —
    /// pane state did not, and a test harness that sandboxed tmux but not
    /// `AW_STATE_DIR` therefore deleted a developer's live panes' state.
    ///
    /// `None` on files written before this existed; such a file is never
    /// deleted, and the next hook stamps it.
    #[serde(default)]
    pub server_pid: Option<u32>,
    /// tmux window this pane belongs to. Panes sharing a window are one group.
    /// Not persisted; refreshed from tmux on every load.
    #[serde(skip)]
    pub window_id: String,
    /// Pane id of this window's lead — the agent that spawned the rest. Equal
    /// to `pane_id` when this *is* the lead. Not persisted.
    #[serde(skip)]
    pub lead_pane: String,
    /// How many panes this lead spawned — the rest of its tmux window. Zero on
    /// a spawned pane itself, so `spawned > 0` means "this row stands for a
    /// group". Derived from the window grouping in `assign_group_leads`, which
    /// is the only place it is computed; not persisted.
    #[serde(skip)]
    pub spawned: usize,
    /// Whether we actually know which agent runs here, as opposed to having
    /// guessed from the tmux label.
    ///
    /// `agent` is filled from the label for a pane no hook has fired in, so it
    /// is never empty and can't be used to tell an agent from a plain shell —
    /// a shell would claim to be an agent named "zsh". This flag is true only
    /// when a hook told us, or when the pane carries our `@aw_agent` stamp.
    /// Not persisted; recomputed on every load.
    #[serde(skip)]
    pub agent_known: bool,
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
            session_id: String::new(),
            parked: false,
            label: String::new(),
            pinned: false,
            agent_known: !agent.is_empty(),
            window_id: String::new(),
            server_pid: None,
            lead_pane: String::new(),
            spawned: 0,
        }
    }

    /// Is this pane its window's lead, rather than one it spawned?
    pub fn is_lead(&self) -> bool {
        self.lead_pane.is_empty() || self.lead_pane == self.pane_id
    }

    /// What this pane can honestly be said to be doing right now.
    pub fn shown(&self, now: u64) -> crate::dash::render::Shown {
        crate::dash::render::shown_status(self.status, self.last_activity, now)
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

/// Pane tallies for the header and the status line, by *derived* state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub working: usize,
    pub waiting: usize,
    pub stalled: usize,
    pub idle: usize,
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
        // Which tmux server wrote each file, kept separately because the state
        // itself is moved out of `hook_state` as rows are built.
        let mut hook_state_pids: HashMap<String, Option<u32>> = HashMap::new();
        if let Ok(read) = std::fs::read_dir(&panes_dir) {
            for d in read.flatten() {
                if d.path().extension().map_or(true, |e| e != "json") {
                    continue;
                }
                if let Ok(s) = PaneState::read(&d.path()) {
                    hook_paths.insert(s.pane_id.clone(), d.path());
                    hook_state_pids.insert(s.pane_id.clone(), s.server_pid);
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
                // Third source for "which agent is this?", after hook state and
                // the `@aw_agent` stamp: what the manifest recorded for this
                // pane under this same server. Covers panes that predate the
                // stamping, which would otherwise be indistinguishable from
                // plain shells and vanish from the switcher.
                //
                // Computed only when some pane actually needs it. It costs a
                // file read plus a tmux round-trip, and this runs on every dash
                // tick — a fully stamped or fully hooked server should pay
                // nothing. (Skipping it also keeps first paint quick enough for
                // the sidebar test's window, which is how the cost surfaced.)
                let live_server_pid = crate::dash::tmux::server_pid();
                let needs_hints = panes.iter().any(|tp| {
                    tp.session.starts_with("aw-")
                        && tp.aw_agent.is_empty()
                        && !hook_state.contains_key(&tp.pane_id)
                });
                let hints = if needs_hints {
                    crate::manifest::agent_hints(
                        &crate::manifest::SessionManifest::load(),
                        crate::dash::tmux::server_pid(),
                    )
                } else {
                    std::collections::BTreeMap::new()
                };

                // (3) For every live pane in an aw-* session, build a row,
                //     overlaying hook state when present. tmux fields
                //     always win over the file's stored values.
                for tp in &panes {
                    // Our own sidebar pane is not an agent. It lives in an
                    // `aw-*` session and runs `aw`, so without this it gets
                    // synthesized into a row named after the window and
                    // counted as idle — in the sidebar's own display, the
                    // popup, and the status line.
                    if tp.aw_sidebar {
                        continue;
                    }
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
                            s.agent_known = !s.agent.is_empty();
                            s.window_id = tp.window_id.clone();
                            s
                        }
                        None => PaneState {
                            schema_version: 1,
                            pane_id: tp.pane_id.clone(),
                            session: tp.session.clone(),
                            workspace,
                            cwd: tp.path.clone(),
                            // No hook has fired here, so prefer what we
                            // stamped on the pane (`@aw_agent`) over the tmux
                            // label. The label is a last resort and a poor
                            // one: Claude's native binary reports its version
                            // string as the foreground command, so an
                            // un-hooked claude pane would be named "2.1.278".
                            agent: agent_for(tp, &hints).unwrap_or_else(|| label.clone()),
                            status: Status::Idle,
                            last_event: String::new(),
                            last_activity: 0,
                            last_prompt: String::new(),
                            session_id: if tp.aw_session_id.is_empty() {
                                hints.get(&tp.pane_id).map(|(_, s)| s.clone()).unwrap_or_default()
                            } else {
                                tp.aw_session_id.clone()
                            },
                            parked: parked_now,
                            label,
                            pinned: pinned_now,
                            agent_known: agent_for(tp, &hints).is_some(),
                            window_id: tp.window_id.clone(),
                            lead_pane: String::new(),
                            spawned: 0,
                            // Built from tmux, not from a file, so it is by
                            // definition owned by the server we are talking to.
                            server_pid: live_server_pid,
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
                        // Absent from the listing is a *suspicion*, not a
                        // verdict. The old guard only covered an empty list, so
                        // one short or partly unreadable listing permanently
                        // deleted state for panes that were alive and busy —
                        // and the agent then looked idle forever, because
                        // nothing rebuilds a deleted file until the next hook.
                        // Deletion is irreversible, so make tmux say it twice.
                        let recorded = hook_state_pids.get(pane_id).copied().flatten();
                        if should_drop(
                            pane_id,
                            recorded,
                            live_server_pid,
                            &live_ids,
                            crate::dash::tmux::pane_is_gone,
                            pid_is_alive,
                            false, // automatic path: never guess about unstamped files
                        ) {
                            let _ = std::fs::remove_file(path);
                            if let Some(ref pdir) = parked_dir {
                                let _ = std::fs::remove_file(pdir.join(pane_id));
                            }
                        }
                    }
                    // Keep the resurrect manifest honest too: sessions and
                    // panes this same server proves dead were closed on
                    // purpose. Records from a previous server pid are
                    // crash survivors and are left for `aw resurrect`.
                    let live_sessions: std::collections::HashSet<String> =
                        panes.iter().map(|p| p.session.clone()).collect();
                    crate::manifest::prune_with_live_server(
                        &live_sessions,
                        &live_ids,
                        crate::dash::tmux::server_pid(),
                    );
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
                    s.agent_known = !s.agent.is_empty();
                    if let Some(ref pdir) = parked_dir {
                        s.parked = pdir.join(&s.pane_id).exists();
                    }
                    s.pinned = pinned_workspaces.contains(&s.workspace);
                    entries.push(s);
                }
            }
        }

        assign_group_leads(&mut entries);

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
    /// Counts restricted to panes we can actually identify as agents.
    ///
    /// [`Self::counts`] includes every pane in an `aw-*` session, which means a
    /// plain shell you happen to have open is tallied as an idle agent (see
    /// [`PaneState::agent_known`]). The sidebar is pinned all day, so that noise
    /// is the difference between a useful readout and one you learn to ignore.
    pub fn agent_counts(&self) -> Counts {
        self.tally(|e| e.agent_known && !e.parked)
    }

    pub fn counts(&self) -> Counts {
        self.tally(|e| !e.parked)
    }

    /// Tally the panes `keep` accepts, by *derived* state.
    ///
    /// One body for both tallies so they can never disagree about what counts as
    /// working. Derived rather than recorded: a `working` latch that was never
    /// renewed is `stalled`, so a session that errored out days ago stops being
    /// reported as busy.
    fn tally(&self, keep: impl Fn(&PaneState) -> bool) -> Counts {
        use crate::dash::render::Shown;
        let now = now_epoch();
        let mut c = Counts::default();
        for e in self.entries.iter().filter(|e| keep(e)) {
            match e.shown(now) {
                Shown::Working => c.working += 1,
                Shown::Waiting => c.waiting += 1,
                Shown::Stalled => c.stalled += 1,
                Shown::Idle => c.idle += 1,
            }
        }
        c
    }
}

/// Append one line to `<state>/gc.log`.
///
/// Deleting a pane's state is the only irreversible thing the dashboard does,
/// and it is invisible: the symptom shows up later as an agent that looks like
/// it has never run. Both outcomes are recorded — the delete, and the more
/// interesting case where the bulk listing said a pane was gone but tmux
/// disagreed, which is the anomaly that sent us hunting in the first place.
///
/// Best effort and self-capping, because logging must never be the reason a
/// snapshot load fails.
fn audit(line: &str) {
    let Ok(dir) = crate::dash::state_root() else { return };
    let path = dir.join("gc.log");
    // Keep it small; this is a breadcrumb trail, not a journal.
    if std::fs::metadata(&path).map(|m| m.len() > 64 * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(
            f,
            "{} pid={} {}",
            crate::dash::state::now_epoch(),
            std::process::id(),
            line
        );
    }
}

/// Mark each pane with its window's lead.
///
/// An agent that spawns helpers splits them into its own tmux window, so a
/// window holding several panes is one group and the window *is* the hierarchy
/// — no agent has to report its children, and nothing couples to a particular
/// agent's internals.
///
/// The lead is the numerically lowest pane id in the window. tmux hands out
/// pane ids monotonically and never reuses them within a server, so the lowest
/// is the pane the window started with and every other pane was split off it
/// later. Pane *index* would be wrong here: it tracks on-screen position, which
/// moves when panes are rearranged.
pub fn assign_group_leads(entries: &mut [PaneState]) {
    use std::collections::HashMap;
    let mut lead: HashMap<String, (u64, String)> = HashMap::new();
    for e in entries.iter() {
        if e.window_id.is_empty() {
            continue;
        }
        let n = pane_ordinal(&e.pane_id);
        lead.entry(e.window_id.clone())
            .and_modify(|best| {
                if n < best.0 {
                    *best = (n, e.pane_id.clone());
                }
            })
            .or_insert((n, e.pane_id.clone()));
    }
    for e in entries.iter_mut() {
        e.lead_pane = match lead.get(&e.window_id) {
            Some((_, id)) => id.clone(),
            // No window info (tmux unreachable): treat the pane as its own lead
            // so nothing is ever hidden for lack of grouping data.
            None => e.pane_id.clone(),
        };
    }
    // Second pass, because a lead's count depends on every other pane's
    // `lead_pane` having been resolved first.
    let mut per_lead: HashMap<String, usize> = HashMap::new();
    for e in entries.iter() {
        if !e.is_lead() {
            *per_lead.entry(e.lead_pane.clone()).or_insert(0) += 1;
        }
    }
    for e in entries.iter_mut() {
        e.spawned = if e.is_lead() {
            per_lead.get(&e.pane_id).copied().unwrap_or(0)
        } else {
            0
        };
    }
}

/// Numeric part of a tmux pane id (`%44` -> 44) for creation-order comparison.
/// An unparseable id sorts last so it can never masquerade as a lead.
fn pane_ordinal(pane_id: &str) -> u64 {
    pane_id.trim_start_matches('%').parse().unwrap_or(u64::MAX)
}

/// Should this pane's state file be deleted?
///
/// Only when it is missing from the bulk listing *and* a second, per-pane
/// question confirms it. Pure in the confirmation so the rule is testable.
pub(crate) fn should_drop(
    pane_id: &str,
    recorded_pid: Option<u32>,
    live_pid: Option<u32>,
    live_ids: &std::collections::HashSet<String>,
    confirm_gone: impl Fn(&str) -> bool,
    pid_alive: impl Fn(u32) -> bool,
    collect_unstamped: bool,
) -> bool {
    if live_ids.contains(pane_id) {
        return false;
    }
    match recorded_pid {
        // Written by the server we are talking to: ours to judge. Still ask
        // tmux about this pane specifically, because absence from one bulk
        // listing is a suspicion and deletion is irreversible.
        Some(p) if Some(p) == live_pid => {
            if confirm_gone(pane_id) {
                audit(&format!("drop {} (own server {}, tmux agrees)", pane_id, p));
                return true;
            }
            audit(&format!(
                "KEEP {} — absent from a {}-pane listing but tmux says it is alive",
                pane_id,
                live_ids.len()
            ));
            false
        }
        // Another server wrote it and that server is still running. Not ours to
        // judge, at any cost: this is the case where a test harness, or any
        // process pointed at a different tmux, would otherwise wipe a live
        // session's state.
        Some(p) if pid_alive(p) => {
            audit(&format!(
                "KEEP {} — owned by live tmux server {} (we are on {:?})",
                pane_id, p, live_pid
            ));
            false
        }
        // The server that wrote it is gone, so nothing can be waiting on it.
        Some(p) => {
            audit(&format!("drop {} (server {} is gone)", pane_id, p));
            true
        }
        // Pre-dates the ownership stamp, so we cannot tell whose it is. The
        // automatic path keeps it: a stale file costs a few bytes, a wrong
        // delete costs an agent's visible history, and the next hook stamps it.
        // `aw dash gc` is a person asking for a clean-up, so there it is fair
        // game once tmux confirms the pane is gone — otherwise legacy files for
        // panes that will never fire another hook would linger forever.
        None if collect_unstamped && confirm_gone(pane_id) => {
            audit(&format!("drop {} (unstamped, explicit gc, tmux agrees)", pane_id));
            true
        }
        None => {
            audit(&format!("KEEP {} — no owning server recorded", pane_id));
            false
        }
    }
}

/// Is a pid still a running process? Used only to decide whether another tmux
/// server still owns some state, so a spawn per candidate is fine.
///
/// `ps -p`, not `kill -0`: `kill` also fails with EPERM for a live process owned
/// by someone else, which would report it dead and hand us permission to delete
/// its state. `ps` answers the question actually being asked — does this pid
/// exist — whoever owns it.
pub(crate) fn pid_is_alive(pid: u32) -> bool {
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(true) // can't tell -> assume alive, i.e. don't delete
}

/// The agent running in a pane we have no hook state for, if we can know it.
///
/// `@aw_agent` first — we wrote it, so it is current. Then the manifest, which
/// covers panes stamped by an older `aw`. `None` means genuinely unknown, and
/// callers must not guess from the tmux label: a shell would claim to be an
/// agent named "zsh", and an un-hooked Claude pane would be named after its
/// version string.
fn agent_for(
    tp: &crate::dash::tmux::PaneInfo,
    hints: &std::collections::BTreeMap<String, (String, String)>,
) -> Option<String> {
    if !tp.aw_agent.is_empty() {
        return Some(tp.aw_agent.clone());
    }
    hints.get(&tp.pane_id).map(|(a, _)| a.clone())
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

    fn win(pane: &str, window: &str) -> PaneState {
        let mut p = PaneState::new(pane, "claude");
        p.window_id = window.to_string();
        p
    }

    #[test]
    fn the_lead_is_the_oldest_pane_in_the_window() {
        // One window holding an agent (%35) and five panes it split off later.
        let mut e = vec![
            win("%44", "@27"), win("%35", "@27"), win("%48", "@27"),
            win("%46", "@27"), win("%47", "@27"), win("%45", "@27"),
            win("%16", "@16"),   // a window of its own
        ];
        assign_group_leads(&mut e);
        for p in &e {
            let want = if p.window_id == "@27" { "%35" } else { "%16" };
            assert_eq!(p.lead_pane, want, "{} got the wrong lead", p.pane_id);
        }
        assert!(e.iter().find(|p| p.pane_id == "%35").unwrap().is_lead());
        assert!(!e.iter().find(|p| p.pane_id == "%44").unwrap().is_lead());
        assert!(e.iter().find(|p| p.pane_id == "%16").unwrap().is_lead());
    }

    #[test]
    fn pane_ids_compare_numerically_not_as_text() {
        // "%9" sorts after "%44" as a string, which would pick the wrong lead.
        let mut e = vec![win("%44", "@1"), win("%9", "@1")];
        assign_group_leads(&mut e);
        assert!(e.iter().all(|p| p.lead_pane == "%9"));
    }

    #[test]
    fn a_pane_with_no_window_info_leads_itself() {
        // tmux unreachable: never hide a pane for lack of grouping data.
        let mut e = vec![PaneState::new("%1", "claude")];
        assign_group_leads(&mut e);
        assert!(e[0].is_lead());
    }

    /// The invariant that makes the destructive path safe: a process may only
    /// collect state written by the tmux server it is itself talking to.
    ///
    /// The scenario these guard is real. A test harness sandboxed tmux but not
    /// `AW_STATE_DIR`, so `aw` ran against a 2-pane private server while
    /// pointed at a developer's real cache, and deleted the state for every one
    /// of their live panes. Env hygiene fixed that instance; this rule makes the
    /// whole class impossible, however a future caller is misconfigured.
    #[test]
    fn state_owned_by_another_live_server_is_never_collected() {
        let live: HashSet<String> = ["%1".to_string()].into();
        // We are talking to server 222. The file was written by 111, which is
        // still running — a real session's state. Never ours to delete, even
        // though our tmux happily confirms the pane is not on *our* server.
        assert!(!should_drop("%44", Some(111), Some(222), &live, |_| true, |_| true, false));
    }

    #[test]
    fn state_from_a_dead_server_is_collected() {
        let live: HashSet<String> = HashSet::new();
        // Written by 111, which no longer exists: nothing can be waiting on it.
        assert!(should_drop("%44", Some(111), Some(222), &live, |_| false, |_| false, false));
    }

    #[test]
    fn own_server_still_needs_tmux_to_confirm() {
        let live: HashSet<String> = ["%1".to_string()].into();
        // Same server, pane absent and confirmed gone: collect.
        assert!(should_drop("%2", Some(222), Some(222), &live, |_| true, |_| true, false));
        // Same server, but tmux says the pane is alive: a short listing. Keep.
        assert!(!should_drop("%2", Some(222), Some(222), &live, |_| false, |_| true, false));
        // Present in the listing: never touched, whatever else is true.
        assert!(!should_drop("%1", Some(222), Some(222), &live, |_| true, |_| true, false));
    }

    #[test]
    fn unstamped_files_are_kept_rather_than_guessed_about() {
        let live: HashSet<String> = HashSet::new();
        // Written before ownership was recorded. A stale file costs bytes; a
        // wrong delete costs an agent's visible history. The next hook stamps it.
        assert!(!should_drop("%2", None, Some(222), &live, |_| true, |_| true, false));
        // But `aw dash gc`, which a person ran on purpose, may clean it.
        assert!(should_drop("%2", None, Some(222), &live, |_| true, |_| true, true));
    }

    #[test]
    fn an_unknowable_pid_is_treated_as_alive() {
        let live: HashSet<String> = HashSet::new();
        // `pid_alive` could not tell. Refuse to delete on a maybe.
        assert!(!should_drop("%2", Some(111), Some(222), &live, |_| true, |_| true, false));
    }

    #[test]
    fn with_no_live_server_nothing_owned_by_a_live_one_is_touched() {
        let live: HashSet<String> = HashSet::new();
        // We could not determine our own server pid. A file owned by a running
        // server stays; one owned by a dead server goes.
        assert!(!should_drop("%2", Some(111), None, &live, |_| true, |_| true, false));
        assert!(should_drop("%3", Some(111), None, &live, |_| true, |_| false, false));
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
