//! Smoke test: confirm the harness can run `aw` against a sandbox and
//! capture normalized output. Intentionally minimal — real scenarios live in
//! the other files under `tests/`.

mod common;

use common::{capture, TestEnv};

#[test]
fn help_runs() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(&["--help"]));
    assert_eq!(cap.exit, 0, "--help failed:\n{}", cap.stderr);
    assert!(
        cap.stdout.contains("Manage isolated workspaces"),
        "unexpected help text:\n{}",
        cap.stdout
    );
}

#[test]
fn sandbox_paths_are_isolated() {
    let env = TestEnv::new();
    // Sandbox dirs all live under tmp.
    for p in [
        &env.home,
        &env.install_dir,
        &env.workspaces_dir,
        &env.bin_dir,
        &env.config_path,
    ] {
        assert!(
            p.starts_with(env.tmp.path()),
            "{} not inside tmp {}",
            p.display(),
            env.tmp.path().display()
        );
    }
}

#[test]
fn fake_remote_is_clonable() {
    let env = TestEnv::new().with_fake_remote("repo1");
    let url = env.remote_url("repo1");
    let dest = env.tmp.path().join("clonecheck");
    let status = std::process::Command::new("git")
        .args(["clone", "--quiet", &url, dest.to_str().unwrap()])
        .status()
        .expect("git");
    assert!(status.success(), "fake remote failed to clone");
    assert!(dest.join("README.md").exists());
}
