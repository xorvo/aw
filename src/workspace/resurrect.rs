//! `aw resurrect [--dry-run]` — rebuild aw tmux sessions after a server
//! death (an agent ran `kill-server`, a power cut, a reboot).
//!
//! The durable inputs: the session manifest (`sessions.json`, written by
//! `aw hook` / `aw start`) says which `aw-*` sessions existed and which
//! agents ran in them; the workspace dirs on disk still exist; and the
//! agents' own CLIs persist their conversations per directory. So we
//! recreate each missing session rooted in its workspace dir and type the
//! agent's *resume command* (`claude --continue`, `codex resume --last`,
//! …) into the fresh pane — the agent picks its conversation back up.
//!
//! Sessions the live server proves were closed on purpose (recorded under
//! the *same* server pid but absent now) are pruned, not restored — see
//! the pid semantics in [`crate::manifest`].

use anyhow::Result;

use crate::config::Config;
use crate::dash::tmux::tmux_command;
use crate::manifest::{PaneRecord, SessionManifest, SessionRecord};
use crate::paths::Paths;

/// One pane to recreate: which agent ran there, rooted where, and — when
/// the hooks reported one — which exact conversation to reopen.
#[derive(Debug, Clone, PartialEq)]
pub struct RestorePane {
    pub agent: String,
    pub cwd: String,
    pub session_id: String,
}

impl RestorePane {
    fn resume(&self, config: &Config) -> Option<String> {
        let sid = if self.session_id.is_empty() { None } else { Some(self.session_id.as_str()) };
        config.resume_command(&self.agent, sid)
    }
}

#[derive(Debug, Clone)]
pub struct RestoreSession {
    pub session: String,
    pub cwd: String,
    /// Deduped by (agent, cwd, session_id), most recently active first.
    /// Never empty: sessions with no agent to resume are pruned, not
    /// restored as bare shells.
    pub panes: Vec<RestorePane>,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub restore: Vec<RestoreSession>,
    /// Deliberately closed under the still-running server, workspace dir
    /// gone, or no agent recorded to resume — dropped from the manifest.
    pub prune: Vec<String>,
    pub already_live: Vec<String>,
}

/// Classify every manifest session. Pure — all tmux/filesystem answers are
/// passed in — so the decision table is unit-testable without a server.
pub fn build_plan(
    manifest: &SessionManifest,
    live_sessions: Option<&[String]>,
    live_server_pid: Option<u32>,
    workspace_dir_exists: impl Fn(&str) -> bool,
) -> Plan {
    let mut plan = Plan::default();
    for (name, rec) in &manifest.sessions {
        if live_sessions.map_or(false, |l| l.iter().any(|s| s == name)) {
            plan.already_live.push(name.clone());
            continue;
        }
        // Missing from tmux. Same server pid → it never died → the session
        // was closed on purpose.
        if live_server_pid.is_some() && rec.server_pid == live_server_pid {
            plan.prune.push(name.clone());
            continue;
        }
        if !workspace_dir_exists(&rec.workspace) {
            plan.prune.push(name.clone());
            continue;
        }
        // Nothing but shells (or agents that never fired a hook) → an
        // empty pane is worse than no session at all.
        let panes = dedupe_panes(rec);
        if panes.is_empty() {
            plan.prune.push(name.clone());
            continue;
        }
        plan.restore.push(RestoreSession {
            session: name.clone(),
            cwd: rec.cwd.clone(),
            panes,
        });
    }
    plan
}

/// Distinct (agent, cwd, session_id) triples from a session's recorded
/// panes, most recently active first. Panes with distinct conversation ids
/// are all kept — each resumes its own conversation. Only id-less
/// duplicates in the same directory collapse, since the `--continue`
/// fallback can only reopen that directory's latest conversation anyway.
/// Agent-less panes (plain shells, snapshot-seen tools we don't know) are
/// dropped — there is nothing to resume in them.
fn dedupe_panes(rec: &SessionRecord) -> Vec<RestorePane> {
    let mut panes: Vec<&PaneRecord> = rec.panes.values().filter(|p| !p.agent.is_empty()).collect();
    panes.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
    let mut out: Vec<RestorePane> = Vec::new();
    for p in panes {
        let cwd = if p.cwd.is_empty() { rec.cwd.clone() } else { p.cwd.clone() };
        let candidate = RestorePane {
            agent: p.agent.clone(),
            cwd,
            session_id: p.session_id.clone(),
        };
        if !out.contains(&candidate) {
            out.push(candidate);
        }
    }
    out
}

pub fn run(dry_run: bool) -> Result<()> {
    let paths = Paths::from_env()?;
    let config = Config::load_or_default(&paths.config_file);
    let mut manifest = SessionManifest::load();
    if manifest.sessions.is_empty() {
        println!("Nothing to resurrect — no sessions recorded yet.");
        return Ok(());
    }

    let live = crate::dash::tmux::list_session_names();
    let live_pid = crate::dash::tmux::server_pid();
    let plan = build_plan(&manifest, live.as_deref(), live_pid, |ws| {
        paths.workspace_dir(ws).is_dir()
    });

    for s in &plan.already_live {
        println!("⏭️  {} — already running", s);
    }
    if plan.restore.is_empty() {
        if !plan.prune.is_empty() && !dry_run {
            for name in &plan.prune {
                manifest.sessions.remove(name);
            }
            manifest.save()?;
            println!("🧹 Pruned {} session(s) from the manifest (closed, gone, or nothing to resume)", plan.prune.len());
        }
        println!("Nothing to resurrect.");
        return Ok(());
    }

    if dry_run {
        println!("Would restore {} session(s):", plan.restore.len());
        for r in &plan.restore {
            print_session_plan(r, &config);
        }
        if !plan.prune.is_empty() {
            println!("Would prune {} session(s) (closed, gone, or nothing to resume): {}", plan.prune.len(), plan.prune.join(", "));
        }
        return Ok(());
    }

    let mut restored = 0usize;
    for r in &plan.restore {
        match restore_session(r, &config) {
            Ok(new_panes) => {
                restored += 1;
                print_session_plan(r, &config);
                // Re-key the manifest onto the fresh panes/server so a
                // second crash resurrects the resurrected sessions too.
                if let Some(rec) = manifest.sessions.get_mut(&r.session) {
                    rec.server_pid = crate::dash::tmux::server_pid();
                    rec.last_activity = crate::dash::state::now_epoch();
                    rec.panes = new_panes
                        .into_iter()
                        .map(|(pane_id, p)| {
                            (
                                pane_id,
                                PaneRecord {
                                    agent: p.agent,
                                    cwd: p.cwd,
                                    session_id: p.session_id,
                                    last_activity: crate::dash::state::now_epoch(),
                                },
                            )
                        })
                        .collect();
                }
            }
            Err(e) => eprintln!("❌ {} — {}", r.session, e),
        }
    }
    for name in &plan.prune {
        manifest.sessions.remove(name);
    }
    manifest.save()?;

    if !plan.prune.is_empty() {
        println!("🧹 Pruned {} session(s) from the manifest (closed, gone, or nothing to resume)", plan.prune.len());
    }
    println!("✅ Restored {}/{} session(s)", restored, plan.restore.len());
    if restored > 0 {
        println!("   Attach with `aw start <name>` or `tmux attach -t aw-<name>`.");
    }
    Ok(())
}

fn print_session_plan(r: &RestoreSession, config: &Config) {
    for p in &r.panes {
        match p.resume(config) {
            Some(cmd) => println!("🔁 {} — {} via `{}`", r.session, p.agent, cmd),
            None => println!("🔁 {} — {} (no resume command; shell only)", r.session, p.agent),
        }
    }
}

/// Create the session and one window per recorded (agent, cwd), typing the
/// resume command into each. Returns the new pane ids with what they run,
/// for re-keying the manifest.
fn restore_session(r: &RestoreSession, config: &Config) -> Result<Vec<(String, RestorePane)>> {
    let first_cwd = r.panes.first().map(|p| p.cwd.as_str()).unwrap_or(r.cwd.as_str());
    let first_pane = tmux_out(&[
        "new-session", "-d", "-P", "-F", "#{pane_id}",
        "-s", &r.session,
        "-c", first_cwd,
    ])?;

    let mut out = Vec::new();
    for (i, p) in r.panes.iter().enumerate() {
        let pane_id = if i == 0 {
            first_pane.clone()
        } else {
            tmux_out(&[
                "new-window", "-d", "-P", "-F", "#{pane_id}",
                "-t", &r.session,
                "-c", &p.cwd,
            ])?
        };
        if let Some(cmd) = p.resume(config) {
            // Literal text then a separate Enter; tmux buffers the input
            // until the pane's shell is ready to read it.
            tmux_out(&["send-keys", "-t", &pane_id, "-l", "--", &cmd])?;
            tmux_out(&["send-keys", "-t", &pane_id, "Enter"])?;
        }
        out.push((pane_id, p.clone()));
    }
    Ok(out)
}

/// Agent binaries we recognize in `pane_current_command` when no hook state
/// exists for a pane (agent launched but no event fired yet, or hooks not
/// installed for that agent).
const KNOWN_AGENTS: &[&str] = &["claude", "codex", "opencode", "kimi", "pi"];

/// `aw snapshot` — capture the *live* tmux truth into the resurrect
/// manifest, on demand. Richer than the hook-driven records: it sees every
/// `aw-*` window including plain shells. Run it before a planned shutdown,
/// then `aw resurrect` after boot.
pub fn snapshot() -> Result<()> {
    let panes = match crate::dash::tmux::list_panes_with_metadata() {
        crate::dash::tmux::PaneListing::Tmux(p) => p,
        crate::dash::tmux::PaneListing::Unavailable => {
            anyhow::bail!("no tmux server running — nothing to snapshot")
        }
    };
    let live_pid = crate::dash::tmux::server_pid();
    let mut manifest = SessionManifest::load();

    // Agent type + conversation id per pane. Seeded from what the manifest
    // already knows about *this* server's panes (a resurrected agent the
    // user hasn't typed into yet has no hook file, and its
    // `pane_current_command` is useless — Claude's native binary reports
    // its version string, e.g. `2.1.267`), then overlaid with hook state,
    // which is always at least as fresh.
    let mut hook_info = manifest_agent_hints(&manifest, live_pid);
    if let Ok(read) = std::fs::read_dir(crate::dash::panes_dir()?) {
        for d in read.flatten() {
            if let Ok(s) = crate::dash::state::PaneState::read(&d.path()) {
                hook_info.insert(s.pane_id.clone(), (s.agent, s.session_id));
            }
        }
    }

    let now = crate::dash::state::now_epoch();
    let records = build_snapshot_records(&panes, &hook_info, live_pid, now);
    // The snapshot is authoritative for *this* server: entries recorded
    // under the same pid but absent now were closed on purpose. Records
    // from older pids are crash survivors awaiting resurrect — keep them.
    if live_pid.is_some() {
        manifest
            .sessions
            .retain(|name, rec| rec.server_pid != live_pid || records.contains_key(name));
    }
    let n_sessions = records.len();
    let n_agents: usize = records
        .values()
        .flat_map(|r| r.panes.values())
        .filter(|p| !p.agent.is_empty())
        .count();
    for (name, rec) in records {
        manifest.sessions.insert(name, rec);
    }
    manifest.save()?;
    println!(
        "📸 Snapshotted {} session(s) ({} agent pane(s)) to the resurrect manifest",
        n_sessions, n_agents
    );
    Ok(())
}

/// `pane_id → (agent, session_id)` for every agent pane the manifest
/// recorded under the live server. Pane ids are never reused within one
/// server lifetime, so same pid + same id is the same pane. Records from
/// other pids are dead panes whose ids the new server may have handed out
/// again — ignored.
fn manifest_agent_hints(
    manifest: &SessionManifest,
    live_pid: Option<u32>,
) -> std::collections::HashMap<String, (String, String)> {
    let mut out = std::collections::HashMap::new();
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

/// Pure core of `aw snapshot`: fold the live pane list into per-session
/// records, enriched with hook-known agent/conversation info.
fn build_snapshot_records(
    panes: &[crate::dash::tmux::PaneInfo],
    hook_info: &std::collections::HashMap<String, (String, String)>,
    server_pid: Option<u32>,
    now: u64,
) -> std::collections::BTreeMap<String, SessionRecord> {
    let mut out = std::collections::BTreeMap::new();
    for p in panes {
        if !p.session.starts_with("aw-") {
            continue;
        }
        let rec = out.entry(p.session.clone()).or_insert_with(|| SessionRecord {
            workspace: p.session.trim_start_matches("aw-").to_string(),
            cwd: p.path.clone(),
            server_pid,
            last_activity: now,
            panes: std::collections::BTreeMap::new(),
        });
        let (agent, session_id) = match hook_info.get(&p.pane_id) {
            Some((a, sid)) => (a.clone(), sid.clone()),
            None if KNOWN_AGENTS.contains(&p.command.as_str()) => (p.command.clone(), String::new()),
            None => (String::new(), String::new()), // plain shell / other tool
        };
        rec.panes.insert(
            p.pane_id.clone(),
            PaneRecord {
                agent,
                cwd: p.path.clone(),
                session_id,
                last_activity: now,
            },
        );
    }
    out
}

/// Run tmux, bail on failure, return trimmed stdout.
fn tmux_out(args: &[&str]) -> Result<String> {
    let out = tmux_command().args(args).output()?;
    if !out.status.success() {
        anyhow::bail!(
            "tmux {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn rec(
        workspace: &str,
        pid: Option<u32>,
        panes: &[(&str, &str, &str, &str, u64)],
    ) -> SessionRecord {
        let mut map = BTreeMap::new();
        for (id, agent, cwd, sid, act) in panes {
            map.insert(
                id.to_string(),
                PaneRecord {
                    agent: agent.to_string(),
                    cwd: cwd.to_string(),
                    session_id: sid.to_string(),
                    last_activity: *act,
                },
            );
        }
        SessionRecord {
            workspace: workspace.to_string(),
            cwd: format!("/ws/{}", workspace),
            server_pid: pid,
            last_activity: 100,
            panes: map,
        }
    }

    fn manifest(entries: Vec<(&str, SessionRecord)>) -> SessionManifest {
        SessionManifest {
            schema_version: 1,
            sessions: entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        }
    }

    #[test]
    fn plan_classifies_live_killed_and_crashed() {
        let m = manifest(vec![
            ("aw-live", rec("live", Some(42), &[])),
            ("aw-killed", rec("killed", Some(42), &[])),
            ("aw-crashed", rec("crashed", Some(7), &[("%1", "claude", "/ws/crashed", "sid-1", 5)])),
            ("aw-nodir", rec("nodir", Some(7), &[])),
            // Crashed, dir exists, but only a shell was recorded → prune,
            // never an empty pane.
            ("aw-shell", rec("shell", Some(7), &[("%2", "", "/ws/shell", "", 5)])),
        ]);
        let live = vec!["aw-live".to_string(), "main".to_string()];
        let plan = build_plan(&m, Some(&live), Some(42), |ws| ws != "nodir");

        assert_eq!(plan.already_live, vec!["aw-live"]);
        assert_eq!(plan.prune, vec!["aw-killed", "aw-nodir", "aw-shell"]);
        assert_eq!(plan.restore.len(), 1);
        assert_eq!(plan.restore[0].session, "aw-crashed");
        assert_eq!(plan.restore[0].panes, vec![RestorePane {
            agent: "claude".into(),
            cwd: "/ws/crashed".into(),
            session_id: "sid-1".into(),
        }]);
    }

    #[test]
    fn plan_with_no_server_restores_everything_known() {
        // Power-outage case: no live server at all → every recorded
        // session is a candidate, regardless of its recorded pid.
        let m = manifest(vec![
            ("aw-a", rec("a", Some(42), &[("%1", "claude", "/ws/a", "", 1)])),
            ("aw-b", rec("b", None, &[("%2", "codex", "/ws/b", "", 1)])),
        ]);
        let plan = build_plan(&m, None, None, |_| true);
        assert_eq!(plan.restore.len(), 2);
        assert!(plan.prune.is_empty());
    }

    #[test]
    fn plan_fresh_server_after_reboot_does_not_prune_old_records() {
        // User opened a plain tmux after reboot: server pid 99, but the
        // records were made under pid 42 → still resurrect candidates.
        let m = manifest(vec![("aw-a", rec("a", Some(42), &[("%1", "claude", "/ws/a", "", 1)]))]);
        let live = vec!["main".to_string()];
        let plan = build_plan(&m, Some(&live), Some(99), |_| true);
        assert_eq!(plan.restore.len(), 1);
        assert!(plan.prune.is_empty());
    }

    #[test]
    fn snapshot_records_enrich_from_hooks_then_command_and_skip_non_aw() {
        use crate::dash::tmux::PaneInfo;
        let pane = |id: &str, session: &str, command: &str| PaneInfo {
            pane_id: id.into(),
            session: session.into(),
            window_name: "w".into(),
            pane_title: "t".into(),
            command: command.into(),
            path: "/ws/foo".into(),
        };
        let panes = vec![
            pane("%1", "aw-foo", "zsh"),    // plain shell
            pane("%2", "aw-foo", "node"),   // hook-known agent (command lies)
            pane("%3", "aw-foo", "codex"),  // no hook state; command matches
            pane("%4", "main", "claude"),   // not an aw session → skipped
        ];
        let mut hooks = std::collections::HashMap::new();
        hooks.insert("%2".to_string(), ("claude".to_string(), "sid-9".to_string()));

        let recs = build_snapshot_records(&panes, &hooks, Some(42), 500);
        assert_eq!(recs.len(), 1);
        let foo = &recs["aw-foo"];
        assert_eq!(foo.workspace, "foo");
        assert_eq!(foo.server_pid, Some(42));
        assert_eq!(foo.panes["%1"].agent, "");
        assert_eq!(foo.panes["%2"].agent, "claude");
        assert_eq!(foo.panes["%2"].session_id, "sid-9");
        assert_eq!(foo.panes["%3"].agent, "codex");
        assert!(!foo.panes.contains_key("%4"));
    }

    #[test]
    fn manifest_hints_only_from_live_server_and_agent_panes() {
        let m = manifest(vec![
            // Same server: a resurrected claude nobody typed into yet.
            ("aw-a", rec("a", Some(42), &[
                ("%16", "claude", "/ws/a", "sid-16", 1),
                ("%17", "", "/ws/a", "", 1), // shell → no hint
            ])),
            // Old server: %16 there was a different pane.
            ("aw-b", rec("b", Some(7), &[("%16", "codex", "/ws/b", "sid-old", 1)])),
        ]);
        let hints = manifest_agent_hints(&m, Some(42));
        assert_eq!(hints.len(), 1);
        assert_eq!(hints["%16"], ("claude".to_string(), "sid-16".to_string()));
        assert!(manifest_agent_hints(&m, None).is_empty());
    }

    #[test]
    fn dedupe_drops_agentless_panes() {
        let r = rec("w", None, &[("%1", "", "/ws/w", "", 10), ("%2", "claude", "/ws/w", "", 5)]);
        let panes = dedupe_panes(&r);
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].agent, "claude");
    }

    #[test]
    fn dedupe_collapses_idless_duplicates_but_keeps_distinct_conversations() {
        let r = rec(
            "w",
            None,
            &[
                // Two id-less claude panes in the same dir → collapse.
                ("%1", "claude", "/ws/w", "", 10),
                ("%2", "claude", "/ws/w", "", 20),
                ("%3", "codex", "/ws/w", "", 15),
                ("%4", "claude", "", "", 5), // empty cwd falls back to session cwd
                // Two claude panes with distinct conversation ids → both kept.
                ("%5", "claude", "/ws/w", "sid-a", 40),
                ("%6", "claude", "/ws/w", "sid-b", 30),
            ],
        );
        let panes = dedupe_panes(&r);
        assert_eq!(panes, vec![
            RestorePane { agent: "claude".into(), cwd: "/ws/w".into(), session_id: "sid-a".into() },
            RestorePane { agent: "claude".into(), cwd: "/ws/w".into(), session_id: "sid-b".into() },
            RestorePane { agent: "claude".into(), cwd: "/ws/w".into(), session_id: "".into() },
            RestorePane { agent: "codex".into(), cwd: "/ws/w".into(), session_id: "".into() },
        ]);
    }
}
