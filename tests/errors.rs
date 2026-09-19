//! Error cases — exit codes only, not stdout shape.

mod common;

use common::{capture, TestEnv};

#[test]
fn init_missing_config() {
    let env = TestEnv::new();
    // Wipe the config file so init can't find it.
    std::fs::remove_file(&env.config_path).unwrap();
    let cap = capture(&env, &env.run(&["init"]));
    assert_ne!(cap.exit, 0, "init should fail when config is missing");
}

#[test]
fn init_unknown_base() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(&["init", "nonexistent"]));
    assert_ne!(cap.exit, 0, "init should fail when base is unknown");
}

#[test]
fn create_without_init() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(&["create", "no-base"]));
    assert_ne!(cap.exit, 0, "create should fail when base hasn't been init'd");
}

#[test]
fn create_already_exists() {
    let env = TestEnv::new();
    assert_eq!(capture(&env, &env.run(&["init"])).exit, 0);
    assert_eq!(capture(&env, &env.run(&["create", "dup"])).exit, 0);
    let second = capture(&env, &env.run(&["create", "dup"]));
    assert_ne!(second.exit, 0, "duplicate create should fail");
}

#[test]
fn delete_nonexistent() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run_with_stdin(&["delete", "ghost"], "y\n"));
    assert_ne!(cap.exit, 0, "delete of missing workspace should fail");
}

#[test]
fn unknown_subcommand() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(&["totally-not-a-command"]));
    assert_ne!(cap.exit, 0, "unknown subcommand should be a non-zero exit");
}
