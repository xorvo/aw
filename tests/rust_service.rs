//! Integration tests for `aw install service` (the launchd login service).
//!
//! `AW_SERVICE_SKIP_LAUNCHCTL=1` writes the plist but skips the actual
//! `launchctl` load, so these run anywhere without touching the host's
//! real launchd or opening a network listener. macOS-only: the command
//! bails on other platforms by design.

#![cfg(target_os = "macos")]

mod common;

use std::process::Command;

use tempfile::TempDir;

fn aw() -> Command {
    let mut c = Command::new(assert_cmd::cargo::cargo_bin("aw"));
    c.env("AW_SERVICE_SKIP_LAUNCHCTL", "1");
    c
}

/// A sandboxed HOME + state dir so the plist and log land in the tempdir,
/// never the developer's real `~/Library/LaunchAgents`.
fn sandbox() -> (TempDir, TempDir) {
    (TempDir::new().unwrap(), TempDir::new().unwrap())
}

#[test]
fn install_writes_plist_then_uninstall_removes_it() {
    let (home, state) = sandbox();
    let plist = home.path().join("Library/LaunchAgents/com.agent-workspaces.serve.plist");

    let out = aw()
        .args(["install", "service"])
        .env("HOME", home.path())
        .env("AW_STATE_DIR", state.path())
        .output()
        .expect("run install service");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(plist.is_file(), "plist should exist at {}", plist.display());

    let body = std::fs::read_to_string(&plist).unwrap();
    assert!(body.contains("<key>Label</key>"));
    assert!(body.contains("com.agent-workspaces.serve"));
    assert!(body.contains("<string>serve</string>"), "ProgramArguments runs `serve`");
    assert!(body.contains("<key>RunAtLoad</key>"));
    assert!(body.contains("<key>KeepAlive</key>"));
    // Log path resolves under the sandboxed state dir.
    assert!(
        body.contains(&state.path().join("serve.log").to_string_lossy().to_string()),
        "log path should point into AW_STATE_DIR:\n{}",
        body
    );
    // PATH is baked so tmux is findable from launchd's minimal env.
    assert!(body.contains("<key>PATH</key>"));
    assert!(body.contains("/opt/homebrew/bin") || body.contains("/usr/local/bin"));

    // Uninstall removes the plist.
    let out = aw()
        .args(["install", "service", "--uninstall"])
        .env("HOME", home.path())
        .env("AW_STATE_DIR", state.path())
        .output()
        .expect("run uninstall");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(!plist.exists(), "plist should be gone after --uninstall");
}

#[test]
fn install_honors_host_and_port_flags() {
    let (home, state) = sandbox();
    let plist = home.path().join("Library/LaunchAgents/com.agent-workspaces.serve.plist");

    let out = aw()
        .args(["install", "service", "--host", "127.0.0.1", "--port", "9999"])
        .env("HOME", home.path())
        .env("AW_STATE_DIR", state.path())
        .output()
        .expect("run install service");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let body = std::fs::read_to_string(&plist).unwrap();
    assert!(body.contains("<string>--host</string>"));
    assert!(body.contains("<string>127.0.0.1</string>"));
    assert!(body.contains("<string>--port</string>"));
    assert!(body.contains("<string>9999</string>"));
}

#[test]
fn install_is_idempotent() {
    let (home, state) = sandbox();
    let plist = home.path().join("Library/LaunchAgents/com.agent-workspaces.serve.plist");

    for _ in 0..2 {
        let out = aw()
            .args(["install", "service"])
            .env("HOME", home.path())
            .env("AW_STATE_DIR", state.path())
            .output()
            .expect("run install service");
        assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    }
    assert!(plist.is_file(), "second run leaves a valid plist");
}

#[test]
fn uninstall_with_no_service_is_a_clean_noop() {
    let (home, state) = sandbox();
    let out = aw()
        .args(["install", "service", "--uninstall"])
        .env("HOME", home.path())
        .env("AW_STATE_DIR", state.path())
        .output()
        .expect("run uninstall");
    assert!(out.status.success(), "no-op uninstall should still exit 0");
    assert!(
        String::from_utf8_lossy(&out.stdout).to_lowercase().contains("no aw serve service"),
        "should say there's nothing to remove"
    );
}
