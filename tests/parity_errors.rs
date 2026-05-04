//! Parity: error cases — exit codes and behavioral parity (not stdout shape).

mod common;

use common::{capture, Bin, TestEnv};

fn missing_config_init(bin: Bin) {
    let env = TestEnv::new();
    // Wipe the config file so init can't find it.
    std::fs::remove_file(&env.config_path).unwrap();

    let out = env.run(bin, &["init"]);
    let cap = capture(&env, &out);
    assert_ne!(cap.exit, 0, "init should fail when config is missing");
}

#[test]
fn bash_init_missing_config() {
    missing_config_init(Bin::Bash);
}

#[test]

fn rust_init_missing_config() {
    missing_config_init(Bin::Rust);
}

fn unknown_base_init(bin: Bin) {
    let env = TestEnv::new();
    let out = env.run(bin, &["init", "nonexistent"]);
    let cap = capture(&env, &out);
    assert_ne!(cap.exit, 0, "init should fail when base is unknown");
}

#[test]
fn bash_init_unknown_base() {
    unknown_base_init(Bin::Bash);
}

#[test]

fn rust_init_unknown_base() {
    unknown_base_init(Bin::Rust);
}

fn create_without_init(bin: Bin) {
    let env = TestEnv::new();
    let out = env.run(bin, &["create", "no-base"]);
    let cap = capture(&env, &out);
    assert_ne!(cap.exit, 0, "create should fail when base hasn't been init'd");
}

#[test]
fn bash_create_without_init() {
    create_without_init(Bin::Bash);
}

#[test]

fn rust_create_without_init() {
    create_without_init(Bin::Rust);
}

fn create_already_exists(bin: Bin) {
    let env = TestEnv::new();
    let init_cap = capture(&env, &env.run(bin, &["init"]));
    assert_eq!(init_cap.exit, 0);
    let first = capture(&env, &env.run(bin, &["create", "dup"]));
    assert_eq!(first.exit, 0);
    let second = capture(&env, &env.run(bin, &["create", "dup"]));
    assert_ne!(second.exit, 0, "duplicate create should fail");
}

#[test]
fn bash_create_already_exists() {
    create_already_exists(Bin::Bash);
}

#[test]

fn rust_create_already_exists() {
    create_already_exists(Bin::Rust);
}

fn delete_nonexistent(bin: Bin) {
    let env = TestEnv::new();
    let out = env.run_with_stdin(bin, &["delete", "ghost"], "y\n");
    let cap = capture(&env, &out);
    assert_ne!(cap.exit, 0, "delete of missing workspace should fail");
}

#[test]
fn bash_delete_nonexistent() {
    delete_nonexistent(Bin::Bash);
}

#[test]

fn rust_delete_nonexistent() {
    delete_nonexistent(Bin::Rust);
}

fn unknown_subcommand(bin: Bin) {
    let env = TestEnv::new();
    let out = env.run(bin, &["totally-not-a-command"]);
    let cap = capture(&env, &out);
    assert_ne!(cap.exit, 0, "unknown subcommand should be a non-zero exit");
}

#[test]
fn bash_unknown_subcommand() {
    unknown_subcommand(Bin::Bash);
}

#[test]

fn rust_unknown_subcommand() {
    unknown_subcommand(Bin::Rust);
}
