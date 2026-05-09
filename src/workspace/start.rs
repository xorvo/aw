//! `aw start <name> [--tmux|--no-tmux]` (and aliases `open` / `enter`) plus
//! the internal `aw _shell-start` that emits a shell snippet for the shell
//! wrapper function to eval.
//!
//! Two entry points:
//!
//! - [`run`] is the direct user-invocation: launches tmux (default if
//!   available) or `exec $SHELL` after sourcing hooks. Matches the bash CLI.
//!
//! - [`shell_start`] emits shell text on stdout for the wrapper function
//!   defined in `aw shell-init`. The wrapper runs `eval "$(aw _shell-start
//!   foo)"`, which mutates the calling shell's env + cwd. This is the
//!   smoother UX once shell integration is set up.

use std::path::Path;

use anyhow::Result;

use crate::paths::Paths;

pub fn run(name: &str, no_tmux: bool) -> Result<()> {
    let paths = Paths::from_env()?;
    let workspace_dir = paths.workspace_dir(name);
    if !workspace_dir.is_dir() {
        eprintln!("❌ Workspace '{}' not found", name);
        std::process::exit(1);
    }

    println!("🎯 Entering workspace: {}", name);
    println!("📂 Location: {}", workspace_dir.display());

    let want_tmux = if no_tmux { false } else { tmux_available() };

    if want_tmux {
        // Direct invocation: spawn or attach to aw-<name>. We don't try to
        // be clever about hook sourcing here — tmux re-execs the shell
        // anyway, and the shell-integration hook (aw-shell-hook.sh) re-runs
        // on cwd change inside the new pane.
        let session = format!("aw-{}", name);
        let exists = std::process::Command::new("tmux")
            .args(["has-session", "-t", &session])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if exists {
            println!("⚠️  Tmux session '{}' already exists", session);
            // Skip the y/n prompt for now; just attach. The shell wrapper
            // path (`_shell-start`) handles the prompt path.
            let _ = std::process::Command::new("tmux")
                .args(["attach", "-t", &session])
                .status();
        } else {
            println!("Creating tmux session: {}", session);
            let _ = std::process::Command::new("tmux")
                .args([
                    "new-session",
                    "-s", &session,
                    "-c", workspace_dir.to_str().unwrap(),
                ])
                .status();
        }
        return Ok(());
    }

    // No-tmux path: print activation message and exec the user's shell with
    // the workspace env exported. The current process is replaced.
    println!();
    println!("✅ Workspace activated! You're now in: {}", workspace_dir.display());
    println!();

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let mut cmd = std::process::Command::new(&shell);
    cmd.current_dir(&workspace_dir)
        .env("AGENT_WORKSPACE", &workspace_dir)
        .env("AGENT_WORKSPACE_NAME", name);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        return Err(err.into());
    }
    #[cfg(not(unix))]
    {
        let _ = cmd.status();
        Ok(())
    }
}

/// Spawn or switch to `aw-<name>`'s tmux session. Used by the dashboard's
/// "open dormant workspace" action, where the caller has already torn down
/// any TUI screen.
///
///   - **Inside tmux** — create the session detached if it's missing, then
///     `tmux switch-client -t aw-<name>`. Returns once switched.
///   - **Outside tmux** — `exec tmux new-session -A -s aw-<name>` so the
///     calling process is replaced by tmux. Same semantics as `aw start`.
pub fn open_or_attach_session(name: &str) -> Result<()> {
    let paths = Paths::from_env()?;
    let workspace_dir = paths.workspace_dir(name);
    if !workspace_dir.is_dir() {
        anyhow::bail!("workspace '{}' not found", name);
    }
    let session = format!("aw-{}", name);
    let dir_str = workspace_dir
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("workspace path is not valid UTF-8"))?;

    if std::env::var_os("TMUX").is_some() {
        // Already inside tmux: ensure the session exists, then switch.
        let exists = std::process::Command::new("tmux")
            .args(["has-session", "-t", &session])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !exists {
            let out = std::process::Command::new("tmux")
                .args([
                    "new-session", "-d",
                    "-s", &session,
                    "-c", dir_str,
                ])
                .output()?;
            if !out.status.success() {
                anyhow::bail!(
                    "tmux new-session failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
        let _ = std::process::Command::new("tmux")
            .args(["switch-client", "-t", &session])
            .status();
        return Ok(());
    }

    // Outside tmux: exec into `new-session -A` (attach if exists, create
    // otherwise). Replaces the current process.
    let mut cmd = std::process::Command::new("tmux");
    cmd.args([
        "new-session", "-A",
        "-s", &session,
        "-c", dir_str,
    ]);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        return Err(err.into());
    }
    #[cfg(not(unix))]
    {
        let _ = cmd.status();
        Ok(())
    }
}

/// `aw _shell-start <name>` — emit shell text for the wrapper to eval.
///
/// On success, writes `cd ...; export ...; source ...` to stdout. On error,
/// writes nothing to stdout and exits non-zero with a message on stderr.
pub fn shell_start(name: &str, no_tmux: bool) -> Result<()> {
    let paths = Paths::from_env()?;
    let workspace_dir = paths.workspace_dir(name);
    if !workspace_dir.is_dir() {
        eprintln!("❌ Workspace '{}' not found", name);
        std::process::exit(1);
    }

    let want_tmux = if no_tmux { false } else { tmux_available() };

    if want_tmux {
        // `tmux new-session -A` attaches if it exists, creates if not.
        // -d (-A only?): we want to attach (or switch), so emit a
        // switch-client when already inside tmux, else new-session -A.
        let session = format!("aw-{}", name);
        if std::env::var_os("TMUX").is_some() {
            // Inside tmux already: switch the client. Create if missing.
            println!("if ! tmux has-session -t {sess} 2>/dev/null; then", sess = sh_quote(&session));
            println!("  tmux new-session -d -s {sess} -c {dir}",
                sess = sh_quote(&session),
                dir = sh_quote(workspace_dir.to_str().unwrap()),
            );
            println!("fi");
            println!("tmux switch-client -t {sess}", sess = sh_quote(&session));
        } else {
            println!("exec tmux new-session -A -s {sess} -c {dir}",
                sess = sh_quote(&session),
                dir = sh_quote(workspace_dir.to_str().unwrap()),
            );
        }
        return Ok(());
    }

    // Plain shell activation: cd + export + source hooks. The user's shell
    // continues to live; the workspace env applies in-place.
    println!("cd {}", sh_quote(workspace_dir.to_str().unwrap()));
    println!("export AGENT_WORKSPACE={}", sh_quote(workspace_dir.to_str().unwrap()));
    println!("export AGENT_WORKSPACE_NAME={}", sh_quote(name));
    for hook in collect_hooks(&paths.install_dir, &workspace_dir) {
        println!("source {}", sh_quote(hook.to_str().unwrap()));
    }
    Ok(())
}

/// All `.sh` files under global `hooks.d/` then under per-workspace
/// `hooks.d/`. Sourced in that order — per-workspace hooks override globals.
fn collect_hooks(install_dir: &Path, workspace_dir: &Path) -> Vec<std::path::PathBuf> {
    let mut hooks = Vec::new();
    for d in [
        install_dir.join("hooks.d"),
        workspace_dir.join(".agent-workspace/hooks.d"),
    ] {
        if let Ok(read) = std::fs::read_dir(&d) {
            let mut local: Vec<_> = read
                .filter_map(|d| d.ok())
                .map(|d| d.path())
                .filter(|p| p.extension().map_or(false, |e| e == "sh"))
                .collect();
            local.sort();
            hooks.extend(local);
        }
    }
    hooks
}

fn tmux_available() -> bool {
    which_simple("tmux")
}

fn which_simple(cmd: &str) -> bool {
    let path = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return false,
    };
    for dir in std::env::split_paths(&path) {
        if dir.join(cmd).is_file() {
            return true;
        }
    }
    false
}

/// POSIX shell single-quote. Always quotes; doubles any embedded single
/// quote via `'...'\''...'`. Suitable for bash/zsh/dash/fish-with-quirks.
pub fn sh_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sh_quote_handles_single_quote() {
        assert_eq!(sh_quote("a'b"), "'a'\\''b'");
        assert_eq!(sh_quote(""), "''");
        assert_eq!(sh_quote("plain"), "'plain'");
    }
}
