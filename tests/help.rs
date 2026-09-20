//! `-h` / `--help` print usage and exit 0, and the usage names every public
//! subcommand.

mod common;

use common::{capture, TestEnv};

fn run_help(arg: &str) -> (i32, String) {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(&[arg]));
    (cap.exit, cap.stdout)
}

#[test]
fn help() {
    let (exit, stdout) = run_help("--help");
    assert_eq!(exit, 0);
    insta::assert_snapshot!("help", stdout);
}

#[test]
fn help_dash_h() {
    let (exit, _) = run_help("-h");
    assert_eq!(exit, 0);
}

#[test]
fn help_mentions_all_public_subcommands() {
    let (_, help) = run_help("--help");
    for cmd in [
        "init", "create", "list", "start", "delete", "config", "edit-config",
        "edit-base", "sync", "reset", "open-home", "resurrect", "snapshot",
        "switch", "dash", "serve", "hook", "shell-init", "completions", "install",
        "self",
    ] {
        assert!(help.contains(cmd), "help missing '{}':\n{}", cmd, help);
    }
}
