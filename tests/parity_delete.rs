//! Parity: `aw delete <name>` prompts for confirmation, then removes the
//! workspace tree on `y`. Cancels on anything else.

mod common;

use common::{capture, fixtures, Bin, TestEnv};

fn setup_with_one_workspace(bin: Bin) -> TestEnv {
    let env = TestEnv::new().with_fake_remote("repo1");
    let cfg = fixtures::config_with_one_remote(&env, "repo1");
    let env = env.with_config(&cfg);
    let init_cap = capture(&env, &env.run(bin, &["init"]));
    assert_eq!(init_cap.exit, 0);
    let create_cap = capture(&env, &env.run(bin, &["create", "feat-doomed"]));
    assert_eq!(create_cap.exit, 0);
    env
}

fn delete_confirm_yes(bin: Bin) {
    let env = setup_with_one_workspace(bin);
    let ws = env.workspaces_dir.join("feat-doomed");
    assert!(ws.exists(), "precondition: workspace exists");

    let out = env.run_with_stdin(bin, &["delete", "feat-doomed"], "y\n");
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0, "delete: {}", cap.stderr);
    assert!(!ws.exists(), "workspace should be gone after y-confirm");
}

#[test]
fn bash_delete_confirm_yes() {
    delete_confirm_yes(Bin::Bash);
}

#[test]

fn rust_delete_confirm_yes() {
    delete_confirm_yes(Bin::Rust);
}

fn delete_confirm_no(bin: Bin) {
    let env = setup_with_one_workspace(bin);
    let ws = env.workspaces_dir.join("feat-doomed");

    let out = env.run_with_stdin(bin, &["delete", "feat-doomed"], "n\n");
    let cap = capture(&env, &out);
    // Bash exits 0 with "Cancelled" message — non-failure.
    assert_eq!(cap.exit, 0, "delete-cancel: {}", cap.stderr);
    assert!(ws.exists(), "workspace should still exist after n-confirm");
}

#[test]
fn bash_delete_confirm_no() {
    delete_confirm_no(Bin::Bash);
}

#[test]

fn rust_delete_confirm_no() {
    delete_confirm_no(Bin::Rust);
}

fn delete_alias_rm(bin: Bin) {
    let env = setup_with_one_workspace(bin);
    let out = env.run_with_stdin(bin, &["rm", "feat-doomed"], "y\n");
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0, "rm: {}", cap.stderr);
    assert!(!env.workspaces_dir.join("feat-doomed").exists());
}

#[test]
fn bash_delete_alias_rm() {
    delete_alias_rm(Bin::Bash);
}

#[test]

fn rust_delete_alias_rm() {
    delete_alias_rm(Bin::Rust);
}
