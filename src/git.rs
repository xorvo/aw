//! Thin wrappers around the `git` CLI.
//!
//! We shell out instead of linking libgit2 to keep dependencies light and
//! preserve the user's ambient git config, ssh agent, credential helpers,
//! and familiar error messages.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Result};

/// Run `git <args>` quietly. Returns Err on non-zero exit.
pub fn run(args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        bail!("git {:?} exited with {:?}", args, status.code());
    }
    Ok(())
}

/// Capture stdout from `git <args>` (e.g. `branch --show-current`). Returns
/// the trimmed stdout. Empty on non-zero exit.
#[allow(dead_code)] // used by future dash integrations
pub fn capture_stdout(cwd: Option<&Path>, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let out = cmd.args(args).stderr(Stdio::null()).output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

/// Derive a repo's local directory name from a clone URL.
///
/// Mirrors `basename "$repo" .git`:
///   git@github.com:org/foo.git    -> foo
///   https://example.com/bar.git   -> bar
///   file:///tmp/baz.git           -> baz
///   /local/path/qux               -> qux
pub fn repo_basename(url: &str) -> String {
    // Trim trailing slashes.
    let url = url.trim_end_matches('/');
    // Take everything after the last `/` or `:` (handles SSH-style git@host:path).
    let last = url
        .rsplit_once(|c| c == '/' || c == ':')
        .map(|(_, b)| b)
        .unwrap_or(url);
    last.strip_suffix(".git").unwrap_or(last).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_common_url_shapes() {
        assert_eq!(repo_basename("git@github.com:org/foo.git"), "foo");
        assert_eq!(repo_basename("https://example.com/bar.git"), "bar");
        assert_eq!(repo_basename("file:///tmp/baz.git"), "baz");
        assert_eq!(repo_basename("/local/path/qux"), "qux");
        assert_eq!(repo_basename("plain"), "plain");
        assert_eq!(repo_basename("with-trailing-slash/"), "with-trailing-slash");
    }
}
