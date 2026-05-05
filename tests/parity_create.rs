//! Parity: `aw create <name>` materializes a workspace from a base.
//!
//! Snapshots the workspace tree (relative paths + sha256). The bash CLI
//! symlinks CLAUDE.md / AGENTS.md from the base when present and copies
//! everything else; the manifest captures both link targets and file hashes.

mod common;

use common::{capture, fixtures, tree_manifest, Bin, TestEnv};

fn init_then_create(env: &TestEnv, bin: Bin, name: &str, base: Option<&str>) {
    // init the base
    let init_out = match base {
        Some(b) => env.run(bin, &["init", b]),
        None => env.run(bin, &["init"]),
    };
    let init_cap = capture(env, &init_out);
    assert_eq!(init_cap.exit, 0, "init failed: {}", init_cap.stderr);

    // create the workspace
    let mut args = vec!["create", name];
    if let Some(b) = base {
        args.push("--base");
        args.push(b);
    }
    let out = env.run(bin, &args);
    let cap = capture(env, &out);
    assert_eq!(cap.exit, 0, "create failed: {}", cap.stderr);
}

fn create_from_default_one_repo(bin: Bin) {
    let env = TestEnv::new().with_fake_remote("repo1");
    let cfg = fixtures::config_with_one_remote(&env, "repo1");
    let env = env.with_config(&cfg);

    init_then_create(&env, bin, "feat-x", None);

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

    let manifest = tree_manifest(&ws);
    insta::with_settings!({snapshot_suffix => bin.label()}, {
        insta::assert_yaml_snapshot!("create_default_one_repo_tree", manifest);
    });
}

#[test]
fn bash_create_from_default_one_repo() {
    create_from_default_one_repo(Bin::Bash);
}

#[test]

fn rust_create_from_default_one_repo() {
    create_from_default_one_repo(Bin::Rust);
}

fn create_from_named_base(bin: Bin) {
    let env = TestEnv::new()
        .with_fake_remote("default-repo")
        .with_fake_remote("dev-repo");
    let cfg = fixtures::config_with_two_bases(&env, "default-repo", "dev-repo");
    let env = env.with_config(&cfg);

    init_then_create(&env, bin, "feat-y", Some("dev"));

    let ws = env.workspaces_dir.join("feat-y");
    assert_eq!(
        std::fs::read_to_string(ws.join(".agent-workspace/base")).unwrap().trim(),
        "dev"
    );
    assert!(ws.join("dev-repo").exists());

    let manifest = tree_manifest(&ws);
    insta::with_settings!({snapshot_suffix => bin.label()}, {
        insta::assert_yaml_snapshot!("create_named_base_tree", manifest);
    });
}

#[test]
fn bash_create_from_named_base() {
    create_from_named_base(Bin::Bash);
}

#[test]

fn rust_create_from_named_base() {
    create_from_named_base(Bin::Rust);
}

fn create_with_local_file_rename(bin: Bin) {
    let env = TestEnv::new()
        .with_fake_remote("repo1")
        .with_local_file("notes/INFO.md", "# notes\n");
    let cfg = fixtures::config_with_remote_and_local(&env, "repo1", "notes", Some("docs"));
    let env = env.with_config(&cfg);

    init_then_create(&env, bin, "feat-z", None);

    let ws = env.workspaces_dir.join("feat-z");
    // Source dir was `notes/`, renamed to `docs/`.
    assert!(ws.join("docs").exists(), "expected docs/ (renamed from notes)");
    assert!(ws.join("docs/INFO.md").exists());

    let manifest = tree_manifest(&ws);
    insta::with_settings!({snapshot_suffix => bin.label()}, {
        insta::assert_yaml_snapshot!("create_local_rename_tree", manifest);
    });
}

#[test]
fn bash_create_with_local_file_rename() {
    create_with_local_file_rename(Bin::Bash);
}

#[test]

fn rust_create_with_local_file_rename() {
    create_with_local_file_rename(Bin::Rust);
}
