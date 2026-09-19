//! `aw list` enumerates workspaces, marking active tmux sessions with a
//! green dot. The sandbox has no tmux server, so these pin the plain-bullet
//! output.

mod common;

use common::{capture, fixtures, TestEnv};

#[test]
fn list_empty() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(&["list"]));
    assert_eq!(cap.exit, 0, "list failed: {}", cap.stderr);
    insta::assert_snapshot!("list_empty", cap.stdout);
}

#[test]
fn list_after_create() {
    let env = TestEnv::new().with_fake_remote("repo1");
    let cfg = fixtures::config_with_one_remote(&env, "repo1");
    let env = env.with_config(&cfg);

    let init_cap = capture(&env, &env.run(&["init"]));
    assert_eq!(init_cap.exit, 0, "init: {}", init_cap.stderr);
    let create_cap = capture(&env, &env.run(&["create", "feat-a"]));
    assert_eq!(create_cap.exit, 0, "create: {}", create_cap.stderr);

    let cap = capture(&env, &env.run(&["list"]));
    assert_eq!(cap.exit, 0, "list failed: {}", cap.stderr);

    // The created date is dynamic — cut everything after `created: ` on each
    // workspace line to keep the snapshot stable.
    let cleaned = cap.stdout
        .lines()
        .map(|line| {
            if let Some(idx) = line.find("created: ") {
                let prefix = &line[..idx + "created: ".len()];
                format!("{}<TIMESTAMP>)", prefix)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    insta::assert_snapshot!("list_after_create", cleaned);
}

#[test]
fn list_alias_ls() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(&["ls"]));
    assert_eq!(cap.exit, 0, "ls alias failed: {}", cap.stderr);
}
