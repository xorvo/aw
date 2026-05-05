//! Parity: `aw list` enumerates workspaces, marking active tmux sessions
//! with a green dot.
//!
//! In tests we don't spin up real tmux sessions, so the green-dot path is
//! exercised separately (and gated by `#[ignore]` for environments without
//! tmux). The default path snapshots clean text output.

mod common;

use common::{capture, fixtures, Bin, TestEnv};

fn list_empty(bin: Bin) -> String {
    let env = TestEnv::new();
    let out = env.run(bin, &["list"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0, "list failed: {}", cap.stderr);
    cap.stdout
}

#[test]
fn bash_list_empty() {
    let out = list_empty(Bin::Bash);
    insta::assert_snapshot!("list_empty_bash", out);
}

#[test]
fn rust_list_empty() {
    let out = list_empty(Bin::Rust);
    insta::assert_snapshot!("list_empty_rust", out);
}

fn list_after_create(bin: Bin) -> String {
    let env = TestEnv::new().with_fake_remote("repo1");
    let cfg = fixtures::config_with_one_remote(&env, "repo1");
    let env = env.with_config(&cfg);

    let init_cap = capture(&env, &env.run(bin, &["init"]));
    assert_eq!(init_cap.exit, 0, "init: {}", init_cap.stderr);
    let create_cap = capture(&env, &env.run(bin, &["create", "feat-a"]));
    assert_eq!(create_cap.exit, 0, "create: {}", create_cap.stderr);

    let out = env.run(bin, &["list"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0, "list failed: {}", cap.stderr);

    // The created date is dynamic ("date" output) — strip the suffix to keep
    // snapshots stable. Bash format ends with `, created: <date>`; we cut
    // anything after `created: ` on each workspace line.
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
    cleaned
}

#[test]
fn bash_list_after_create() {
    let out = list_after_create(Bin::Bash);
    insta::assert_snapshot!("list_after_create_bash", out);
}

#[test]

fn rust_list_after_create() {
    let out = list_after_create(Bin::Rust);
    insta::assert_snapshot!("list_after_create_rust", out);
}

#[test]
fn list_alias_ls_works_in_bash() {
    let env = TestEnv::new();
    let out = env.run(Bin::Bash, &["ls"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0, "ls alias failed: {}", cap.stderr);
}
