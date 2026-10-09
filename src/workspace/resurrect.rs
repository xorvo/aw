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
    /// When the *original* pane last saw agent activity. Carried through so
    /// the restored pane's dash row reports the real age instead of starting
    /// over at "just now" (or at "—", which is what an absent state file
    /// gets). Not part of the dedupe key.
    pub last_activity: u64,
    /// The pane's id and window under the *dead* server. Panes that shared a
    /// window are rebuilt into one window, lowest original id first — that
    /// pane is the lead the others were split off, so the dashboard groups
    /// them under it again. Not part of the dedupe key.
    pub pane_id: String,
    pub window_id: String,
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
///
/// Sessions to restore come most recently active first: launches are paced,
/// so a full server takes minutes, and the work you were just doing should be
/// back first.
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
            panes,
        });
    }
    // `panes` is newest first, so its head is the session's latest activity.
    plan.restore.sort_by_key(|r| std::cmp::Reverse(r.panes[0].last_activity));
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
    let mut panes: Vec<(&String, &PaneRecord)> =
        rec.panes.iter().filter(|(_, p)| !p.agent.is_empty()).collect();
    panes.sort_by(|a, b| b.1.last_activity.cmp(&a.1.last_activity));
    let mut out: Vec<RestorePane> = Vec::new();
    for (pane_id, p) in panes {
        let cwd = if p.cwd.is_empty() { rec.cwd.clone() } else { p.cwd.clone() };
        let candidate = RestorePane {
            agent: p.agent.clone(),
            cwd,
            session_id: p.session_id.clone(),
            last_activity: p.last_activity,
            pane_id: pane_id.clone(),
            window_id: p.window_id.clone(),
        };
        // Dedupe on the (agent, cwd, session_id) triple only. Panes are
        // sorted newest-first, so the survivor carries the newest timestamp.
        let dup = out.iter().any(|o| {
            o.agent == candidate.agent
                && o.cwd == candidate.cwd
                && o.session_id == candidate.session_id
        });
        if !dup {
            out.push(candidate);
        }
    }
    out
}

pub fn run(dry_run: bool, per_minute: Option<u32>) -> Result<()> {
    let paths = Paths::from_env()?;
    let config = Config::load_or_default(&paths.config_file);
    let per_minute = per_minute.unwrap_or_else(|| config.resurrect_per_minute());
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

    let launches = plan
        .restore
        .iter()
        .flat_map(|r| &r.panes)
        .filter(|p| p.resume(&config).is_some())
        .count();
    if dry_run {
        println!(
            "Would restore {} session(s), launching {} agent(s){}:",
            plan.restore.len(),
            launches,
            pace_summary(launches, per_minute)
        );
        for r in &plan.restore {
            print_session_plan(r, &config);
        }
        if !plan.prune.is_empty() {
            println!("Would prune {} session(s) (closed, gone, or nothing to resume): {}", plan.prune.len(), plan.prune.join(", "));
        }
        return Ok(());
    }

    println!(
        "🔁 Restoring {} session(s), launching {} agent(s){}",
        plan.restore.len(),
        launches,
        pace_summary(launches, per_minute)
    );
    let mut launcher = Launcher {
        pacer: Pacer::new(per_minute),
        launched: 0,
        total: launches,
        tty: std::io::IsTerminal::is_terminal(&std::io::stdout()),
    };
    let mut restored = 0usize;
    for r in &plan.restore {
        match restore_session(r, &config, &mut launcher) {
            Ok(new_panes) => {
                restored += 1;
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
                                    window_id: p.window_id,
                                },
                            )
                        })
                        .collect();
                }
                // Saved per session, not once at the end: a paced run takes
                // minutes, and an interrupted one must not forget the
                // sessions it already brought back.
                manifest.save()?;
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

/// `", at most 5/min (about 6 min)"` — or nothing when pacing won't bite.
fn pace_summary(launches: usize, per_minute: u32) -> String {
    if per_minute == 0 || launches <= per_minute as usize {
        return String::new();
    }
    // The first `per_minute` go at once; every further batch waits a minute.
    let minutes = (launches - 1) / per_minute as usize;
    format!(", at most {}/min (about {} min)", per_minute, minutes)
}

/// Sliding-window limit on agent launches: at most `per_minute` in any 60s.
///
/// What trips the providers' rate limits is starting many agent sessions at
/// once; creating tmux panes is free, so only launches are counted. Pure in
/// the clock so the arithmetic is testable.
struct Pacer {
    per_minute: u32,
    /// Times of the most recent launches, oldest first, at most `per_minute`.
    recent: std::collections::VecDeque<std::time::Instant>,
}

impl Pacer {
    const WINDOW: std::time::Duration = std::time::Duration::from_secs(60);

    fn new(per_minute: u32) -> Self {
        Self { per_minute, recent: std::collections::VecDeque::new() }
    }

    /// How long to wait before the next launch may go.
    fn delay(&self, now: std::time::Instant) -> std::time::Duration {
        if self.per_minute == 0 || self.recent.len() < self.per_minute as usize {
            return std::time::Duration::ZERO;
        }
        (self.recent[0] + Self::WINDOW).saturating_duration_since(now)
    }

    fn record(&mut self, now: std::time::Instant) {
        if self.per_minute == 0 {
            return;
        }
        self.recent.push_back(now);
        while self.recent.len() > self.per_minute as usize {
            self.recent.pop_front();
        }
    }
}

/// Paces agent launches and reports each one, so a run that takes minutes
/// shows where it is and why it is waiting.
struct Launcher {
    pacer: Pacer,
    launched: usize,
    total: usize,
    tty: bool,
}

impl Launcher {
    /// Block until the rate limit allows another launch, with a live
    /// countdown on a terminal (one line otherwise, so logs stay readable).
    fn wait_turn(&self) {
        use std::io::Write;
        let mut left = self.pacer.delay(std::time::Instant::now());
        if left.is_zero() {
            return;
        }
        if !self.tty {
            println!(
                "⏳ {}/min limit — waiting {}s before the next launch",
                self.pacer.per_minute,
                left.as_secs_f64().ceil() as u64
            );
            std::thread::sleep(left);
            return;
        }
        while !left.is_zero() {
            print!(
                "\r\x1b[2K⏳ {}/min limit — next launch in {}s ({}/{} launched)",
                self.pacer.per_minute,
                left.as_secs_f64().ceil() as u64,
                self.launched,
                self.total
            );
            let _ = std::io::stdout().flush();
            std::thread::sleep(left.min(std::time::Duration::from_secs(1)));
            left = self.pacer.delay(std::time::Instant::now());
        }
        print!("\r\x1b[2K");
        let _ = std::io::stdout().flush();
    }

    fn launched(&mut self, session: &str, agent: &str, cmd: &str) {
        self.pacer.record(std::time::Instant::now());
        self.launched += 1;
        let width = self.total.to_string().len();
        println!(
            "[{:>width$}/{}] 🔁 {} — {} via `{}`",
            self.launched,
            self.total,
            session,
            agent,
            cmd,
            width = width
        );
    }
}

fn print_session_plan(r: &RestoreSession, config: &Config) {
    for p in &r.panes {
        match p.resume(config) {
            Some(cmd) => println!("🔁 {} — {} via `{}`", r.session, p.agent, cmd),
            None => println!("🔁 {} — {} (no resume command; shell only)", r.session, p.agent),
        }
    }
}

/// Panes grouped by the window they shared, in the order to recreate them.
///
/// Windows come in the order their most recently active pane appears in
/// `panes` (already newest first), so the session still opens on the freshest
/// work. Inside a window, panes go in original creation order: the first is
/// the lead, the rest were split off it. A pane with no recorded window is a
/// window of its own, which is all a pre-window-tracking manifest can say.
fn window_groups(panes: &[RestorePane]) -> Vec<Vec<&RestorePane>> {
    let mut groups: Vec<Vec<&RestorePane>> = Vec::new();
    for p in panes {
        let existing = if p.window_id.is_empty() {
            None
        } else {
            groups.iter_mut().find(|g| g[0].window_id == p.window_id)
        };
        match existing {
            Some(g) => g.push(p),
            None => groups.push(vec![p]),
        }
    }
    for g in &mut groups {
        g.sort_by_key(|p| pane_ordinal(&p.pane_id));
    }
    groups
}

/// Numeric part of a tmux pane id (`%44` -> 44); tmux hands them out in
/// creation order. Unparseable ids sort last so they never become a lead.
fn pane_ordinal(pane_id: &str) -> u64 {
    pane_id.trim_start_matches('%').parse().unwrap_or(u64::MAX)
}

/// Create the session and one window per recorded window, splitting a
/// window's helpers back in beside their lead, and type the resume command
/// into each pane. Returns the new pane ids with what they run — window ids
/// rewritten to the new ones — for re-keying the manifest.
///
/// Each pane is created only once its launch is allowed, so a paced run never
/// leaves shells sitting idle in the dashboard waiting for their turn.
fn restore_session(
    r: &RestoreSession,
    config: &Config,
    launcher: &mut Launcher,
) -> Result<Vec<(String, RestorePane)>> {
    let mut out = Vec::new();
    for (gi, group) in window_groups(&r.panes).into_iter().enumerate() {
        // Split off the newest pane rather than the lead, so the helpers keep
        // their original on-screen order (a split lands right after its
        // target).
        let mut last = String::new();
        let mut window = String::new();
        for (i, p) in group.into_iter().enumerate() {
            let resume = p.resume(config);
            if resume.is_some() {
                launcher.wait_turn();
            }
            let pane_id = if i > 0 {
                let id = tmux_out(&[
                    "split-window", "-d", "-P", "-F", "#{pane_id}",
                    "-t", &last,
                    "-c", &p.cwd,
                ])?;
                // Re-spread after every split. A detached session is only
                // 80x24, and repeatedly halving the lead runs out of room after
                // a handful of helpers.
                tmux_out(&["select-layout", "-t", &window, "tiled"])?;
                id
            } else {
                let made = if gi == 0 {
                    tmux_out(&[
                        "new-session", "-d", "-P", "-F", "#{pane_id} #{window_id}",
                        "-s", &r.session,
                        "-c", &p.cwd,
                    ])?
                } else {
                    tmux_out(&[
                        "new-window", "-d", "-P", "-F", "#{pane_id} #{window_id}",
                        "-t", &r.session,
                        "-c", &p.cwd,
                    ])?
                };
                let (id, w) = made.split_once(' ').unwrap_or((made.as_str(), ""));
                window = w.to_string();
                id.to_string()
            };
            last = pane_id.clone();
            match resume {
                Some(cmd) => {
                    // Literal text then a separate Enter; tmux buffers the
                    // input until the pane's shell is ready to read it.
                    tmux_out(&["send-keys", "-t", &pane_id, "-l", "--", &cmd])?;
                    tmux_out(&["send-keys", "-t", &pane_id, "Enter"])?;
                    launcher.launched(&r.session, &p.agent, &cmd);
                }
                None => println!("   {} — {} (no resume command; shell only)", r.session, p.agent),
            }
            // Give the dashboard something to show before the resumed agent
            // fires its first hook: without a state file the row reads "—" for
            // last activity, and the pane's real age is lost to the restart.
            // Cosmetic, so a failed write must not fail an otherwise good
            // restore.
            let st = seeded_pane_state(&r.session, &pane_id, p);
            let _ = crate::dash::state::pane_state_path(&pane_id)
                .and_then(|path| st.write_atomic(&path));
            // The whole point of stamping here: a resumed agent fires no hook until
            // someone types in it, so without this the pane is anonymous to any
            // tmux binding that wants to know which conversation it holds.
            crate::dash::tmux::stamp_pane(&pane_id, &p.agent, &p.session_id);

            let mut restored = p.clone();
            restored.window_id = window.clone();
            out.push((pane_id, restored));
        }
    }
    Ok(out)
}

/// The state file a restored pane starts life with: hook-derived identity
/// from the manifest, `last_activity` from the *original* pane, and no
/// status/event/prompt history (the manifest never recorded those).
///
/// `session`, `workspace` and `cwd` are refreshed from tmux on every dash
/// load, so they are filled for completeness rather than correctness.
fn seeded_pane_state(
    session: &str,
    pane_id: &str,
    p: &RestorePane,
) -> crate::dash::state::PaneState {
    let mut st = crate::dash::state::PaneState::new(pane_id, &p.agent);
    st.session = session.to_string();
    st.workspace = session.trim_start_matches("aw-").to_string();
    st.cwd = p.cwd.clone();
    st.session_id = p.session_id.clone();
    st.last_activity = p.last_activity;
    st.server_pid = crate::dash::tmux::server_pid();
    st
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
    crate::manifest::agent_hints(manifest, live_pid)
        .into_iter()
        .collect()
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
                window_id: p.window_id.clone(),
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
                    window_id: String::new(),
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
            last_activity: 5,
            pane_id: "%1".into(),
            window_id: String::new(),
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
            aw_agent: String::new(),
            aw_session_id: String::new(),
            aw_sidebar: false,
            window_id: "@4".into(),
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
        assert_eq!(foo.panes["%3"].window_id, "@4");
        assert!(!foo.panes.contains_key("%4"));
    }

    #[test]
    fn seeded_state_keeps_the_original_activity_stamp() {
        let p = RestorePane {
            agent: "claude".into(),
            cwd: "/ws/w".into(),
            session_id: "sid-1".into(),
            last_activity: 1_700_000_000,
            pane_id: "%3".into(),
            window_id: "@1".into(),
        };
        let st = seeded_pane_state("aw-w", "%9", &p);
        assert_eq!(st.pane_id, "%9");
        assert_eq!(st.session, "aw-w");
        assert_eq!(st.workspace, "w");
        assert_eq!(st.agent, "claude");
        assert_eq!(st.cwd, "/ws/w");
        assert_eq!(st.session_id, "sid-1");
        // The whole point: not `now`.
        assert_eq!(st.last_activity, 1_700_000_000);
        assert_eq!(st.status, crate::dash::state::Status::Idle);
        assert!(st.last_event.is_empty());
        assert!(st.last_prompt.is_empty());
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
        // The surviving id-less claude carries %2's timestamp (20) — the
        // newest of the three that collapsed (%1=10, %2=20, %4=5).
        assert_eq!(panes, vec![
            RestorePane { agent: "claude".into(), cwd: "/ws/w".into(), session_id: "sid-a".into(), last_activity: 40, pane_id: "%5".into(), window_id: "".into() },
            RestorePane { agent: "claude".into(), cwd: "/ws/w".into(), session_id: "sid-b".into(), last_activity: 30, pane_id: "%6".into(), window_id: "".into() },
            RestorePane { agent: "claude".into(), cwd: "/ws/w".into(), session_id: "".into(), last_activity: 20, pane_id: "%2".into(), window_id: "".into() },
            RestorePane { agent: "codex".into(), cwd: "/ws/w".into(), session_id: "".into(), last_activity: 15, pane_id: "%3".into(), window_id: "".into() },
        ]);
    }

    #[test]
    fn restore_order_is_most_recently_active_session_first() {
        let m = manifest(vec![
            ("aw-a", rec("a", Some(7), &[("%1", "claude", "/ws/a", "", 10)])),
            ("aw-b", rec("b", Some(7), &[("%2", "claude", "/ws/b", "", 30), ("%3", "claude", "/ws/b", "x", 1)])),
            ("aw-c", rec("c", Some(7), &[("%4", "claude", "/ws/c", "", 20)])),
        ]);
        let plan = build_plan(&m, None, None, |_| true);
        let order: Vec<&str> = plan.restore.iter().map(|r| r.session.as_str()).collect();
        assert_eq!(order, vec!["aw-b", "aw-c", "aw-a"]);
    }

    #[test]
    fn pacer_allows_a_burst_then_waits_out_the_window() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let mut p = Pacer::new(3);
        for i in 0..3 {
            assert_eq!(p.delay(t0 + Duration::from_secs(i)), Duration::ZERO);
            p.record(t0 + Duration::from_secs(i));
        }
        // Fourth launch at t=5: waits until the first (t=0) is 60s old.
        assert_eq!(p.delay(t0 + Duration::from_secs(5)), Duration::from_secs(55));
        p.record(t0 + Duration::from_secs(60));
        // Now the oldest in the window is t=1.
        assert_eq!(p.delay(t0 + Duration::from_secs(60)), Duration::from_secs(1));
        assert_eq!(p.delay(t0 + Duration::from_secs(61)), Duration::ZERO);
    }

    #[test]
    fn pacer_zero_is_unlimited() {
        let now = std::time::Instant::now();
        let mut p = Pacer::new(0);
        for _ in 0..100 {
            p.record(now);
        }
        assert!(p.delay(now).is_zero());
    }

    #[test]
    fn pace_summary_only_when_the_limit_bites() {
        assert_eq!(pace_summary(5, 5), "");
        assert_eq!(pace_summary(40, 0), "");
        assert_eq!(pace_summary(6, 5), ", at most 5/min (about 1 min)");
        assert_eq!(pace_summary(33, 5), ", at most 5/min (about 6 min)");
    }

    fn restore_pane(pane_id: &str, window_id: &str, last_activity: u64) -> RestorePane {
        RestorePane {
            agent: "claude".into(),
            cwd: "/ws/w".into(),
            session_id: format!("sid-{}", pane_id),
            last_activity,
            pane_id: pane_id.into(),
            window_id: window_id.into(),
        }
    }

    #[test]
    fn panes_that_shared_a_window_are_rebuilt_together_lead_first() {
        // Newest first, as `dedupe_panes` hands them over. The busiest pane is
        // a helper (%12) in @3; its lead %10 is older, but must still come
        // first so the others are split off it.
        let panes = vec![
            restore_pane("%12", "@3", 50),
            restore_pane("%4", "@1", 40),
            restore_pane("%10", "@3", 30),
            restore_pane("%7", "", 20),
            restore_pane("%11", "@3", 10),
            restore_pane("%8", "", 5),
        ];
        let ids: Vec<Vec<&str>> = window_groups(&panes)
            .iter()
            .map(|g| g.iter().map(|p| p.pane_id.as_str()).collect())
            .collect();
        assert_eq!(ids, vec![
            vec!["%10", "%11", "%12"],
            vec!["%4"],
            // No recorded window: never merged with each other.
            vec!["%7"],
            vec!["%8"],
        ]);
    }
}
