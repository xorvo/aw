//! Sidebar tests. We run against a private tmux server (`-L <socket>`) so
//! we don't disturb the host's tmux state, and tag the env so the binary's
//! tmux calls land on that same server.

mod common;

use std::process::Command;

use common::{Bin, TestEnv};

fn tmux_socket() -> String {
    format!("aw-sidebar-test-{}", std::process::id())
}

/// Boot a private tmux server with one session, return the (socket, session).
struct PrivateTmux {
    socket: String,
}

impl PrivateTmux {
    fn spawn(env: &TestEnv) -> Self {
        let socket = tmux_socket();
        // Make sure no leftover server with this name exists.
        let _ = Command::new("tmux")
            .args(["-L", &socket, "kill-server"])
            .stderr(std::process::Stdio::null())
            .status();
        // Set TMUX_TMPDIR so our server's socket is sandboxed under env.tmp.
        Command::new("tmux")
            .args([
                "-L", &socket,
                "new-session", "-d",
                "-x", "120", "-y", "30",
                "-s", "probe",
                "-c", env.home.to_str().unwrap(),
            ])
            .env("TMUX_TMPDIR", env.tmp.path())
            .status()
            .expect("spawn private tmux");
        Self { socket }
    }

    fn send(&self, env: &TestEnv, args: &[&str]) -> std::process::Output {
        let mut cmd = Command::new("tmux");
        cmd.args(["-L", &self.socket]).args(args);
        cmd.env("TMUX_TMPDIR", env.tmp.path())
            .output()
            .expect("tmux subcmd")
    }

    /// Run `aw <args>` *inside* the server (not via send-keys; via env-set
    /// `TMUX` so the binary thinks it's inside this server's session).
    fn run_aw(&self, env: &TestEnv, args: &[&str]) -> std::process::Output {
        // Resolve the session's TMUX env var by asking tmux directly.
        let info = self.send(env, &[
            "list-sessions", "-F", "#{socket_path}\t#{session_id}",
        ]);
        let line = String::from_utf8_lossy(&info.stdout);
        let mut parts = line.lines().next().unwrap_or("").splitn(2, '\t');
        let socket_path = parts.next().unwrap_or("").to_string();
        let session_id = parts.next().unwrap_or("").to_string();
        let tmux_var = format!("{},{},{}", socket_path, std::process::id(), session_id.trim_start_matches('$'));

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
            .env("TMUX_TMPDIR", env.tmp.path())
            .env("TMUX", tmux_var)
            .env("LC_ALL", "en_US.UTF-8")
            .env("LANG", "en_US.UTF-8")
            .output()
            .expect("spawn aw")
    }

    fn pane_count(&self, env: &TestEnv) -> usize {
        let out = self.send(env, &["list-panes", "-s", "-t", "probe", "-F", "#{pane_id}"]);
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .count()
    }

    fn sidebar_count(&self, env: &TestEnv) -> usize {
        let out = self.send(
            env,
            &["list-panes", "-s", "-t", "probe", "-F", "#{pane_id}\t#{@aw-sidebar}"],
        );
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| {
                let mut it = l.splitn(2, '\t');
                let _ = it.next();
                it.next() == Some("1")
            })
            .count()
    }
}

impl Drop for PrivateTmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .stderr(std::process::Stdio::null())
            .status();
    }
}

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
fn first_invocation_creates_one_tagged_sidebar() {
    if !tmux_available() {
        eprintln!("tmux not on PATH; skipping");
        return;
    }
    let env = TestEnv::new();
    let server = PrivateTmux::spawn(&env);
    assert_eq!(server.pane_count(&env), 1, "fresh session should have one pane");

    let out = server.run_aw(&env, &["dash", "sidebar"]);
    assert!(
        out.status.success(),
        "aw dash sidebar failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(server.pane_count(&env), 2, "should have split a sidebar");
    assert_eq!(server.sidebar_count(&env), 1, "exactly one tagged sidebar");
}

#[test]
fn second_invocation_focuses_existing_sidebar_does_not_split() {
    if !tmux_available() {
        eprintln!("tmux not on PATH; skipping");
        return;
    }
    let env = TestEnv::new();
    let server = PrivateTmux::spawn(&env);

    let _ = server.run_aw(&env, &["dash", "sidebar"]);
    assert_eq!(server.pane_count(&env), 2);
    assert_eq!(server.sidebar_count(&env), 1);

    // Run again — should NOT create a second sidebar.
    let out = server.run_aw(&env, &["dash", "sidebar"]);
    assert!(out.status.success(), "second invocation: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(server.pane_count(&env), 2, "should not have split again");
    assert_eq!(server.sidebar_count(&env), 1, "still one tagged sidebar");
}

#[test]
fn sidebar_render_includes_keybinding_hints() {
    let env = TestEnv::new();
    // Drive _sidebar-loop briefly and capture its first paint.
    let path = format!(
        "{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
        env.fake_bin.display()
    );
    let mut child = Command::new(Bin::Rust.path())
        .args(["_sidebar-loop"])
        .current_dir(&env.home)
        .env_clear()
        .env("HOME", &env.home)
        .env("PATH", path)
        .env("AW_INSTALL_DIR", &env.install_dir)
        .env("AW_WORKSPACES_DIR", &env.workspaces_dir)
        .env("AW_BIN_DIR", &env.bin_dir)
        .env("AW_CONFIG_FILE", &env.config_path)
        .env("AW_STATE_DIR", &env.state_dir)
        .env("TMUX_TMPDIR", env.tmp.path())
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(400));
    let _ = child.kill();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    // Either at least one paint happened (look anywhere in stdout) or the
    // process was killed mid-write and produced nothing — assert the
    // simpler "hints appear somewhere in any paint."
    assert!(
        stdout.contains("prefix+a") && stdout.contains("popup"),
        "missing prefix+a / popup hint in:\n{:?}",
        stdout
    );
    assert!(
        stdout.contains("prefix+N") && stdout.contains("next"),
        "missing prefix+N / next hint in:\n{:?}",
        stdout
    );
}
