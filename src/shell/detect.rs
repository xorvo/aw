//! `aw _detect-workspace <cwd>` — print the workspace path containing
//! `cwd`, or nothing if there isn't one. Called by the auto-cd hook.

use std::path::{Path, PathBuf};

use anyhow::Result;

pub fn run(cwd: &str) -> Result<()> {
    if let Some(p) = detect(Path::new(cwd)) {
        println!("{}", p.display());
    }
    // Always exit 0 — empty stdout is the "no workspace here" signal.
    Ok(())
}

fn detect(start: &Path) -> Option<PathBuf> {
    let workspaces_dir = std::env::var_os("AW_WORKSPACES_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join("agent-workspaces")));

    let mut cur = start;
    loop {
        if cur.join(".agent-workspace/name").is_file() {
            // Restrict to dirs under $AW_WORKSPACES_DIR so we never
            // false-positive on someone's pre-existing `.agent-workspace/`
            // outside the managed tree (we hit this in tests).
            if let Some(ref root) = workspaces_dir {
                if cur.starts_with(root) {
                    return Some(cur.to_path_buf());
                }
            } else {
                return Some(cur.to_path_buf());
            }
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => return None,
        }
    }
}
