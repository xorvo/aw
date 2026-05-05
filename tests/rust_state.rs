//! State-merge regression tests. The bugs these guard:
//!
//! - **Dead-pane row**: state file persists for a pane that no longer
//!   exists in tmux; the popup keeps showing it.
//! - **Sticky session/cwd**: a pane moves between sessions or `cd`s, but
//!   the dashboard reports the original.
//! - **Tmux-unavailable false-empties**: when the tmux server isn't
//!   running, the dashboard mustn't claim there are zero panes — it should
//!   fall back to whatever state files exist.
//! - **Auto-gc**: stale state files should be cleaned up the next time
//!   the dashboard loads with a healthy tmux, without `aw dash gc`.

mod common;

use std::process::Command;

use common::{Bin, TestEnv};

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A private tmux server with a short socket path (macOS caps Unix domain
/// socket paths at 104 chars, and the system tempdir alone usually eats
/// 50+).
struct PrivateTmux {
    socket_path: std::path::PathBuf,
    tmux_tmpdir: std::path::PathBuf,
}

impl PrivateTmux {
    fn spawn(env: &TestEnv, session: &str) -> Self {
        // /tmp/awts-<pid>-<random>.sock — short on purpose. tmux -S takes
        // a full path, bypassing the $TMUX_TMPDIR/tmux-<uid>/ prefix.
        let nonce: u32 = rand_u32();
        let socket_path = std::path::PathBuf::from(format!(
            "/tmp/awts-{}-{:x}.sock",
            std::process::id(),
            nonce
        ));
        let tmux_tmpdir = env.tmp.path().to_path_buf();

        // Defensive: kill any leftover server on this socket.
        let _ = Command::new("tmux")
            .args(["-S", socket_path.to_str().unwrap(), "kill-server"])
            .stderr(std::process::Stdio::null())
            .status();

        let out = Command::new("tmux")
            .args([
                "-S", socket_path.to_str().unwrap(),
                "new-session", "-d",
                "-x", "120", "-y", "30",
                "-s", session,
                "-c", env.home.to_str().unwrap(),
            ])
            .env("TMUX_TMPDIR", &tmux_tmpdir)
            .output()
            .expect("spawn tmux");
        assert!(
            out.status.success(),
            "could not start private tmux at {}: {:?}",
            socket_path.display(),
            String::from_utf8_lossy(&out.stderr),
        );
        Self { socket_path, tmux_tmpdir }
    }

    fn lines(&self, args: &[&str]) -> Vec<String> {
        let out = Command::new("tmux")
            .args(["-S", self.socket_path.to_str().unwrap()])
            .args(args)
            .env("TMUX_TMPDIR", &self.tmux_tmpdir)
            .output()
            .expect("tmux subcmd");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(String::from)
            .collect()
    }

    fn raw(&self, args: &[&str]) -> std::process::Output {
        Command::new("tmux")
            .args(["-S", self.socket_path.to_str().unwrap()])
            .args(args)
            .env("TMUX_TMPDIR", &self.tmux_tmpdir)
            .output()
            .expect("tmux subcmd")
    }

    /// First pane id of the named session.
    fn first_pane_id(&self, session: &str) -> String {
        self.lines(&["list-panes", "-t", session, "-F", "#{pane_id}"])
            .into_iter()
            .next()
            .expect("session has at least one pane")
    }

    /// Run `aw <args>` against this private server's socket. We shim
    /// `tmux` on PATH so calls from inside `aw` route to our private
    /// server transparently — without modifying any code in src/.
    fn run_aw(&self, env: &TestEnv, args: &[&str]) -> std::process::Output {
        // Drop a `tmux` shim into the existing fake_bin dir that prepends
        // `-S <socket_path>` to every invocation. This is the only way to
        // redirect calls inside the binary without polluting production
        // code with a `--tmux-socket` flag just for tests.
        let shim_path = env.fake_bin.join("tmux");
        let real_tmux = which_real_tmux().expect("real tmux must be on PATH");
        let shim = format!(
            "#!/bin/sh\nexec {} -S {} \"$@\"\n",
            shellesc(&real_tmux),
            shellesc(self.socket_path.to_str().unwrap()),
        );
        std::fs::write(&shim_path, shim).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&shim_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        // PATH must put fake_bin (with the shim) BEFORE the system tmux dir.
        let path = format!(
            "{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
            env.fake_bin.display()
        );
        Command::new(Bin::Rust.path())
            .args(args)
            .current_dir(&env.home)
            .env_clear()
            .env("HOME", &env.home)
            .env("PATH", path)
            .env("AW_INSTALL_DIR", &env.install_dir)
            .env("AW_WORKSPACES_DIR", &env.workspaces_dir)
            .env("AW_BIN_DIR", &env.bin_dir)
            .env("AW_CONFIG_FILE", &env.config_path)
            .env("AW_STATE_DIR", &env.state_dir)
            .env("TMUX_TMPDIR", &self.tmux_tmpdir)
            .env("LC_ALL", "en_US.UTF-8")
            .env("LANG", "en_US.UTF-8")
            .output()
            .expect("spawn aw")
    }
}

fn which_real_tmux() -> Option<String> {
    let candidates = ["/opt/homebrew/bin/tmux", "/usr/local/bin/tmux", "/usr/bin/tmux"];
    candidates.iter().find(|p| std::path::Path::new(p).is_file()).map(|s| s.to_string())
}

fn shellesc(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-')) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn rand_u32() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    nanos.wrapping_mul(2_654_435_761)
}

impl Drop for PrivateTmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-S", self.socket_path.to_str().unwrap(), "kill-server"])
            .env("TMUX_TMPDIR", &self.tmux_tmpdir)
            .stderr(std::process::Stdio::null())
            .status();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Drop a state file for a fake pane id directly (bypassing the hook). Used
/// to seed the bug scenarios without coordinating with a private tmux.
fn seed_state_file(env: &TestEnv, pane_id: &str, workspace: &str, agent: &str) {
    let dir = env.state_dir.join("panes");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}.json", pane_id));
    let body = serde_json::json!({
        "schema_version": 1,
        "pane_id": pane_id,
        "session": format!("aw-{}", workspace),
        "workspace": workspace,
        "cwd": "/tmp/old-cwd",
        "agent": agent,
        "status": "working",
        "last_event": "UserPromptSubmit",
        "last_activity": 1700000000u64,
        "last_prompt": "stale",
    });
    std::fs::write(path, body.to_string()).unwrap();
}

fn dash_json(env: &TestEnv, server: &PrivateTmux) -> serde_json::Value {
    let out = server.run_aw(env, &["dash", "json"]);
    assert!(
        out.status.success(),
        "dash json failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("dash json output is JSON")
}

// ---- bug 1: dead pane filtered out + auto-gc'd ----

#[test]
fn dead_pane_state_file_is_filtered_out_and_deleted() {
    if !tmux_available() {
        eprintln!("tmux not available; skipping");
        return;
    }
    let env = TestEnv::new();
    let server = PrivateTmux::spawn(&env, "aw-realws");

    // Live pane = aw-realws's first pane. Plus a stale state file pointing
    // to %999 (which doesn't exist on this private server).
    seed_state_file(&env, "%999", "ghost", "claude");
    let stale_path = env.state_dir.join("panes/%999.json");
    assert!(stale_path.is_file(), "precondition");

    let snapshot = dash_json(&env, &server);
    let panes: Vec<&str> = snapshot
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["pane_id"].as_str().unwrap())
        .collect();
    assert!(
        !panes.contains(&"%999"),
        "dead pane should not appear in snapshot: {:?}",
        panes
    );

    // Auto-gc: the file should be gone after the load.
    assert!(
        !stale_path.exists(),
        "stale state file should be auto-deleted on load"
    );
}

// ---- bug 2: tmux is authoritative for session/cwd/workspace ----

#[test]
fn live_pane_overrides_stale_session_and_cwd_from_state_file() {
    if !tmux_available() {
        eprintln!("tmux not available; skipping");
        return;
    }
    let env = TestEnv::new();
    let server = PrivateTmux::spawn(&env, "aw-current");

    let live_pane = server.first_pane_id("aw-current");
    // Seed a state file claiming the pane belongs to a different workspace
    // and a stale cwd. The dashboard should report what tmux says now.
    seed_state_file(&env, &live_pane, "old-workspace", "claude");

    let snapshot = dash_json(&env, &server);
    let entry = snapshot
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["pane_id"].as_str() == Some(&live_pane))
        .expect("live pane should be in snapshot");

    assert_eq!(
        entry["workspace"], "current",
        "tmux's session-derived workspace should win over stale state file"
    );
    assert_eq!(
        entry["session"], "aw-current",
        "session refreshed from tmux"
    );
    assert_ne!(
        entry["cwd"], "/tmp/old-cwd",
        "cwd should be refreshed from tmux"
    );
    // Hook-derived fields preserved.
    assert_eq!(entry["status"], "working");
    assert_eq!(entry["last_prompt"], "stale");
}

// ---- bug 3: tmux unavailable falls back instead of going empty ----

#[test]
fn tmux_unavailable_falls_back_to_state_files() {
    let env = TestEnv::new();
    // Seed two state files; don't start any private tmux server. The
    // sandbox's TMUX_TMPDIR points at env.tmp.path() (which exists but
    // contains no tmux server), so tmux list-panes will exit non-zero.
    seed_state_file(&env, "%1", "alpha", "claude");
    seed_state_file(&env, "%2", "beta", "codex");

    let out = env.run(Bin::Rust, &["dash", "json"]);
    assert!(
        out.status.success(),
        "dash json should succeed even without tmux"
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let panes: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["pane_id"].as_str().unwrap())
        .collect();
    assert!(
        panes.contains(&"%1") && panes.contains(&"%2"),
        "fallback should preserve all state files: {:?}",
        panes
    );

    // And — critically — the files should NOT be auto-deleted, because
    // we have no authority to call them dead.
    assert!(env.state_dir.join("panes/%1.json").is_file());
    assert!(env.state_dir.join("panes/%2.json").is_file());
}

// ---- bug 4: parked sentinels for dead panes also auto-cleaned ----

#[test]
fn dead_panes_parked_sentinels_are_also_cleaned() {
    if !tmux_available() {
        eprintln!("tmux not available; skipping");
        return;
    }
    let env = TestEnv::new();
    let server = PrivateTmux::spawn(&env, "aw-x");

    seed_state_file(&env, "%888", "ghost", "claude");
    let park_dir = env.state_dir.join("parked");
    std::fs::create_dir_all(&park_dir).unwrap();
    std::fs::write(park_dir.join("%888"), "").unwrap();

    let _ = dash_json(&env, &server);

    assert!(!park_dir.join("%888").exists(), "stale park sentinel should be auto-deleted");
}

// ---- bug 6: unhooked pane label uses window_name, not foreground command ----

#[test]
fn unhooked_pane_label_uses_window_name_not_current_command() {
    if !tmux_available() {
        eprintln!("tmux not available; skipping");
        return;
    }
    let env = TestEnv::new();
    let server = PrivateTmux::spawn(&env, "aw-named");
    // Rename the window so it has a stable, human-friendly label.
    let _ = server.raw(&["rename-window", "-t", "aw-named", "claude"]);

    let snapshot = dash_json(&env, &server);
    let entry = snapshot
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["workspace"].as_str() == Some("named"))
        .expect("named workspace pane");
    assert_eq!(
        entry["agent"], "claude",
        "should pick up the renamed window as the label, not the foreground shell"
    );
}

// ---- bug 7: empty tmux pane list does NOT trigger auto-gc ----

#[test]
fn empty_tmux_list_does_not_wipe_hook_files() {
    if !tmux_available() {
        eprintln!("tmux not available; skipping");
        return;
    }
    let env = TestEnv::new();
    // Spawn a server with one pane in a NON-aw session, so the aw-* filter
    // returns 0 panes even though tmux itself is healthy and has 1 pane.
    let server = PrivateTmux::spawn(&env, "not-aw-prefixed");

    seed_state_file(&env, "%50", "alpha", "claude");
    let path = env.state_dir.join("panes/%50.json");
    assert!(path.is_file(), "precondition");

    let _ = dash_json(&env, &server);

    // Auto-gc condition is "panes is non-empty AND id is missing." The
    // session above contributes one pane to `panes`, so the auto-gc loop
    // does run; %50 is not live, so it gets deleted. That's correct.
    // To exercise the *conservative* branch — empty list — we kill the
    // session and re-run.
    let _ = server.raw(&["kill-session", "-t", "not-aw-prefixed"]);

    seed_state_file(&env, "%51", "beta", "codex");
    let path2 = env.state_dir.join("panes/%51.json");
    assert!(path2.is_file());

    let _ = dash_json(&env, &server);

    assert!(
        path2.is_file(),
        "state file should NOT be auto-deleted when tmux returns zero panes"
    );
}

// ---- bug 5: live pane keeps its hook-derived status across loads ----

#[test]
fn live_panes_hook_status_is_preserved_across_loads() {
    if !tmux_available() {
        eprintln!("tmux not available; skipping");
        return;
    }
    let env = TestEnv::new();
    let server = PrivateTmux::spawn(&env, "aw-hooked");
    let live_pane = server.first_pane_id("aw-hooked");

    // Seed a state file marking this pane as `waiting`. After a load,
    // tmux should refresh ground-truth fields but `status: waiting` must
    // stick — that's the whole point of hook state.
    seed_state_file(&env, &live_pane, "hooked", "claude");
    // Override the seeded status to waiting via a manual edit (seeder
    // writes "working").
    let path = env.state_dir.join(format!("panes/{}.json", live_pane));
    let mut v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    v["status"] = serde_json::Value::String("waiting".into());
    std::fs::write(&path, v.to_string()).unwrap();

    let snapshot = dash_json(&env, &server);
    let entry = snapshot
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["pane_id"].as_str() == Some(&live_pane))
        .expect("live pane present");
    assert_eq!(entry["status"], "waiting", "hook status should survive merge");
}
