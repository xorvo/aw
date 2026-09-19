//! `aw create <name>` materializes a workspace from a base.
//!
//! Snapshots the workspace tree (relative paths + sha256). `CLAUDE.md` and
//! `AGENTS.md` are symlinked from the base when present and everything else
//! is copied; the manifest captures both link targets and file hashes.

mod common;

use common::{capture, fixtures, tree_manifest, TestEnv};

fn init_then_create(env: &TestEnv, name: &str, base: Option<&str>) {
    let init_out = match base {
        Some(b) => env.run(&["init", b]),
        None => env.run(&["init"]),
    };
    let init_cap = capture(env, &init_out);
    assert_eq!(init_cap.exit, 0, "init failed: {}", init_cap.stderr);

    let mut args = vec!["create", name];
    if let Some(b) = base {
        args.push("--base");
        args.push(b);
    }
    let cap = capture(env, &env.run(&args));
    assert_eq!(cap.exit, 0, "create failed: {}", cap.stderr);
}

#[test]
fn create_from_default_one_repo() {
    let env = TestEnv::new().with_fake_remote("repo1");
    let cfg = fixtures::config_with_one_remote(&env, "repo1");
    let env = env.with_config(&cfg);

    init_then_create(&env, "feat-x", None);

    let ws = env.workspaces_dir.join("feat-x");
    assert!(ws.exists());
    assert_eq!(
        std::fs::read_to_string(ws.join(".agent-workspace/name")).unwrap().trim(),
        "feat-x"
    );
    assert_eq!(
        std::fs::read_to_string(ws.join(".agent-workspace/base")).unwrap().trim(),
        "default"
    );
    assert!(ws.join("repo1").exists(), "expected workspace to contain repo1/");

    insta::assert_yaml_snapshot!("create_default_one_repo_tree", tree_manifest(&ws));
}

#[test]
fn create_from_named_base() {
    let env = TestEnv::new()
        .with_fake_remote("default-repo")
        .with_fake_remote("dev-repo");
    let cfg = fixtures::config_with_two_bases(&env, "default-repo", "dev-repo");
    let env = env.with_config(&cfg);

    init_then_create(&env, "feat-y", Some("dev"));

    let ws = env.workspaces_dir.join("feat-y");
    assert_eq!(
        std::fs::read_to_string(ws.join(".agent-workspace/base")).unwrap().trim(),
        "dev"
    );
    assert!(ws.join("dev-repo").exists());

    insta::assert_yaml_snapshot!("create_named_base_tree", tree_manifest(&ws));
}

#[test]
fn create_with_local_file_rename() {
    let env = TestEnv::new()
        .with_fake_remote("repo1")
        .with_local_file("notes/INFO.md", "# notes\n");
    let cfg = fixtures::config_with_remote_and_local(&env, "repo1", "notes", Some("docs"));
    let env = env.with_config(&cfg);

    init_then_create(&env, "feat-z", None);

    let ws = env.workspaces_dir.join("feat-z");
    // Source dir was `notes/`, renamed to `docs/`.
    assert!(ws.join("docs").exists(), "expected docs/ (renamed from notes)");
    assert!(ws.join("docs/INFO.md").exists());

    insta::assert_yaml_snapshot!("create_local_rename_tree", tree_manifest(&ws));
}
