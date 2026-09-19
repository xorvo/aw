//! `aw init` clones repos as bare mirrors into the base workspace.
//!
//! We snapshot the resulting tree (relative paths + sha256) under
//! `<install_dir>/base/<base_name>/`. Stdout is checked loosely (key markers)
//! because it includes timing/parallel ordering that isn't behaviorally
//! meaningful.

mod common;

use common::{capture, fixtures, tree_manifest, TestEnv};

fn run_init(env: &TestEnv, base: Option<&str>) -> common::CapturedOutput {
    let mut args = vec!["init"];
    if let Some(b) = base {
        args.push(b);
    }
    capture(env, &env.run(&args))
}

// ---- empty config (no repos, no local files) ----

#[test]
fn init_empty_default() {
    let env = TestEnv::new();
    let cap = run_init(&env, None);
    assert_eq!(cap.exit, 0, "init failed: {}", cap.stderr);
    let manifest = tree_manifest(&env.install_dir.join("base/default"));
    insta::assert_yaml_snapshot!("init_empty_tree", manifest);
}

// ---- one remote ----

#[test]
fn init_with_one_remote() {
    let env = TestEnv::new().with_fake_remote("repo1");
    let cfg = fixtures::config_with_one_remote(&env, "repo1");
    let env = env.with_config(&cfg);

    let cap = run_init(&env, None);
    assert_eq!(cap.exit, 0, "init failed: {}", cap.stderr);
    assert!(
        cap.stdout.contains("repo1"),
        "stdout should mention repo1: {}",
        cap.stdout
    );

    let base_root = env.install_dir.join("base/default");
    let cache_dir = base_root.join(".agent-workspace/repo-cache/repo1.git");
    assert!(
        cache_dir.exists(),
        "expected bare mirror at {}",
        cache_dir.display()
    );

    insta::assert_yaml_snapshot!("init_one_remote_tree", tree_manifest(&base_root));
}

// ---- two remotes (parallel clone — manifest must be order-independent) ----

#[test]
fn init_with_two_remotes() {
    let env = TestEnv::new()
        .with_fake_remote("alpha")
        .with_fake_remote("beta");
    let cfg = fixtures::config_with_two_remotes(&env, "alpha", "beta");
    let env = env.with_config(&cfg);

    let cap = run_init(&env, None);
    assert_eq!(cap.exit, 0, "init failed: {}", cap.stderr);

    let base_root = env.install_dir.join("base/default");
    for repo in ["alpha", "beta"] {
        let cache_dir = base_root.join(format!(".agent-workspace/repo-cache/{}.git", repo));
        assert!(cache_dir.exists(), "missing mirror: {}", cache_dir.display());
    }

    insta::assert_yaml_snapshot!("init_two_remotes_tree", tree_manifest(&base_root));
}

// ---- named base (non-"default") ----

#[test]
fn init_with_named_base() {
    let env = TestEnv::new()
        .with_fake_remote("default-repo")
        .with_fake_remote("dev-repo");
    let cfg = fixtures::config_with_two_bases(&env, "default-repo", "dev-repo");
    let env = env.with_config(&cfg);

    let cap = run_init(&env, Some("dev"));
    assert_eq!(cap.exit, 0, "init failed: {}", cap.stderr);
    assert!(
        env.install_dir.join("base/dev").exists(),
        "expected base/dev to exist"
    );

    insta::assert_yaml_snapshot!("init_named_base_tree", tree_manifest(&env.install_dir.join("base/dev")));
}
