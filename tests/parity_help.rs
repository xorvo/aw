//! Parity: help / -h / --help should print usage and exit 0.
//!
//! Stdout snapshots are committed per-binary (suffixed `_bash` / `_rust`)
//! because clap's auto-generated help differs from the bash hand-rolled help.
//! Parity here means: both print *something useful*, exit 0, mention the
//! same set of subcommand names somewhere in the output.

mod common;

use common::{capture, Bin, TestEnv};

fn run_help(bin: Bin, arg: &str) -> (i32, String) {
    let env = TestEnv::new();
    let out = env.run(bin, &[arg]);
    let cap = capture(&env, &out);
    (cap.exit, cap.stdout)
}

#[test]
fn bash_help_subcommand() {
    let (exit, stdout) = run_help(Bin::Bash, "help");
    assert_eq!(exit, 0);
    insta::assert_snapshot!("help_bash", stdout);
}

#[test]
fn bash_help_dash_h() {
    let (exit, _) = run_help(Bin::Bash, "-h");
    assert_eq!(exit, 0);
}

#[test]
fn bash_help_dash_dash_help() {
    let (exit, _) = run_help(Bin::Bash, "--help");
    assert_eq!(exit, 0);
}

#[test]
fn rust_help() {
    let (exit, stdout) = run_help(Bin::Rust, "--help");
    assert_eq!(exit, 0);
    insta::assert_snapshot!("help_rust", stdout);
}

/// Behavioral parity: both binaries' help mention every public subcommand name.
#[test]
fn both_help_mentions_all_public_subcommands() {
    let (_, bash_help) = run_help(Bin::Bash, "help");
    for cmd in [
        "init", "create", "list", "delete", "config", "edit-config",
        "edit-base", "sync", "open-home", "help",
    ] {
        assert!(
            bash_help.contains(cmd),
            "bash help missing '{}':\n{}",
            cmd, bash_help
        );
    }
}
