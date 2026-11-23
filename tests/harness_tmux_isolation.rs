//! The suite must never reach a tmux server it did not start.
//!
//! It once did. Guards cleaned up with a bare `tmux kill-server` and set only
//! `TMUX_TMPDIR`. Run from inside a tmux pane, they inherited `$TMUX`, which
//! tmux honours over `TMUX_TMPDIR`, so they killed the developer's own server
//! along with every agent running in it.

mod common;

use std::process::Stdio;

#[test]
fn tmux_helper_drops_the_callers_session() {
    let cmd = common::tmux();
    let removed: Vec<_> = cmd
        .get_envs()
        .filter(|(_, v)| v.is_none())
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    assert!(removed.contains(&"TMUX".to_string()), "{removed:?}");
    assert!(removed.contains(&"TMUX_PANE".to_string()), "{removed:?}");
}

#[test]
fn sandbox_tmux_names_its_socket_explicitly() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cmd = common::sandbox_tmux_at(tmp.path());
    let args: Vec<_> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args.first().map(String::as_str), Some("-S"), "{args:?}");
    assert!(
        args[1].starts_with(&tmp.path().display().to_string()),
        "{args:?}"
    );
}

/// A test file that builds its own tmux `Command` bypasses both guards.
#[test]
fn no_test_spawns_tmux_except_through_the_helpers() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        if src.contains(concat!("Command::new(", "\"tmux\")")) {
            offenders.push(path.display().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "use common::tmux()/sandbox_tmux(): {offenders:?}"
    );
}

/// `aw` itself talks to the server `$TMUX` names (`aw serve` even sends keys),
/// so a test file that spawns it must either clear the environment or cut it
/// off from the host's tmux.
#[test]
fn every_aw_spawn_is_cut_off_from_the_host_tmux() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let spawns = src.matches(concat!("cargo_bin(", "\"aw\")")).count()
            + src.matches(concat!("Command::new(", "aw_bin())")).count()
            + src.matches(concat!("Command::new(", "common::aw_bin())")).count()
            + src.matches(concat!("let aw = ", "aw_bin();")).count();
        let guarded = src.matches("env_clear()").count()
            + src.matches(concat!("isolate_from_", "host_tmux(")).count();
        if spawns > guarded {
            offenders.push(format!("{} ({spawns} spawns, {guarded} guarded)", path.display()));
        }
    }
    assert!(
        offenders.is_empty(),
        "env_clear() or common::isolate_from_host_tmux: {offenders:?}"
    );
}

/// Even with `$TMUX` leaking into the command (pointed at a decoy that plays
/// the developer's server), a sandbox `kill-server` must leave the decoy up.
#[test]
fn sandbox_kill_server_spares_the_server_named_in_tmux_env() {
    if !common::tmux()
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        eprintln!("skipping: tmux unavailable");
        return;
    }
    // Short path: macOS caps socket paths at 104 bytes.
    let decoy = std::path::PathBuf::from(format!("/tmp/awdecoy-{}.sock", std::process::id()));
    let up = |sock: &std::path::Path| {
        common::tmux()
            .args(["-S", sock.to_str().unwrap(), "has-session"])
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };
    assert!(common::tmux()
        .args([
            "-S",
            decoy.to_str().unwrap(),
            "new-session",
            "-d",
            "-s",
            "decoy",
            "sleep 60"
        ])
        .status()
        .unwrap()
        .success());

    let sandbox = tempfile::TempDir::new().unwrap();
    assert!(common::sandbox_tmux_at(sandbox.path())
        .args(["new-session", "-d", "-s", "sandbox", "sleep 60"])
        .status()
        .unwrap()
        .success());
    let _ = common::sandbox_tmux_at(sandbox.path())
        .env("TMUX", format!("{},1,0", decoy.display()))
        .args(["kill-server"])
        .stderr(Stdio::null())
        .status();

    let survived = up(&decoy);
    let sandbox_gone = !up(&common::sandbox_tmux_socket(sandbox.path()));
    let _ = common::tmux()
        .args(["-S", decoy.to_str().unwrap(), "kill-server"])
        .status();
    let _ = std::fs::remove_file(&decoy);
    assert!(survived, "kill-server reached the server named in $TMUX");
    assert!(sandbox_gone, "kill-server missed the sandbox server");
}
