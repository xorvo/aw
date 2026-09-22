//! Durable session manifest: `<state_root>/sessions.json`.
//!
//! The tmux server holds the only copy of "which sessions exist"; when it
//! dies (an agent runs `kill-server`, a power cut) that knowledge dies with
//! it. This manifest is the on-disk shadow: every `aw-*` session plus the
//! agents seen inside it, keyed by session *name* — durable, derived from
//! the workspace — never by pane id, which tmux reassigns on every server
//! restart. `aw resurrect` replays it.
//!
//! Writers: `aw hook` (every agent event), `aw start` / the dash open path
//! (session creation), `aw delete` (removal), and the dash snapshot loader
//! (dead-pane pruning). Writes are read-modify-write with an atomic rename;
//! concurrent hooks can race and the last writer wins — acceptable, since
//! every field is a freshness signal the next event re-writes.
//!
//! ## Deliberate kill vs. crash
//!
//! Each session records the tmux **server pid** it was last seen under. A
//! session missing from a live server *with the same pid* was closed on
//! purpose (the server never died) → prune, don't resurrect. A missing
//! session whose recorded pid differs from the live server's (or where no
//! server is running) is a crash survivor → resurrect candidate.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneRecord {
    pub agent: String,
    pub cwd: String,
    /// Agent-reported conversation id (from the hook payload's
    /// `session_id`). Lets resurrect reopen the *exact* conversation
    /// (`claude --resume <id>`) instead of just the directory's latest.
    /// Empty when the agent never reported one.
    #[serde(default)]
    pub session_id: String,
    /// Unix epoch seconds.
    pub last_activity: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub workspace: String,
    /// Workspace directory the session was rooted in (`new-session -c`).
    pub cwd: String,
    /// Pid of the tmux server this session was last seen under.
    pub server_pid: Option<u32>,
    /// Unix epoch seconds.
    pub last_activity: u64,
    /// Keyed by pane id. Pane ids die with the server; after resurrection
    /// they are re-keyed to the newly created panes.
    #[serde(default)]
    pub panes: BTreeMap<String, PaneRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionManifest {
    #[serde(default = "schema_version_1")]
    pub schema_version: u32,
    /// Keyed by tmux session name (`aw-<workspace>`).
    #[serde(default)]
    pub sessions: BTreeMap<String, SessionRecord>,
}

fn schema_version_1() -> u32 {
    1
}

pub fn manifest_path() -> Result<PathBuf> {
    Ok(crate::dash::state_root()?.join("sessions.json"))
}

impl SessionManifest {
    /// Load the manifest; a missing or malformed file is an empty manifest
    /// (same tolerance as the pane state files).
    pub fn load() -> Self {
        manifest_path()
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = manifest_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir {}", parent.display()))?;
        }
        let tmp = path.with_extension("json.tmp");
        let mut me = self.clone();
        me.schema_version = 1;
        let raw = serde_json::to_string_pretty(&me)? + "\n";
        std::fs::write(&tmp, raw)
            .with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, &path)
            .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))
    }

    fn session_entry(&mut self, session: &str, workspace: &str, cwd: &str) -> &mut SessionRecord {
        self.sessions
            .entry(session.to_string())
            .or_insert_with(|| SessionRecord {
                workspace: workspace.to_string(),
                cwd: cwd.to_string(),
                server_pid: None,
                last_activity: 0,
                panes: BTreeMap::new(),
            })
    }
}

/// Record (or refresh) a session with no pane detail — called on session
/// creation, before any agent has fired a hook.
pub fn record_session(session: &str, workspace: &str, cwd: &str) {
    let now = crate::dash::state::now_epoch();
    let pid = crate::dash::tmux::server_pid();
    let mut m = SessionManifest::load();
    let rec = m.session_entry(session, workspace, cwd);
    rec.workspace = workspace.to_string();
    rec.cwd = cwd.to_string();
    rec.last_activity = now;
    if pid.is_some() {
        rec.server_pid = pid;
    }
    let _ = m.save();
}

/// Record an agent event in a pane. Only `aw-*` sessions are tracked —
/// agents running in personal tmux sessions are none of our business.
pub fn record_pane(
    session: &str,
    workspace: &str,
    cwd: &str,
    agent: &str,
    pane_id: &str,
    session_id: &str,
) {
    if !session.starts_with("aw-") {
        return;
    }
    let now = crate::dash::state::now_epoch();
    let pid = crate::dash::tmux::server_pid();
    let mut m = SessionManifest::load();
    let rec = m.session_entry(session, workspace, cwd);
    if !workspace.is_empty() {
        rec.workspace = workspace.to_string();
    }
    rec.last_activity = now;
    if pid.is_some() {
        rec.server_pid = pid;
    }
    rec.panes.insert(
        pane_id.to_string(),
        PaneRecord {
            agent: agent.to_string(),
            cwd: cwd.to_string(),
            session_id: session_id.to_string(),
            last_activity: now,
        },
    );
    let _ = m.save();
}

/// `pane_id -> (agent, session_id)` for every agent pane recorded under the
/// live server.
///
/// Pane ids are unique for one server lifetime, so same pid plus same id is the
/// same pane. Records from other pids describe dead panes whose ids the current
/// server may have handed out again, so they are ignored.
///
/// Used to recognise an agent pane that has no hook state and no `@aw_agent`
/// stamp — panes that predate the stamping, which would otherwise look like
/// plain shells.
pub fn agent_hints(
    manifest: &SessionManifest,
    live_pid: Option<u32>,
) -> BTreeMap<String, (String, String)> {
    let mut out = BTreeMap::new();
    let Some(pid) = live_pid else { return out };
    for rec in manifest.sessions.values().filter(|r| r.server_pid == Some(pid)) {
        for (id, p) in &rec.panes {
            if !p.agent.is_empty() {
                out.insert(id.clone(), (p.agent.clone(), p.session_id.clone()));
            }
        }
    }
    out
}

/// Drop a session entirely (workspace deleted, or pruned as deliberately
/// killed). No-op if absent.
pub fn remove_session(session: &str) {
    let mut m = SessionManifest::load();
    if m.sessions.remove(session).is_some() {
        let _ = m.save();
    }
}

/// Prune records the live tmux server proves dead — but only for sessions
/// recorded under the *same* server pid. Records from a previous server are
/// crash survivors awaiting `aw resurrect` and must not be touched, even if
/// a fresh server with unrelated sessions is already running (e.g. the user
/// opened a plain tmux after a reboot).
///
/// - a recorded session absent from `live_sessions` → deliberately killed → dropped
/// - a recorded pane absent from `live_pane_ids` → agent/pane exited → dropped
pub fn prune_with_live_server(
    live_sessions: &std::collections::HashSet<String>,
    live_pane_ids: &std::collections::HashSet<String>,
    live_server_pid: Option<u32>,
) {
    let pid = match live_server_pid {
        Some(p) => p,
        None => return,
    };
    let mut m = SessionManifest::load();
    let mut dirty = false;
    m.sessions.retain(|name, rec| {
        if rec.server_pid != Some(pid) {
            return true; // different (or unknown) server — not ours to judge
        }
        if !live_sessions.contains(name) {
            dirty = true;
            return false;
        }
        let before = rec.panes.len();
        rec.panes.retain(|pane_id, _| live_pane_ids.contains(pane_id));
        if rec.panes.len() != before {
            dirty = true;
        }
        true
    });
    if dirty {
        let _ = m.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::collections::HashSet;
    use tempfile::TempDir;

    fn with_state_dir<F: FnOnce()>(f: F) {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("AW_STATE_DIR", tmp.path());
        f();
        std::env::remove_var("AW_STATE_DIR");
    }

    fn seed(sessions: &[(&str, Option<u32>, &[(&str, &str)])]) {
        let mut m = SessionManifest::default();
        for (name, pid, panes) in sessions {
            let mut rec = SessionRecord {
                workspace: name.trim_start_matches("aw-").to_string(),
                cwd: format!("/ws/{}", name),
                server_pid: *pid,
                last_activity: 100,
                panes: BTreeMap::new(),
            };
            for (pane, agent) in *panes {
                rec.panes.insert(
                    pane.to_string(),
                    PaneRecord {
                        agent: agent.to_string(),
                        cwd: rec.cwd.clone(),
                        session_id: String::new(),
                        last_activity: 100,
                    },
                );
            }
            m.sessions.insert(name.to_string(), rec);
        }
        m.save().unwrap();
    }

    #[test]
    #[serial]
    fn load_tolerates_missing_and_garbage() {
        with_state_dir(|| {
            assert!(SessionManifest::load().sessions.is_empty());
            let path = manifest_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "not json").unwrap();
            assert!(SessionManifest::load().sessions.is_empty());
        });
    }

    #[test]
    #[serial]
    fn record_pane_ignores_non_aw_sessions() {
        with_state_dir(|| {
            record_pane("main", "x", "/x", "claude", "%1", "");
            assert!(SessionManifest::load().sessions.is_empty());
        });
    }

    #[test]
    #[serial]
    fn record_pane_roundtrips() {
        with_state_dir(|| {
            record_pane("aw-foo", "foo", "/ws/foo", "claude", "%3", "sid-123");
            let m = SessionManifest::load();
            let rec = &m.sessions["aw-foo"];
            assert_eq!(rec.workspace, "foo");
            assert_eq!(rec.panes["%3"].agent, "claude");
            assert_eq!(rec.panes["%3"].session_id, "sid-123");
        });
    }

    #[test]
    #[serial]
    fn remove_session_drops_entry() {
        with_state_dir(|| {
            seed(&[("aw-foo", None, &[])]);
            remove_session("aw-foo");
            assert!(SessionManifest::load().sessions.is_empty());
        });
    }

    #[test]
    #[serial]
    fn prune_only_touches_same_pid_records() {
        with_state_dir(|| {
            seed(&[
                // Same server: session gone → prune; pane %9 dead → prune.
                ("aw-killed", Some(42), &[("%1", "claude")]),
                ("aw-live", Some(42), &[("%2", "codex"), ("%9", "claude")]),
                // Older server: crash survivor, untouched.
                ("aw-crashed", Some(7), &[("%1", "claude")]),
                // Unknown server: untouched.
                ("aw-unknown", None, &[]),
            ]);
            let live_sessions: HashSet<String> = ["aw-live".to_string()].into();
            let live_panes: HashSet<String> = ["%2".to_string()].into();
            prune_with_live_server(&live_sessions, &live_panes, Some(42));

            let m = SessionManifest::load();
            assert!(!m.sessions.contains_key("aw-killed"));
            assert!(m.sessions.contains_key("aw-crashed"));
            assert!(m.sessions.contains_key("aw-unknown"));
            let live = &m.sessions["aw-live"];
            assert_eq!(live.panes.len(), 1);
            assert!(live.panes.contains_key("%2"));
        });
    }

    #[test]
    #[serial]
    fn prune_without_live_pid_is_a_noop() {
        with_state_dir(|| {
            seed(&[("aw-foo", Some(42), &[("%1", "claude")])]);
            prune_with_live_server(&HashSet::new(), &HashSet::new(), None);
            assert!(SessionManifest::load().sessions.contains_key("aw-foo"));
        });
    }
}
