//! Parity for auxiliary commands: edit-config, edit-base, open-home, sync.
//!
//! These shell out to editors / git. With `EDITOR=true` in the sandbox env,
//! the editor commands no-op cleanly and we can assert exit status only.
//! Sync is exercised end-to-end against the fake remotes the harness creates.

mod common;

use common::{capture, fixtures, Bin, TestEnv};

// ---- edit-config ----

fn edit_config(bin: Bin) {
    let env = TestEnv::new();
    let out = env.run(bin, &["edit-config"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0, "edit-config: {}", cap.stderr);
}

#[test]
fn bash_edit_config() { edit_config(Bin::Bash); }
#[test]
fn rust_edit_config() { edit_config(Bin::Rust); }

// ---- edit-base (existing) ----

fn edit_base_existing(bin: Bin) {
    let env = TestEnv::new();
    let init_cap = capture(&env, &env.run(bin, &["init"]));
    assert_eq!(init_cap.exit, 0);
    let out = env.run(bin, &["edit-base", "default"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0, "edit-base: {}", cap.stderr);
}

#[test]
fn bash_edit_base_existing() { edit_base_existing(Bin::Bash); }
#[test]
fn rust_edit_base_existing() { edit_base_existing(Bin::Rust); }

// ---- edit-base (missing) ----

fn edit_base_missing(bin: Bin) {
    let env = TestEnv::new();
    let out = env.run(bin, &["edit-base", "ghost"]);
    let cap = capture(&env, &out);
    assert_ne!(cap.exit, 0, "edit-base ghost should fail");
}

#[test]
fn bash_edit_base_missing() { edit_base_missing(Bin::Bash); }
#[test]
fn rust_edit_base_missing() { edit_base_missing(Bin::Rust); }

// ---- open-home ----

fn open_home(bin: Bin) {
    let env = TestEnv::new();
    let out = env.run(bin, &["open-home"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0, "open-home: {}", cap.stderr);
}

#[test]
fn bash_open_home() { open_home(Bin::Bash); }
#[test]
fn rust_open_home() { open_home(Bin::Rust); }

// ---- sync (current workspace) ----
//
// Setup: init + create a workspace with one fake remote. cd into the
// workspace and run sync. The remote and local should be in sync, so we
// expect "already up to date" + exit 0.

fn sync_up_to_date(bin: Bin) {
    let env = TestEnv::new().with_fake_remote("repo1");
    let cfg = fixtures::config_with_one_remote(&env, "repo1");
    let env = env.with_config(&cfg);
    let init_cap = capture(&env, &env.run(bin, &["init"]));
    assert_eq!(init_cap.exit, 0);
    let create_cap = capture(&env, &env.run(bin, &["create", "feat-sync"]));
    assert_eq!(create_cap.exit, 0);

    // Run sync with AGENT_WORKSPACE set so the detection logic finds the workspace.
    let workspace = env.workspaces_dir.join("feat-sync");
    let path = format!(
        "{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
        env.fake_bin.display()
    );
    let out = std::process::Command::new(bin.path())
        .args(["sync"])
        .current_dir(&env.home)
        .env_clear()
        .env("HOME", &env.home)
        .env("PATH", path)
        .env("AW_INSTALL_DIR", &env.install_dir)
        .env("AW_WORKSPACES_DIR", &env.workspaces_dir)
        .env("AW_BIN_DIR", &env.bin_dir)
        .env("AW_CONFIG_FILE", &env.config_path)
        .env("AGENT_WORKSPACE", &workspace)
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .env("TZ", "UTC")
        .env("EDITOR", "true")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(out.status.code(), Some(0), "sync: {}", stdout);
    assert!(
        stdout.contains("repo1") && stdout.contains("up to date"),
        "expected up-to-date sync, got: {}",
        stdout
    );
}

#[test]
fn bash_sync_up_to_date() { sync_up_to_date(Bin::Bash); }
#[test]
fn rust_sync_up_to_date() { sync_up_to_date(Bin::Rust); }

// ---- sync (outside any workspace) ----

fn sync_outside_workspace(bin: Bin) {
    let env = TestEnv::new();
    let out = env.run(bin, &["sync"]);
    let cap = capture(&env, &out);
    assert_ne!(cap.exit, 0, "sync outside should fail");
}

#[test]
fn bash_sync_outside_workspace() { sync_outside_workspace(Bin::Bash); }
#[test]
fn rust_sync_outside_workspace() { sync_outside_workspace(Bin::Rust); }
