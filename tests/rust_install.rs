//! Tests for `aw install ...` — shell rc / agent hooks / tmux bindings.
//!
//! Each test runs against a sandboxed $HOME, asserts on file contents, and
//! re-runs to confirm idempotency.

mod common;

use common::{capture, Bin, TestEnv};

// ---- shell-init rc append ----

#[test]
fn install_shell_writes_zshrc_with_marker() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(Bin::Rust, &["install", "shell", "--shell", "zsh"]));
    assert_eq!(cap.exit, 0, "{}", cap.stderr);
    let zshrc = env.home.join(".zshrc");
    let body = std::fs::read_to_string(&zshrc).unwrap();
    assert!(body.contains("# >>> aw shell-init >>>"), "missing marker: {}", body);
    assert!(body.contains("eval \"$(aw shell-init zsh)\""), "missing eval: {}", body);
    assert!(body.contains("# <<< aw shell-init <<<"), "missing close marker");
}

#[test]
fn install_shell_idempotent() {
    let env = TestEnv::new();
    let _ = env.run(Bin::Rust, &["install", "shell", "--shell", "zsh"]);
    let _ = env.run(Bin::Rust, &["install", "shell", "--shell", "zsh"]);
    let body = std::fs::read_to_string(env.home.join(".zshrc")).unwrap();
    assert_eq!(body.matches("# >>> aw shell-init >>>").count(), 1);
}

#[test]
fn install_shell_preserves_existing_content() {
    let env = TestEnv::new();
    std::fs::write(env.home.join(".zshrc"), "alias ll='ls -la'\n").unwrap();
    let _ = env.run(Bin::Rust, &["install", "shell", "--shell", "zsh"]);
    let body = std::fs::read_to_string(env.home.join(".zshrc")).unwrap();
    assert!(body.contains("alias ll='ls -la'"));
    assert!(body.contains("eval \"$(aw shell-init zsh)\""));
}

// ---- tmux bindings ----

#[test]
fn install_tmux_bindings_writes_block() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(Bin::Rust, &["install", "tmux-bindings"]));
    assert_eq!(cap.exit, 0, "{}", cap.stderr);
    let body = std::fs::read_to_string(env.home.join(".tmux.conf")).unwrap();
    assert!(body.contains("bind-key a display-popup"));
    assert!(body.contains("aw dash next-ready"));
    assert!(body.contains("# >>> aw tmux bindings >>>"));
}

#[test]
fn install_tmux_bindings_replaces_block_in_place() {
    let env = TestEnv::new();
    let path = env.home.join(".tmux.conf");
    std::fs::write(
        &path,
        "set -g mouse on\n# >>> aw tmux bindings >>>\nold-content\n# <<< aw tmux bindings <<<\nset -g foo bar\n",
    )
    .unwrap();
    let _ = env.run(Bin::Rust, &["install", "tmux-bindings"]);
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("set -g mouse on"));
    assert!(body.contains("set -g foo bar"));
    assert!(!body.contains("old-content"));
    assert!(body.contains("aw dash next-ready"));
    assert_eq!(body.matches("# >>> aw tmux bindings >>>").count(), 1);
}

// ---- claude hooks ----

#[test]
fn install_claude_hooks_writes_all_events() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(Bin::Rust, &["install", "hooks", "--agent", "claude"]));
    assert_eq!(cap.exit, 0, "{}", cap.stderr);
    let raw = std::fs::read_to_string(env.home.join(".claude/settings.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let hooks = &json["hooks"];
    for ev in ["UserPromptSubmit", "PreToolUse", "Notification", "Stop"] {
        let arr = hooks[ev].as_array().expect(ev);
        let cmds: Vec<&str> = arr
            .iter()
            .flat_map(|g| g["hooks"].as_array().unwrap())
            .map(|h| h["command"].as_str().unwrap())
            .collect();
        assert!(
            cmds.iter().any(|c| c.contains("aw hook --agent claude") && c.contains(ev)),
            "missing aw hook for {}: {:?}", ev, cmds
        );
    }
}

#[test]
fn install_claude_hooks_idempotent_and_preserves_other_entries() {
    let env = TestEnv::new();
    let path = env.home.join(".claude/settings.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"{"theme":"dark","hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo other"}]}]}}"#,
    )
    .unwrap();
    let _ = env.run(Bin::Rust, &["install", "hooks", "--agent", "claude"]);
    let _ = env.run(Bin::Rust, &["install", "hooks", "--agent", "claude"]);
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(json["theme"], "dark");
    let stop_cmds: Vec<&str> = json["hooks"]["Stop"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["hooks"].as_array().unwrap())
        .map(|h| h["command"].as_str().unwrap())
        .collect();
    let aw_count = stop_cmds.iter().filter(|c| c.contains("aw hook")).count();
    assert!(stop_cmds.iter().any(|c| *c == "echo other"));
    assert_eq!(aw_count, 1, "aw entry should be added exactly once");
}

// ---- codex hooks ----

#[test]
fn install_codex_hooks_writes_hooks_and_enables_feature() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(Bin::Rust, &["install", "hooks", "--agent", "codex"]));
    assert_eq!(cap.exit, 0, "{}", cap.stderr);

    let hooks: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(env.home.join(".codex/hooks.json")).unwrap(),
    )
    .unwrap();
    for ev in ["SessionStart", "UserPromptSubmit", "PreToolUse", "Stop"] {
        let cmds: Vec<&str> = hooks["hooks"][ev]
            .as_array()
            .expect(ev)
            .iter()
            .flat_map(|g| g["hooks"].as_array().unwrap())
            .map(|h| h["command"].as_str().unwrap())
            .collect();
        assert!(cmds.iter().any(|c| c.contains("aw hook --agent codex")));
    }

    let toml = std::fs::read_to_string(env.home.join(".codex/config.toml")).unwrap();
    assert!(toml.contains("codex_hooks = true"), "{}", toml);
}

// ---- pi extension ----

#[test]
fn install_pi_writes_extension_files() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(Bin::Rust, &["install", "hooks", "--agent", "pi"]));
    assert_eq!(cap.exit, 0, "{}", cap.stderr);
    let dir = env.home.join(".config/pi/extensions/aw-dash");
    assert!(dir.join("package.json").is_file());
    assert!(dir.join("index.ts").is_file());
    let ts = std::fs::read_to_string(dir.join("index.ts")).unwrap();
    assert!(ts.contains("aw"));
    assert!(ts.contains("agent_start"));
}

// ---- install all ----

#[test]
fn install_all_runs_every_step() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(Bin::Rust, &["install", "all"]));
    assert_eq!(cap.exit, 0, "{}", cap.stderr);
    assert!(env.home.join(".tmux.conf").is_file());
    assert!(env.home.join(".claude/settings.json").is_file());
    assert!(env.home.join(".codex/hooks.json").is_file());
    assert!(env.home.join(".config/pi/extensions/aw-dash/index.ts").is_file());
}
