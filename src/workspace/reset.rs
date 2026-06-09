//! `aw reset [--hard]` — fetch the default branch and reset every repo in
//! the current workspace to it.
//!
//! Without `--hard`: skip any repo that has uncommitted changes or whose
//! HEAD has commits not on `origin/<default>` (i.e. the reset would lose
//! work). Skipped repos are listed at the end.
//!
//! With `--hard`: ignore both checks and force-reset to the remote default
//! branch.
//!
//! Worktree is always switched to the default branch on a successful reset.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;

pub fn run(hard: bool) -> Result<()> {
    let workspace_dir = match detect_workspace() {
        Some(p) => p,
        None => {
            eprintln!(
                "❌ Not inside a workspace. Navigate to a workspace or use 'aw open <name>' first."
            );
            std::process::exit(1);
        }
    };

    let ws_name = std::fs::read_to_string(workspace_dir.join(".agent-workspace/name"))
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| {
            workspace_dir
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });

    if hard {
        println!("🔧 Hard-resetting repos in workspace: {}", ws_name);
    } else {
        println!("🔧 Resetting repos in workspace: {}", ws_name);
    }
    println!("📂 {}", workspace_dir.display());
    println!();

    let mut reset_count = 0u32;
    let mut failed = 0u32;
    let mut skipped = 0u32;
    let mut warnings: Vec<String> = Vec::new();

    let read = match std::fs::read_dir(&workspace_dir) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    let mut repo_dirs: Vec<PathBuf> = read
        .filter_map(|d| d.ok())
        .map(|d| d.path())
        .filter(|p| p.is_dir() && p.join(".git").exists())
        .collect();
    repo_dirs.sort();

    for dir in repo_dirs {
        let repo_name = dir
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        match reset_repo(&dir, hard) {
            ResetResult::Reset(branch) => {
                println!("  ✓ {}: reset to origin/{}", repo_name, branch);
                reset_count += 1;
            }
            ResetResult::SkippedNoBranch => {
                println!(
                    "  ⚠️  {}: could not determine default branch, skipping",
                    repo_name
                );
                warnings.push(format!("{}: could not determine default branch", repo_name));
                skipped += 1;
            }
            ResetResult::SkippedDirty => {
                println!("  ⚠️  {}: uncommitted changes, skipping", repo_name);
                warnings.push(format!(
                    "{}: uncommitted changes (use --hard to discard)",
                    repo_name
                ));
                skipped += 1;
            }
            ResetResult::SkippedDiverged(branch) => {
                println!(
                    "  ⚠️  {}: HEAD has commits not on origin/{}, skipping",
                    repo_name, branch
                );
                warnings.push(format!(
                    "{}: divergent commits on HEAD (use --hard to discard)",
                    repo_name
                ));
                skipped += 1;
            }
            ResetResult::FailedFetch => {
                println!("  ❌ {}: fetch failed", repo_name);
                failed += 1;
            }
            ResetResult::FailedCheckout(branch) => {
                println!("  ❌ {}: failed to checkout {}", repo_name, branch);
                failed += 1;
            }
            ResetResult::FailedReset(branch) => {
                println!("  ❌ {} ({}): reset failed", repo_name, branch);
                failed += 1;
            }
        }
    }

    println!();
    println!(
        "📈 Reset complete: {} reset, {} skipped, {} failed",
        reset_count, skipped, failed
    );

    if !warnings.is_empty() {
        println!();
        println!("⚠️  Skipped repos:");
        for w in &warnings {
            println!("  - {}", w);
        }
        if !hard {
            println!();
            println!("Re-run with 'aw reset --hard' to force-reset and discard local changes.");
        }
    }

    Ok(())
}

enum ResetResult {
    Reset(String),
    SkippedNoBranch,
    SkippedDirty,
    SkippedDiverged(String),
    FailedFetch,
    FailedCheckout(String),
    FailedReset(String),
}

fn reset_repo(dir: &Path, hard: bool) -> ResetResult {
    let default_branch = match resolve_default_branch(dir) {
        Some(b) => b,
        None => return ResetResult::SkippedNoBranch,
    };

    if git(dir, &["fetch", "origin", &default_branch, "--quiet"]).is_err() {
        return ResetResult::FailedFetch;
    }

    let remote_ref = format!("refs/remotes/origin/{}", default_branch);

    if !hard {
        // Uncommitted changes (includes untracked) → skip.
        if !capture(dir, &["status", "--porcelain"]).is_empty() {
            return ResetResult::SkippedDirty;
        }
        // HEAD ahead of (or diverged from) origin/<default> → skip.
        let head_sha = capture(dir, &["rev-parse", "HEAD"]);
        if !head_sha.is_empty()
            && git(
                dir,
                &["merge-base", "--is-ancestor", &head_sha, &remote_ref],
            )
            .is_err()
        {
            return ResetResult::SkippedDiverged(default_branch);
        }
    }

    // Switch worktree to the default branch and point it at the remote tip.
    // `-B` covers both "branch exists" and "branch missing" in one call; under
    // `--hard` we also add `-f` so a tracked-file conflict between the current
    // branch and the default branch doesn't block the switch. (Without --hard
    // the worktree is already verified clean above, so -f isn't needed.)
    let mut args: Vec<&str> = vec!["checkout", "-q"];
    if hard {
        args.push("-f");
    }
    args.extend(["-B", &default_branch, &remote_ref]);
    if git(dir, &args).is_err() {
        return ResetResult::FailedCheckout(default_branch);
    }

    // `checkout -B <branch> <ref>` already aligns HEAD + worktree with <ref>,
    // but redo the hard reset as defense in depth (cheap; idempotent).
    if git(dir, &["reset", "--hard", "--quiet", &remote_ref]).is_err() {
        return ResetResult::FailedReset(default_branch);
    }

    ResetResult::Reset(default_branch)
}

fn resolve_default_branch(dir: &Path) -> Option<String> {
    let head_ref = capture(dir, &["symbolic-ref", "refs/remotes/origin/HEAD"]);
    if let Some(b) = head_ref.strip_prefix("refs/remotes/origin/") {
        return Some(b.to_string());
    }
    for candidate in ["main", "master"] {
        if ref_exists(dir, &format!("refs/remotes/origin/{}", candidate)) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn ref_exists(dir: &Path, refname: &str) -> bool {
    git(dir, &["show-ref", "--verify", "--quiet", refname]).is_ok()
}

fn git(dir: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .args(["-C", dir.to_str().unwrap()])
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        anyhow::bail!("git {:?}", args);
    }
    Ok(())
}

fn capture(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-C", dir.to_str().unwrap()])
        .args(args)
        .stderr(std::process::Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

fn detect_workspace() -> Option<PathBuf> {
    if let Some(env_ws) = std::env::var_os("AGENT_WORKSPACE")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
    {
        if env_ws.join(".agent-workspace").is_dir() {
            return Some(env_ws);
        }
    }
    let pwd = std::env::current_dir().ok()?;
    if pwd.join(".agent-workspace/name").is_file() {
        return Some(pwd);
    }
    let mut cur = pwd.as_path();
    while let Some(parent) = cur.parent() {
        if cur.join(".agent-workspace/name").is_file() {
            return Some(cur.to_path_buf());
        }
        cur = parent;
    }
    None
}
