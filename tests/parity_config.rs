//! Parity: `aw config` shows the config file path.

mod common;

use common::{capture, Bin, TestEnv};

fn run_config(bin: Bin) -> (i32, String, String) {
    let env = TestEnv::new();
    let out = env.run(bin, &["config"]);
    let cap = capture(&env, &out);
    (cap.exit, cap.stdout, cap.stderr)
}

#[test]
fn bash_config_shows_path() {
    let (exit, stdout, _) = run_config(Bin::Bash);
    assert_eq!(exit, 0);
    insta::assert_snapshot!("config_bash", stdout);
}

#[test]
fn rust_config_shows_path() {
    let (exit, stdout, _) = run_config(Bin::Rust);
    assert_eq!(exit, 0);
    insta::assert_snapshot!("config_rust", stdout);
}

#[test]
fn config_output_references_config_path_marker() {
    let (_, stdout, _) = run_config(Bin::Bash);
    assert!(
        stdout.contains("<CONFIG>") || stdout.contains("<INSTALL_DIR>"),
        "config command output didn't reference the config path: {}",
        stdout
    );
}
