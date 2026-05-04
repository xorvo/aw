//! Rust-only tests for shell-init / _shell-start / _detect-workspace /
//! completions. These have no bash counterpart, so we snapshot the Rust
//! output directly and run no parity comparison.

mod common;

use common::{capture, fixtures, Bin, TestEnv};

// ---- shell-init: snapshot the emitted hook for each shell ----

fn shell_init(shell: &str) -> String {
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["shell-init", shell]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0, "shell-init {}: {}", shell, cap.stderr);
    cap.stdout
}

#[test]
fn shell_init_zsh() {
    insta::assert_snapshot!("shell_init_zsh", shell_init("zsh"));
}

#[test]
fn shell_init_bash() {
    insta::assert_snapshot!("shell_init_bash", shell_init("bash"));
}

#[test]
fn shell_init_fish() {
    insta::assert_snapshot!("shell_init_fish", shell_init("fish"));
}

#[test]
fn completions_zsh_starts_with_compdef() {
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["completions", "zsh"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0);
    assert!(
        cap.stdout.contains("#compdef aw"),
        "expected zsh compdef header, got: {}",
        cap.stdout.lines().take(3).collect::<Vec<_>>().join(" / ")
    );
}

// ---- _shell-start: emit shell snippet for in-shell activation ----

fn setup_workspace_for_start() -> TestEnv {
    let env = TestEnv::new().with_fake_remote("repo1");
    let cfg = fixtures::config_with_one_remote(&env, "repo1");
    let env = env.with_config(&cfg);
    let init_cap = capture(&env, &env.run(Bin::Rust, &["init"]));
    assert_eq!(init_cap.exit, 0, "init: {}", init_cap.stderr);
    let create_cap = capture(&env, &env.run(Bin::Rust, &["create", "feat-a"]));
    assert_eq!(create_cap.exit, 0, "create: {}", create_cap.stderr);
    env
}

#[test]
fn shell_start_no_tmux_emits_cd_and_exports() {
    let env = setup_workspace_for_start();
    let out = env.run(Bin::Rust, &["_shell-start", "feat-a", "--no-tmux"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0, "_shell-start: {}", cap.stderr);
    // Path-normalized output: cd <WORKSPACES_DIR>/feat-a, exports, no source.
    insta::assert_snapshot!("shell_start_no_tmux", cap.stdout);
}

#[test]
fn shell_start_missing_workspace_exits_nonzero() {
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["_shell-start", "ghost", "--no-tmux"]);
    let cap = capture(&env, &out);
    assert_ne!(cap.exit, 0);
    assert!(
        cap.stdout.is_empty(),
        "stdout should be empty so a wrapper eval doesn't run garbage: {:?}",
        cap.stdout
    );
}

#[test]
fn shell_start_with_global_hook_sources_it() {
    let env = setup_workspace_for_start();
    // Drop a global hook.
    let global = env.install_dir.join("hooks.d");
    std::fs::create_dir_all(&global).unwrap();
    std::fs::write(global.join("zz-test.sh"), "echo hello\n").unwrap();

    let out = env.run(Bin::Rust, &["_shell-start", "feat-a", "--no-tmux"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0);
    assert!(
        cap.stdout.contains("source") && cap.stdout.contains("zz-test.sh"),
        "expected sourced hook in: {}",
        cap.stdout
    );
}

// ---- _detect-workspace ----

#[test]
fn detect_workspace_finds_root_from_subdir() {
    let env = setup_workspace_for_start();
    let workspace = env.workspaces_dir.join("feat-a");
    let sub = workspace.join("repo1");
    let out = env.run(Bin::Rust, &["_detect-workspace", sub.to_str().unwrap()]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0);
    assert!(
        cap.stdout.trim().ends_with("feat-a"),
        "expected workspace path in stdout: {:?}",
        cap.stdout
    );
}

#[test]
fn detect_workspace_outside_workspaces_dir_emits_nothing() {
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["_detect-workspace", env.tmp.path().to_str().unwrap()]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0);
    assert_eq!(cap.stdout.trim(), "");
}
