//! `aw config` shows the config file path.

mod common;

use common::{capture, TestEnv};

#[test]
fn config_shows_path() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(&["config"]));
    assert_eq!(cap.exit, 0);
    assert!(
        cap.stdout.contains("<CONFIG>") || cap.stdout.contains("<INSTALL_DIR>"),
        "config output didn't reference the config path: {}",
        cap.stdout
    );
    insta::assert_snapshot!("config", cap.stdout);
}
