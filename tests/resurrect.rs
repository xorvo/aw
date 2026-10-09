//! `aw resurrect` against a real (sandboxed) tmux server.

mod common;

use common::{capture, TestEnv};
use std::process::Command;

/// Kills the sandbox's tmux server on drop, so a failed assertion never leaves
/// one running.
struct ServerGuard {
    tmpdir: std::path::PathBuf,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["kill-server"])
            .env("TMUX_TMPDIR", &self.tmpdir)
            .stderr(std::process::Stdio::null())
            .status();
    }
}

fn tmux_available() -> bool {
    Command::new("tmux").arg("-V").output().map(|o| o.status.success()).unwrap_or(false)
}

/// An agent that split helpers into its own window is one group on the
/// dashboard. Resurrect used to give every pane a window of its own, which
/// flattened the group into unrelated sessions.
#[test]
fn resurrect_rebuilds_a_window_of_helpers_under_its_lead() {
    if !tmux_available() {
        eprintln!("skipping: tmux unavailable");
        return;
    }
    let env = TestEnv::new();
    let _guard = ServerGuard { tmpdir: env.tmp.path().to_path_buf() };
    let ws = env.workspaces_dir.join("grp");
    std::fs::create_dir_all(&ws).unwrap();
    let cwd = ws.display().to_string();

    // `pi` has no default resume command, so nothing is typed into the panes.
    // Window @3 held a lead (%10) and two helpers; %4 was alone in @1. The
    // busiest pane is a helper, which must not become the window's lead.
    let pane = |sid: &str, act: u64, window: &str| {
        format!(
            r#"{{"agent":"pi","cwd":"{cwd}","session_id":"{sid}","last_activity":{act},"window_id":"{window}"}}"#
        )
    };
    let manifest = format!(
        r#"{{"schema_version":1,"sessions":{{"aw-grp":{{"workspace":"grp","cwd":"{cwd}",
            "server_pid":999999,"last_activity":100,"panes":{{
            "%10":{},"%11":{},"%12":{},"%4":{}}}}}}}}}"#,
        pane("lead", 30, "@3"),
        pane("helper-a", 10, "@3"),
        pane("helper-b", 50, "@3"),
        pane("solo", 40, "@1"),
    );
    std::fs::write(env.state_dir.join("sessions.json"), manifest).unwrap();

    let cap = capture(&env, &env.run(&["resurrect"]));
    assert_eq!(cap.exit, 0, "{}\n{}", cap.stdout, cap.stderr);

    // Panes per window, in creation order.
    let out = Command::new("tmux")
        .args(["list-panes", "-s", "-t", "aw-grp", "-F", "#{window_id} #{@aw_session_id}"])
        .envs(common::sandbox_env(&env))
        .output()
        .unwrap();
    let listing = String::from_utf8_lossy(&out.stdout);
    let mut windows: Vec<(String, Vec<String>)> = Vec::new();
    for line in listing.lines() {
        let (w, sid) = line.split_once(' ').unwrap();
        match windows.iter_mut().find(|(id, _)| id == w) {
            Some((_, sids)) => sids.push(sid.to_string()),
            None => windows.push((w.to_string(), vec![sid.to_string()])),
        }
    }
    let mut groups: Vec<Vec<String>> = windows.into_iter().map(|(_, s)| s).collect();
    groups.sort();
    assert_eq!(
        groups,
        vec![vec!["lead", "helper-a", "helper-b"], vec!["solo"]],
        "tmux listing:\n{}",
        listing
    );
    // Window membership is all the dashboard groups by
    // (`dash::state::assign_group_leads`), so this is the grouping it shows.

    // And the re-keyed manifest records the new windows, so a second crash
    // keeps the grouping too.
    let m: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(env.state_dir.join("sessions.json")).unwrap())
            .unwrap();
    let panes = m["sessions"]["aw-grp"]["panes"].as_object().unwrap();
    let window_of = |sid: &str| {
        panes.values().find(|p| p["session_id"] == sid).unwrap()["window_id"].as_str().unwrap().to_string()
    };
    assert!(window_of("lead").starts_with('@'));
    assert_eq!(window_of("lead"), window_of("helper-a"));
    assert_eq!(window_of("lead"), window_of("helper-b"));
    assert_ne!(window_of("lead"), window_of("solo"));
}

/// Seed `n` crashed sessions of one `pi` pane each, newest last.
fn seed_sessions(env: &TestEnv, n: usize) {
    let mut sessions = Vec::new();
    for i in 0..n {
        let ws = env.workspaces_dir.join(format!("s{i}"));
        std::fs::create_dir_all(&ws).unwrap();
        let cwd = ws.display();
        sessions.push(format!(
            r#""aw-s{i}":{{"workspace":"s{i}","cwd":"{cwd}","server_pid":999999,"last_activity":{i},
                "panes":{{"%{i}":{{"agent":"pi","cwd":"{cwd}","session_id":"","last_activity":{i}}}}}}}"#
        ));
    }
    std::fs::write(
        env.state_dir.join("sessions.json"),
        format!(r#"{{"schema_version":1,"sessions":{{{}}}}}"#, sessions.join(",")),
    )
    .unwrap();
}

/// A paced run takes minutes, so the plan says how long up front.
#[test]
fn resurrect_dry_run_states_the_pace() {
    let env = TestEnv::new().with_config(
        "default:\n  repos: []\nagent_config:\n  resume_commands:\n    pi: 'true'\n  resurrect_per_minute: 2\n",
    );
    seed_sessions(&env, 5);
    let cap = capture(&env, &env.run(&["resurrect", "--dry-run"]));
    assert_eq!(cap.exit, 0, "{}", cap.stderr);
    assert!(
        cap.stdout.contains("Would restore 5 session(s), launching 5 agent(s), at most 2/min (about 2 min):"),
        "{}",
        cap.stdout
    );
    // The flag overrides the config; 0 turns pacing off.
    let cap = capture(&env, &env.run(&["resurrect", "--dry-run", "--per-minute", "0"]));
    assert!(cap.stdout.contains("launching 5 agent(s):"), "{}", cap.stdout);
}

/// Every launch is counted against the total, newest session first.
#[test]
fn resurrect_reports_each_launch() {
    if !tmux_available() {
        eprintln!("skipping: tmux unavailable");
        return;
    }
    // `true` is a harmless stand-in for an agent's resume command.
    let env = TestEnv::new().with_config(
        "default:\n  repos: []\nagent_config:\n  resume_commands:\n    pi: 'true'\n",
    );
    let _guard = ServerGuard { tmpdir: env.tmp.path().to_path_buf() };
    seed_sessions(&env, 3);
    let cap = capture(&env, &env.run(&["resurrect", "--per-minute", "0"]));
    assert_eq!(cap.exit, 0, "{}\n{}", cap.stdout, cap.stderr);
    let launches: Vec<&str> = cap.stdout.lines().filter(|l| l.starts_with('[')).collect();
    assert_eq!(
        launches,
        vec![
            "[1/3] 🔁 aw-s2 — pi via `true`",
            "[2/3] 🔁 aw-s1 — pi via `true`",
            "[3/3] 🔁 aw-s0 — pi via `true`",
        ],
        "{}",
        cap.stdout
    );
    assert!(cap.stdout.contains("✅ Restored 3/3 session(s)"), "{}", cap.stdout);
}
