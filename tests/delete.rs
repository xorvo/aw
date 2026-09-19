//! `aw delete <name>` prompts for confirmation, then removes the workspace
//! tree on `y`. Cancels on anything else.

mod common;

use common::{capture, fixtures, TestEnv};

fn setup_with_one_workspace() -> TestEnv {
    let env = TestEnv::new().with_fake_remote("repo1");
    let cfg = fixtures::config_with_one_remote(&env, "repo1");
    let env = env.with_config(&cfg);
    assert_eq!(capture(&env, &env.run(&["init"])).exit, 0);
    assert_eq!(capture(&env, &env.run(&["create", "feat-doomed"])).exit, 0);
    env
}

#[test]
fn delete_confirm_yes() {
    let env = setup_with_one_workspace();
    let ws = env.workspaces_dir.join("feat-doomed");
    assert!(ws.exists(), "precondition: workspace exists");

    let cap = capture(&env, &env.run_with_stdin(&["delete", "feat-doomed"], "y\n"));
    assert_eq!(cap.exit, 0, "delete: {}", cap.stderr);
    assert!(!ws.exists(), "workspace should be gone after y-confirm");
}

#[test]
fn delete_confirm_no() {
    let env = setup_with_one_workspace();
    let ws = env.workspaces_dir.join("feat-doomed");

    let cap = capture(&env, &env.run_with_stdin(&["delete", "feat-doomed"], "n\n"));
    // Cancelling is not a failure: exit 0 with a "Cancelled" message.
    assert_eq!(cap.exit, 0, "delete-cancel: {}", cap.stderr);
    assert!(ws.exists(), "workspace should still exist after n-confirm");
}

#[test]
fn delete_alias_rm() {
    let env = setup_with_one_workspace();
    let cap = capture(&env, &env.run_with_stdin(&["rm", "feat-doomed"], "y\n"));
    assert_eq!(cap.exit, 0, "rm: {}", cap.stderr);
    assert!(!env.workspaces_dir.join("feat-doomed").exists());
}
