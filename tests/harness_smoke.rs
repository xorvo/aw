//! Smoke test: confirm the harness can run both binaries against a sandbox
//! and capture normalized output. This is intentionally minimal — real
//! parity scenarios live in `tests/parity_*.rs`.

mod common;

use common::{capture, Bin, TestEnv};

#[test]
fn bash_help_runs() {
    let env = TestEnv::new();
    let out = env.run(Bin::Bash, &["help"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0, "bash help failed:\n{}", cap.stderr);
    assert!(
        cap.stdout.contains("Manage isolated workspaces"),
        "unexpected help text:\n{}",
        cap.stdout
    );
}

#[test]
fn rust_help_runs() {
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["--help"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0, "rust --help failed:\n{}", cap.stderr);
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
