//! Test harness for parity + dashboard tests.
//!
//! Each integration test file at `tests/<name>.rs` declares `mod common;` to
//! pull this module in. Cargo treats files under `tests/common/` as a shared
//! module rather than its own test binary.

#![allow(dead_code)] // each test binary uses a subset of helpers

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

pub mod fixtures;
pub mod snapshot;

/// Which `aw` binary the test is driving. Parity tests run the same scenario
/// twice (once per arm); Rust-only tests pin `Bin::Rust`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Bin {
    /// The frozen bash reference at `tests/fixtures/aw-bash`.
    Bash,
    /// The Rust binary built by `cargo build`.
    Rust,
}

impl Bin {
    pub fn path(&self) -> PathBuf {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        match self {
            Bin::Bash => manifest.join("tests/fixtures/aw-bash"),
            // assert_cmd resolves the Cargo-built binary including any
            // CARGO_TARGET_DIR override; fall back to target/debug for plain
            // `cargo test` invocations.
            Bin::Rust => assert_cmd::cargo::cargo_bin("aw"),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Bin::Bash => "bash",
            Bin::Rust => "rust",
        }
    }
}

/// Sandbox: a self-contained set of directories pointed at by aw's env-var
/// overrides. No global state is touched.
pub struct TestEnv {
    pub tmp: TempDir,
    pub home: PathBuf,
    pub install_dir: PathBuf,
    pub workspaces_dir: PathBuf,
    pub bin_dir: PathBuf,
    pub config_path: PathBuf,
    /// Where bare git repos live so configs can reference them as
    /// `file://<remotes_dir>/<name>.git`.
    pub remotes_dir: PathBuf,
    /// Where local fixture files live so configs can reference them
    /// directly via path under `local_files`.
    pub locals_dir: PathBuf,
    /// Directory of fake editors (`cursor`, `code`, etc. shimmed to `exit 0`)
    /// that we prepend to PATH so `edit-config` / `edit-base` / `open-home`
    /// don't actually launch GUI editors and block the test runner.
    pub fake_bin: PathBuf,
    /// `$AW_STATE_DIR` — where dash state files live. Sandboxed per-test so
    /// nothing leaks into the real `~/.cache/aw`.
    pub state_dir: PathBuf,
}

impl TestEnv {
    /// Build an empty sandbox with a minimal config (`default: { repos: [], local_files: [] }`).
    pub fn new() -> Self {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();
        let home = root.join("home");
        let install_dir = home.join(".agent-workspaces");
        let workspaces_dir = home.join("agent-workspaces");
        let bin_dir = home.join(".local/bin");
        let config_path = install_dir.join("config.yaml");
        let remotes_dir = root.join("remotes");
        let locals_dir = root.join("locals");
        let fake_bin = root.join("fake-bin");
        let state_dir = root.join("state");

        for d in [
            &home,
            &install_dir,
            &workspaces_dir,
            &bin_dir,
            &remotes_dir,
            &locals_dir,
            &fake_bin,
            &state_dir,
        ] {
            std::fs::create_dir_all(d).expect("mkdir");
        }

        std::fs::write(
            &config_path,
            "default:\n  repos: []\n  local_files: []\n",
        )
        .expect("write default config");

        // Drop in shims for every editor either binary might try to launch.
        // Each is `exit 0` — `command -v` finds them, the launch returns
        // immediately, and no Cursor/VS Code window pops up mid-test.
        for name in ["cursor", "code", "nvim", "vim", "nano", "open", "xdg-open"] {
            let p = fake_bin.join(name);
            std::fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perm = std::fs::metadata(&p).unwrap().permissions();
                perm.set_mode(0o755);
                std::fs::set_permissions(&p, perm).unwrap();
            }
        }

        Self {
            tmp,
            home,
            install_dir,
            workspaces_dir,
            bin_dir,
            config_path,
            remotes_dir,
            locals_dir,
            fake_bin,
            state_dir,
        }
    }

    /// Replace the config file with the given YAML literal.
    pub fn with_config(self, yaml: &str) -> Self {
        std::fs::write(&self.config_path, yaml).expect("write config");
        self
    }

    /// Initialize a bare git repo at `<remotes_dir>/<name>.git` with one
    /// commit on `main` containing a `README.md`. Returns self for chaining.
    pub fn with_fake_remote(self, name: &str) -> Self {
        let bare = self.remotes_dir.join(format!("{}.git", name));
        run_git(&["init", "--bare", "--quiet", "-b", "main", bare.to_str().unwrap()]);

        let work = self.tmp.path().join(format!("__work_{}", name));
        std::fs::create_dir_all(&work).unwrap();
        run_git(&["init", "--quiet", "-b", "main", work.to_str().unwrap()]);
        std::fs::write(work.join("README.md"), format!("# {}\n", name)).unwrap();
        let work_str = work.to_str().unwrap();
        run_git(&["-C", work_str, "add", "."]);
        run_git(&[
            "-C", work_str,
            "-c", "user.name=test",
            "-c", "user.email=t@e",
            "commit", "--quiet", "-m", "initial",
        ]);
        run_git(&["-C", work_str, "remote", "add", "origin", bare.to_str().unwrap()]);
        run_git(&["-C", work_str, "push", "--quiet", "-u", "origin", "main"]);

        self
    }

    /// Drop a local fixture file/directory under `<locals_dir>` so it can be
    /// referenced from `local_files:` in the config.
    pub fn with_local_file(self, rel: &str, content: &str) -> Self {
        let path = self.locals_dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
        self
    }

    /// Resolve the URL form a config can use to reference a fake remote.
    pub fn remote_url(&self, name: &str) -> String {
        format!("file://{}", self.remotes_dir.join(format!("{}.git", name)).display())
    }

    /// Resolve the path a config can use under `local_files:` for a fixture.
    pub fn local_path(&self, rel: &str) -> String {
        self.locals_dir.join(rel).display().to_string()
    }

    /// Run `aw <args>` with the sandbox env. Captures stdout, stderr, and exit.
    pub fn run(&self, bin: Bin, args: &[&str]) -> Output {
        self.run_with_stdin(bin, args, "")
    }

    /// Run `aw <args>` with stdin piped in. Use for confirmation prompts
    /// (e.g. `delete` reads `y/N`).
    pub fn run_with_stdin(&self, bin: Bin, args: &[&str], stdin: &str) -> Output {
        use std::io::Write;
        use std::process::Stdio;
        let mut cmd = Command::new(bin.path());
        cmd.args(args);
        configure_env(&mut cmd, self);
        if !stdin.is_empty() {
            cmd.stdin(Stdio::piped());
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("failed to spawn aw");
        if !stdin.is_empty() {
            if let Some(mut s) = child.stdin.take() {
                s.write_all(stdin.as_bytes()).expect("write stdin");
            }
        }
        child.wait_with_output().expect("wait")
    }

}

fn configure_env(cmd: &mut Command, env: &TestEnv) {
    let path = format!(
        "{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
        env.fake_bin.display()
    );
    cmd.current_dir(&env.home) // pin cwd inside the sandbox so workspace
                               // detection (parent walk) doesn't escape into
                               // a real `.agent-workspace/` outside the test.
        .env_clear()
        .env("HOME", &env.home)
        .env("PATH", path)
        .env("AW_INSTALL_DIR", &env.install_dir)
        .env("AW_WORKSPACES_DIR", &env.workspaces_dir)
        .env("AW_BIN_DIR", &env.bin_dir)
        .env("AW_CONFIG_FILE", &env.config_path)
        .env("AW_STATE_DIR", &env.state_dir)
        // Insulate the test from any tmux server running on the host: tmux
        // looks for sockets under `$TMUX_TMPDIR/tmux-<uid>/`. We point at a
        // fresh sandbox dir, so `tmux list-panes -a` finds no server and
        // `aw dash` returns only state-file-derived rows.
        .env("TMUX_TMPDIR", env.tmp.path())
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .env("TZ", "UTC")
        // UTF-8 locale: `C` mangles bash's emoji output. `en_US.UTF-8` is
        // available on macOS by default; on Linux CI we install `en_US.UTF-8`
        // (or fall back to `C.UTF-8` via locale-gen).
        .env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .env("EDITOR", "true"); // any subcommand that opens an editor no-ops
}

fn run_git(args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .status()
        .expect("git");
    assert!(status.success(), "git {:?} failed", args);
}

/// Convert an Output into a (stdout_string, stderr_string, exit_code) triple
/// with paths/timestamps normalized for snapshotting.
pub fn capture(env: &TestEnv, out: &Output) -> CapturedOutput {
    CapturedOutput {
        stdout: snapshot::normalize(env, &String::from_utf8_lossy(&out.stdout)),
        stderr: snapshot::normalize(env, &String::from_utf8_lossy(&out.stderr)),
        exit: out.status.code().unwrap_or(-1),
    }
}

#[derive(serde::Serialize, Debug)]
pub struct CapturedOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit: i32,
}

/// Convenience: assert command succeeded and return the captured output.
pub fn ok(env: &TestEnv, out: Output) -> CapturedOutput {
    let captured = capture(env, &out);
    assert_eq!(
        captured.exit, 0,
        "expected success, got exit={}\nstdout:\n{}\nstderr:\n{}",
        captured.exit, captured.stdout, captured.stderr
    );
    captured
}

/// Walk a directory and return a sorted manifest of `<rel> -> hash` for snapshot diffing.
pub fn tree_manifest(root: &Path) -> snapshot::Manifest {
    snapshot::Manifest::of(root)
}
