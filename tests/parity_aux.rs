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

/// Run `aw sync` with `AGENT_WORKSPACE` pointing at the workspace so the
/// detection logic finds it. Returns (stdout, exit code).
fn run_sync(env: &TestEnv, bin: Bin, workspace: &std::path::Path) -> (String, i32) {
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
        .env("AGENT_WORKSPACE", workspace)
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .env("TZ", "UTC")
        .env("EDITOR", "true")
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

fn sync_up_to_date(bin: Bin) {
    let env = TestEnv::new().with_fake_remote("repo1");
    let cfg = fixtures::config_with_one_remote(&env, "repo1");
    let env = env.with_config(&cfg);
    let init_cap = capture(&env, &env.run(bin, &["init"]));
    assert_eq!(init_cap.exit, 0);
    let create_cap = capture(&env, &env.run(bin, &["create", "feat-sync"]));
    assert_eq!(create_cap.exit, 0);

    let workspace = env.workspaces_dir.join("feat-sync");
    let (stdout, exit) = run_sync(&env, bin, &workspace);
    assert_eq!(exit, 0, "sync: {}", stdout);
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

// ---- sync (fast-forward behavior, rust-only) ----
//
// Intentional divergence from bash: when the default branch is checked out,
// sync fast-forwards via `merge --ff-only` so the index/worktree move with
// the ref. Bash's unconditional `update-ref` left a stale index (upstream
// commits appeared inverted as staged changes). The not-checked-out path
// still uses `update-ref` and never touches the worktree.

/// Run git in `dir` with a fixed identity, asserting success.
fn git_in(dir: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .status()
        .expect("git");
    assert!(status.success(), "git {:?} failed in {}", args, dir.display());
}

fn git_out(dir: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git");
    assert!(out.status.success(), "git {:?} failed in {}", args, dir.display());
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Push one commit adding `newfile.txt` to the fake remote `repo1`.
fn advance_remote(env: &TestEnv) {
    let scratch = env.tmp.path().join("__advance_repo1");
    let bare = env.remotes_dir.join("repo1.git");
    let status = std::process::Command::new("git")
        .args(["clone", "--quiet"])
        .arg(&bare)
        .arg(&scratch)
        .status()
        .expect("git clone");
    assert!(status.success());
    std::fs::write(scratch.join("newfile.txt"), "upstream\n").unwrap();
    git_in(&scratch, &["add", "."]);
    git_in(&scratch, &["commit", "--quiet", "-m", "upstream change"]);
    git_in(&scratch, &["push", "--quiet", "origin", "main"]);
    std::fs::remove_dir_all(&scratch).unwrap();
}

/// Init + create a workspace named `feat-sync` from one fake remote and
/// return (env, workspace_dir, repo_dir).
fn sync_fixture() -> (TestEnv, std::path::PathBuf, std::path::PathBuf) {
    let env = TestEnv::new().with_fake_remote("repo1");
    let cfg = fixtures::config_with_one_remote(&env, "repo1");
    let env = env.with_config(&cfg);
    assert_eq!(capture(&env, &env.run(Bin::Rust, &["init"])).exit, 0);
    assert_eq!(capture(&env, &env.run(Bin::Rust, &["create", "feat-sync"])).exit, 0);
    let workspace = env.workspaces_dir.join("feat-sync");
    let repo = workspace.join("repo1");
    (env, workspace, repo)
}

// Default branch checked out (the state a fresh clone leaves): the
// fast-forward must bring the index and worktree along — `git status` stays
// clean and the new file appears.
#[test]
fn rust_sync_ff_checked_out_updates_worktree() {
    let (env, workspace, repo) = sync_fixture();
    advance_remote(&env);

    let (stdout, exit) = run_sync(&env, Bin::Rust, &workspace);
    assert_eq!(exit, 0, "sync: {}", stdout);
    assert!(
        stdout.contains("fast-forwarded 1 commit(s)"),
        "expected fast-forward, got: {}",
        stdout
    );
    assert_eq!(
        git_out(&repo, &["status", "--porcelain"]),
        "",
        "worktree should be clean after a checked-out fast-forward"
    );
    assert!(repo.join("newfile.txt").is_file(), "worktree should have the upstream file");
}

// Checked out + a local edit that the fast-forward would overwrite: git
// refuses, sync reports it as skipped, and the local edit survives.
#[test]
fn rust_sync_ff_checked_out_blocked_by_local_changes() {
    let (env, workspace, repo) = sync_fixture();

    // Remote and local both touch newfile.txt.
    advance_remote(&env);
    std::fs::write(repo.join("newfile.txt"), "local edit\n").unwrap();

    let (stdout, exit) = run_sync(&env, Bin::Rust, &workspace);
    assert_eq!(exit, 0, "sync: {}", stdout);
    assert!(
        stdout.contains("local changes block fast-forward"),
        "expected worktree-blocked skip, got: {}",
        stdout
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("newfile.txt")).unwrap(),
        "local edit\n",
        "local edit must survive a blocked sync"
    );
}

// Default branch NOT checked out: the ref moves but the feature-branch
// worktree is untouched (no merge, no new file).
#[test]
fn rust_sync_ff_not_checked_out_leaves_worktree_alone() {
    let (env, workspace, repo) = sync_fixture();
    git_in(&repo, &["checkout", "--quiet", "-b", "feature"]);
    advance_remote(&env);

    let (stdout, exit) = run_sync(&env, Bin::Rust, &workspace);
    assert_eq!(exit, 0, "sync: {}", stdout);
    assert!(
        stdout.contains("fast-forwarded 1 commit(s)"),
        "expected fast-forward, got: {}",
        stdout
    );
    assert_eq!(
        git_out(&repo, &["rev-parse", "refs/heads/main"]),
        git_out(&repo, &["rev-parse", "refs/remotes/origin/main"]),
        "main ref should be fast-forwarded to the remote tip"
    );
    assert!(
        !repo.join("newfile.txt").exists(),
        "feature-branch worktree must not be touched"
    );
    assert_eq!(git_out(&repo, &["status", "--porcelain"]), "");
}
