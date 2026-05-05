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
    // With neither config present, we default to the XDG path (modern).
    let body = std::fs::read_to_string(env.home.join(".config/tmux/tmux.conf")).unwrap();
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

#[test]
fn install_tmux_bindings_prefers_xdg_when_present() {
    let env = TestEnv::new();
    let xdg = env.home.join(".config/tmux/tmux.conf");
    let legacy = env.home.join(".tmux.conf");
    std::fs::create_dir_all(xdg.parent().unwrap()).unwrap();
    std::fs::write(&xdg, "set -g mouse on\n").unwrap();
    std::fs::write(&legacy, "# legacy stub\n").unwrap();

    let cap = capture(&env, &env.run(Bin::Rust, &["install", "tmux-bindings"]));
    assert_eq!(cap.exit, 0, "{}", cap.stderr);

    // Bindings should land in the XDG file.
    let xdg_body = std::fs::read_to_string(&xdg).unwrap();
    assert!(xdg_body.contains("aw dash next-ready"), "XDG missing bindings:\n{}", xdg_body);
    assert!(xdg_body.contains("set -g mouse on"), "XDG should preserve existing content");

    // Legacy file should be untouched (no bindings — and we never wrote there).
    let legacy_body = std::fs::read_to_string(&legacy).unwrap();
    assert!(!legacy_body.contains("aw dash next-ready"), "legacy file should not be touched");
}

#[test]
fn install_tmux_bindings_uses_legacy_when_xdg_absent() {
    let env = TestEnv::new();
    let xdg = env.home.join(".config/tmux/tmux.conf");
    let legacy = env.home.join(".tmux.conf");
    std::fs::write(&legacy, "set -g mouse on\n").unwrap();

    let _ = env.run(Bin::Rust, &["install", "tmux-bindings"]);
    assert!(!xdg.exists(), "should not have created the XDG file");
    let body = std::fs::read_to_string(&legacy).unwrap();
    assert!(body.contains("aw dash next-ready"));
}

#[test]
fn install_tmux_bindings_creates_xdg_when_neither_exists() {
    let env = TestEnv::new();
    let xdg = env.home.join(".config/tmux/tmux.conf");
    let _ = env.run(Bin::Rust, &["install", "tmux-bindings"]);
    assert!(xdg.is_file(), "expected XDG file to be created");
    let body = std::fs::read_to_string(&xdg).unwrap();
    assert!(body.contains("aw dash next-ready"));
}

#[test]
fn install_tmux_bindings_strips_stale_block_from_other_file() {
    let env = TestEnv::new();
    let xdg = env.home.join(".config/tmux/tmux.conf");
    let legacy = env.home.join(".tmux.conf");
    // Simulate the bug we just fixed: bindings live in the wrong file.
    std::fs::write(
        &legacy,
        "# >>> aw tmux bindings >>>\nold\n# <<< aw tmux bindings <<<\n",
    )
    .unwrap();
    std::fs::create_dir_all(xdg.parent().unwrap()).unwrap();
    std::fs::write(&xdg, "set -g mouse on\n").unwrap();

    let cap = capture(&env, &env.run(Bin::Rust, &["install", "tmux-bindings"]));
    assert_eq!(cap.exit, 0);

    // New target has the bindings, legacy lost the block.
    assert!(std::fs::read_to_string(&xdg).unwrap().contains("aw dash next-ready"));
    let legacy_body = std::fs::read_to_string(&legacy).unwrap();
    assert!(!legacy_body.contains("# >>> aw tmux bindings >>>"), "stale marker remains: {}", legacy_body);
}

#[test]
fn install_tmux_bindings_honors_explicit_config_flag() {
    let env = TestEnv::new();
    let custom = env.home.join("custom-loc/tmux.conf");
    let xdg = env.home.join(".config/tmux/tmux.conf");
    std::fs::create_dir_all(xdg.parent().unwrap()).unwrap();
    std::fs::write(&xdg, "set -g mouse on\n").unwrap();

    let cap = capture(
        &env,
        &env.run(
            Bin::Rust,
            &[
                "install",
                "tmux-bindings",
                "--config",
                custom.to_str().unwrap(),
            ],
        ),
    );
    assert_eq!(cap.exit, 0, "{}", cap.stderr);
    assert!(custom.is_file(), "custom file not created");
    assert!(std::fs::read_to_string(&custom).unwrap().contains("aw dash next-ready"));
    // XDG should still be untouched.
    let xdg_body = std::fs::read_to_string(&xdg).unwrap();
    assert!(!xdg_body.contains("aw dash next-ready"));
}

#[test]
fn install_tmux_bindings_honors_xdg_config_home_env() {
    let env = TestEnv::new();
    let custom_xdg_root = env.home.join("alt-xdg");
    let xdg_path = custom_xdg_root.join("tmux/tmux.conf");

    let mut cmd = std::process::Command::new(common::Bin::Rust.path());
    cmd.args(["install", "tmux-bindings"])
        .current_dir(&env.home)
        .env_clear()
        .env("HOME", &env.home)
        .env(
            "PATH",
            format!(
                "{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
                env.fake_bin.display()
            ),
        )
        .env("AW_INSTALL_DIR", &env.install_dir)
        .env("AW_WORKSPACES_DIR", &env.workspaces_dir)
        .env("AW_BIN_DIR", &env.bin_dir)
        .env("AW_CONFIG_FILE", &env.config_path)
        .env("AW_STATE_DIR", &env.state_dir)
        .env("TMUX_TMPDIR", env.tmp.path())
        .env("XDG_CONFIG_HOME", &custom_xdg_root)
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8");
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(xdg_path.is_file(), "expected XDG_CONFIG_HOME-rooted file");
    assert!(std::fs::read_to_string(&xdg_path).unwrap().contains("aw dash next-ready"));
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
    let dir = env.home.join(".pi/agent/extensions/aw-dash");
    assert!(dir.join("package.json").is_file(), "package.json missing");
    assert!(dir.join("index.ts").is_file(), "index.ts missing");
    let ts = std::fs::read_to_string(dir.join("index.ts")).unwrap();
    assert!(ts.contains("export default"), "factory pattern missing");
    assert!(ts.contains("agent_start"));
    assert!(ts.contains("aw hook") || ts.contains("\"hook\""));
}

#[test]
fn install_pi_cleans_up_stale_old_path() {
    let env = TestEnv::new();
    // Simulate an old install from an earlier version.
    let stale = env.home.join(".config/pi/extensions/aw-dash");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("index.ts"), "// old garbage\n").unwrap();
    let cap = capture(&env, &env.run(Bin::Rust, &["install", "hooks", "--agent", "pi"]));
    assert_eq!(cap.exit, 0, "{}", cap.stderr);
    assert!(!stale.is_dir(), "stale directory should be removed");
    assert!(env.home.join(".pi/agent/extensions/aw-dash/index.ts").is_file());
}

// ---- install all ----

#[test]
fn install_all_runs_every_step() {
    let env = TestEnv::new();
    let cap = capture(&env, &env.run(Bin::Rust, &["install", "all"]));
    assert_eq!(cap.exit, 0, "{}", cap.stderr);
    // tmux bindings land in the XDG path by default since neither file
    // pre-exists in a fresh test sandbox.
    assert!(env.home.join(".config/tmux/tmux.conf").is_file());
    assert!(env.home.join(".claude/settings.json").is_file());
    assert!(env.home.join(".codex/hooks.json").is_file());
    assert!(env.home.join(".pi/agent/extensions/aw-dash/index.ts").is_file());
}
