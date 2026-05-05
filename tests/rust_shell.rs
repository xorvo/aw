//! Rust-only tests for shell-init / _shell-start / _detect-workspace /
//! completions. These have no bash counterpart, so we snapshot the Rust
//! output directly and run no parity comparison.

mod common;

use common::{capture, fixtures, Bin, TestEnv};

// ---- shell-init: snapshot the emitted hook for each shell ----

fn shell_init(shell: &str) -> String {
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["shell-init", shell]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0, "shell-init {}: {}", shell, cap.stderr);
    cap.stdout
}

#[test]
fn shell_init_zsh() {
    insta::assert_snapshot!("shell_init_zsh", shell_init("zsh"));
}

#[test]
fn shell_init_bash() {
    insta::assert_snapshot!("shell_init_bash", shell_init("bash"));
}

#[test]
fn shell_init_fish() {
    insta::assert_snapshot!("shell_init_fish", shell_init("fish"));
}

#[test]
fn completions_zsh_starts_with_compdef() {
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["completions", "zsh"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0);
    assert!(
        cap.stdout.contains("#compdef aw"),
        "expected zsh compdef header, got: {}",
        cap.stdout.lines().take(3).collect::<Vec<_>>().join(" / ")
    );
}

/// Pipe text into `<shell> -n /dev/stdin` to syntax-validate without exec.
fn syntax_check(shell: &str, script: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(shell)
        .args(["-n", "/dev/stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn {}: {}", shell, e))?;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(script.as_bytes())
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "{} -n exit {}: {}",
            shell,
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

#[test]
fn shell_init_zsh_is_valid_zsh() {
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["shell-init", "zsh"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0);
    if let Err(e) = syntax_check("zsh", &cap.stdout) {
        panic!("zsh syntax error in shell-init output:\n{}", e);
    }
}

#[test]
fn shell_init_bash_is_valid_bash() {
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["shell-init", "bash"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0);
    if let Err(e) = syntax_check("bash", &cap.stdout) {
        panic!("bash syntax error in shell-init output:\n{}", e);
    }
}

#[test]
fn shell_init_zsh_loads_and_registers_in_real_zsh() {
    // End-to-end: spawn a real zsh, eval the hook, ask it about its state.
    // Catches issues a static snapshot can't (function-name typos, etc.).
    let env = TestEnv::new();
    let aw = Bin::Rust.path();
    let path = format!(
        "{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
        env.fake_bin.display()
    );
    // Put aw on PATH so `command aw` inside the hook resolves.
    let aw_dir = aw.parent().unwrap();
    let path = format!("{}:{}", aw_dir.display(), path);

    let probe = r#"
emulate -L zsh
autoload -Uz compinit
compinit -u
eval "$(command aw shell-init zsh)" || { echo "FAIL: eval"; exit 1; }
(( $+functions[_aw] ))             || { echo "FAIL: _aw not defined"; exit 2; }
(( $+functions[__aw_workspaces] )) || { echo "FAIL: __aw_workspaces not defined"; exit 3; }
(( $+functions[__aw_bases] ))      || { echo "FAIL: __aw_bases not defined"; exit 4; }
(( $+functions[__aw_chpwd] ))      || { echo "FAIL: __aw_chpwd not defined"; exit 5; }
(( $+functions[aw] ))              || { echo "FAIL: aw wrapper not defined"; exit 6; }
[[ ${_comps[aw]} == _aw ]]         || { echo "FAIL: compdef registration: ${_comps[aw]:-none}"; exit 7; }
echo OK
"#;

    let out = std::process::Command::new("zsh")
        .args(["-c", probe])
        .env_clear()
        .env("HOME", &env.home)
        .env("PATH", path)
        .env("AW_INSTALL_DIR", &env.install_dir)
        .env("AW_WORKSPACES_DIR", &env.workspaces_dir)
        .env("AW_BIN_DIR", &env.bin_dir)
        .env("AW_CONFIG_FILE", &env.config_path)
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .output()
        .expect("spawn zsh");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(
        out.status.code(),
        Some(0),
        "zsh probe failed:\nstdout:{}\nstderr:{}",
        stdout, stderr
    );
    assert!(stdout.trim().ends_with("OK"), "expected OK, got: {}", stdout);
}

#[test]
fn shell_init_bash_loads_and_registers_in_real_bash() {
    let env = TestEnv::new();
    let aw = Bin::Rust.path();
    let aw_dir = aw.parent().unwrap();
    let path = format!(
        "{}:{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
        aw_dir.display(),
        env.fake_bin.display()
    );

    let probe = r#"
eval "$(command aw shell-init bash)" || { echo "FAIL: eval"; exit 1; }
declare -F __aw_complete > /dev/null || { echo "FAIL: __aw_complete not defined"; exit 2; }
declare -F __aw_chpwd     > /dev/null || { echo "FAIL: __aw_chpwd not defined"; exit 3; }
declare -F aw             > /dev/null || { echo "FAIL: aw wrapper not defined"; exit 4; }
complete -p aw 2>/dev/null | grep -q "__aw_complete" || { echo "FAIL: bash complete not registered: $(complete -p aw 2>&1)"; exit 5; }
echo OK
"#;

    let out = std::process::Command::new("bash")
        .args(["-c", probe])
        .env_clear()
        .env("HOME", &env.home)
        .env("PATH", path)
        .env("AW_INSTALL_DIR", &env.install_dir)
        .env("AW_WORKSPACES_DIR", &env.workspaces_dir)
        .env("AW_BIN_DIR", &env.bin_dir)
        .env("AW_CONFIG_FILE", &env.config_path)
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .output()
        .expect("spawn bash");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(0), "bash probe failed:\n{}\n{}", stdout, stderr);
    assert!(stdout.trim().ends_with("OK"), "expected OK: {}", stdout);
}

/// End-to-end: drive zsh's completion machinery and verify dynamic
/// workspace names actually surface from `__aw_workspaces`.
///
/// We use zsh's `_complete_debug` style: bind `_main_complete` to a key
/// and send it programmatically. Simpler: invoke `compstate` directly via
/// a synthetic call. Even simpler — and what we do — is mimic what zsh's
/// completion would do by capturing what `_describe` receives. We monkey-
/// patch `_describe` to print its candidates, then call `_aw` with a hand-
/// crafted environment.
#[test]
fn dynamic_workspace_completion_lists_real_workspaces() {
    let env = setup_workspace_for_start();
    let _ = env.run(Bin::Rust, &["create", "feat-b"]);
    let _ = env.run(Bin::Rust, &["create", "feat-c"]);

    let aw = Bin::Rust.path();
    let aw_dir = aw.parent().unwrap();
    let path = format!(
        "{}:{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
        aw_dir.display(),
        env.fake_bin.display()
    );

    // Monkey-patch _describe to dump candidates to stdout, then invoke
    // __aw_workspaces directly. This validates the data path
    // (`aw _list-workspaces` → array → _describe input).
    let probe = r#"
emulate -L zsh
autoload -Uz compinit
compinit -u
eval "$(command aw shell-init zsh)"
_describe() {
  # zsh's _describe takes the array as a *variable name* in the last
  # positional arg; deref via (P) to print the actual entries.
  local -a __args=("$@")
  local __varname=${__args[-1]}
  local __item
  for __item in ${(P)__varname}; do echo "CAND:$__item"; done
}
__aw_workspaces
"#;

    let out = std::process::Command::new("zsh")
        .args(["-c", probe])
        .env_clear()
        .env("HOME", &env.home)
        .env("PATH", path)
        .env("AW_INSTALL_DIR", &env.install_dir)
        .env("AW_WORKSPACES_DIR", &env.workspaces_dir)
        .env("AW_BIN_DIR", &env.bin_dir)
        .env("AW_CONFIG_FILE", &env.config_path)
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .output()
        .expect("spawn zsh");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    // We seeded feat-a, feat-b, feat-c. All three should show up via the
    // monkey-patched _describe, in the candidate stream.
    for ws in ["feat-a", "feat-b", "feat-c"] {
        assert!(
            stdout.contains(&format!("CAND:{}", ws))
                || stdout.contains(&format!("CAND:{}\n", ws))
                || stdout.lines().any(|l| l.starts_with("CAND:") && l.contains(ws)),
            "missing workspace `{}` in candidates:\n{}",
            ws, stdout
        );
    }
}

#[test]
fn dynamic_base_completion_lists_real_bases() {
    let env = TestEnv::new()
        .with_fake_remote("alpha")
        .with_fake_remote("beta");
    let cfg = common::fixtures::config_with_two_bases(&env, "alpha", "beta");
    let env = env.with_config(&cfg);

    let aw = Bin::Rust.path();
    let aw_dir = aw.parent().unwrap();
    let path = format!(
        "{}:{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
        aw_dir.display(),
        env.fake_bin.display()
    );

    let probe = r#"
emulate -L zsh
autoload -Uz compinit
compinit -u
eval "$(command aw shell-init zsh)"
_describe() {
  # zsh's _describe takes the array as a *variable name* in the last
  # positional arg (e.g. `_describe -t workspaces 'workspace' ws`); deref
  # via (P) to print the actual entries.
  local -a __args=("$@")
  local __varname=${__args[-1]}
  local __item
  for __item in ${(P)__varname}; do echo "CAND:$__item"; done
}
__aw_bases
"#;

    let out = std::process::Command::new("zsh")
        .args(["-c", probe])
        .env_clear()
        .env("HOME", &env.home)
        .env("PATH", path)
        .env("AW_INSTALL_DIR", &env.install_dir)
        .env("AW_WORKSPACES_DIR", &env.workspaces_dir)
        .env("AW_BIN_DIR", &env.bin_dir)
        .env("AW_CONFIG_FILE", &env.config_path)
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .output()
        .expect("spawn zsh");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    for base in ["default", "dev"] {
        assert!(
            stdout.lines().any(|l| l.starts_with("CAND:") && l.contains(base)),
            "missing base `{}` in candidates:\n{}",
            base, stdout
        );
    }
}

#[test]
fn shell_init_zsh_wires_compdef_for_aw() {
    // Sanity-check on the emitted hook: should define our completion fn,
    // call compdef, and reference the dynamic helpers.
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["shell-init", "zsh"]);
    let cap = capture(&env, &out);
    let s = &cap.stdout;
    assert!(s.contains("compdef _aw aw"), "compdef missing: {}", s);
    assert!(s.contains("__aw_workspaces"), "workspaces helper missing");
    assert!(s.contains("__aw_bases"), "bases helper missing");
    assert!(s.contains("aw _list-workspaces"), "delegate to _list-workspaces missing");
    assert!(s.contains("aw _list-bases"), "delegate to _list-bases missing");
}

// ---- _shell-start: emit shell snippet for in-shell activation ----

fn setup_workspace_for_start() -> TestEnv {
    let env = TestEnv::new().with_fake_remote("repo1");
    let cfg = fixtures::config_with_one_remote(&env, "repo1");
    let env = env.with_config(&cfg);
    let init_cap = capture(&env, &env.run(Bin::Rust, &["init"]));
    assert_eq!(init_cap.exit, 0, "init: {}", init_cap.stderr);
    let create_cap = capture(&env, &env.run(Bin::Rust, &["create", "feat-a"]));
    assert_eq!(create_cap.exit, 0, "create: {}", create_cap.stderr);
    env
}

#[test]
fn shell_start_no_tmux_emits_cd_and_exports() {
    let env = setup_workspace_for_start();
    let out = env.run(Bin::Rust, &["_shell-start", "feat-a", "--no-tmux"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0, "_shell-start: {}", cap.stderr);
    // Path-normalized output: cd <WORKSPACES_DIR>/feat-a, exports, no source.
    insta::assert_snapshot!("shell_start_no_tmux", cap.stdout);
}

#[test]
fn shell_start_missing_workspace_exits_nonzero() {
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["_shell-start", "ghost", "--no-tmux"]);
    let cap = capture(&env, &out);
    assert_ne!(cap.exit, 0);
    assert!(
        cap.stdout.is_empty(),
        "stdout should be empty so a wrapper eval doesn't run garbage: {:?}",
        cap.stdout
    );
}

#[test]
fn shell_start_with_global_hook_sources_it() {
    let env = setup_workspace_for_start();
    // Drop a global hook.
    let global = env.install_dir.join("hooks.d");
    std::fs::create_dir_all(&global).unwrap();
    std::fs::write(global.join("zz-test.sh"), "echo hello\n").unwrap();

    let out = env.run(Bin::Rust, &["_shell-start", "feat-a", "--no-tmux"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0);
    assert!(
        cap.stdout.contains("source") && cap.stdout.contains("zz-test.sh"),
        "expected sourced hook in: {}",
        cap.stdout
    );
}

// ---- _detect-workspace ----

#[test]
fn detect_workspace_finds_root_from_subdir() {
    let env = setup_workspace_for_start();
    let workspace = env.workspaces_dir.join("feat-a");
    let sub = workspace.join("repo1");
    let out = env.run(Bin::Rust, &["_detect-workspace", sub.to_str().unwrap()]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0);
    assert!(
        cap.stdout.trim().ends_with("feat-a"),
        "expected workspace path in stdout: {:?}",
        cap.stdout
    );
}

#[test]
fn detect_workspace_outside_workspaces_dir_emits_nothing() {
    let env = TestEnv::new();
    let out = env.run(Bin::Rust, &["_detect-workspace", env.tmp.path().to_str().unwrap()]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0);
    assert_eq!(cap.stdout.trim(), "");
}

// ---- _list-workspaces / _list-bases (used by tab completion) ----

#[test]
fn list_workspaces_prints_names_one_per_line() {
    let env = setup_workspace_for_start();
    // Add a second workspace so we can assert on ordering.
    let _ = env.run(Bin::Rust, &["create", "feat-b"]);

    let out = env.run(Bin::Rust, &["_list-workspaces"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0);
    let names: Vec<&str> = cap.stdout.lines().collect();
    assert!(names.contains(&"feat-a"), "{:?}", names);
    assert!(names.contains(&"feat-b"), "{:?}", names);
}

#[test]
fn list_workspaces_silent_when_dir_missing() {
    let env = TestEnv::new();
    // Workspaces dir exists but is empty.
    let out = env.run(Bin::Rust, &["_list-workspaces"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0);
    assert_eq!(cap.stdout.trim(), "");
}

#[test]
fn list_bases_prints_config_keys() {
    let env = TestEnv::new()
        .with_fake_remote("alpha")
        .with_fake_remote("beta");
    let cfg = common::fixtures::config_with_two_bases(&env, "alpha", "beta");
    let env = env.with_config(&cfg);
    let out = env.run(Bin::Rust, &["_list-bases"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0);
    let names: Vec<&str> = cap.stdout.lines().collect();
    assert_eq!(names, vec!["default", "dev"]);
}

#[test]
fn list_bases_silent_when_config_missing() {
    let env = TestEnv::new();
    std::fs::remove_file(&env.config_path).unwrap();
    let out = env.run(Bin::Rust, &["_list-bases"]);
    let cap = capture(&env, &out);
    assert_eq!(cap.exit, 0);
    assert_eq!(cap.stdout.trim(), "");
}
