//! `aw sync` — fetch + fast-forward the default branch in every repo of
//! the current workspace.
//!
//! "Current workspace" detection (mirrors bash):
//!   1. `$AGENT_WORKSPACE` if it points at a workspace dir.
//!   2. `$PWD` if it has `.agent-workspace/name`.
//!   3. Walk parents until we find one.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;

pub fn run() -> Result<()> {
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

    println!("🔄 Syncing repos in workspace: {}", ws_name);
    println!("📂 {}", workspace_dir.display());
    println!();

    let mut synced = 0u32;
    let mut failed = 0u32;
    let mut skipped = 0u32;

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
        match sync_repo(&dir) {
            SyncResult::UpToDate(branch) => {
                println!("  ✓ {} ({}): already up to date", repo_name, branch);
                synced += 1;
            }
            SyncResult::FastForwarded { branch, ahead } => {
                println!(
                    "  ✓ {} ({}): fast-forwarded {} commit(s)",
                    repo_name, branch, ahead
                );
                synced += 1;
            }
            SyncResult::SkippedNoBranch => {
                println!(
                    "  ⚠️  {}: could not determine default branch, skipping",
                    repo_name
                );
                skipped += 1;
            }
            SyncResult::SkippedDiverged(branch) => {
                println!(
                    "  ⚠️  {} ({}): local has diverged, skipping (rebase manually)",
                    repo_name, branch
                );
                skipped += 1;
            }
            SyncResult::SkippedNoLocal(branch) => {
                println!(
                    "  ⚠️  {}: local branch '{}' not found, skipping",
                    repo_name, branch
                );
                skipped += 1;
            }
            SyncResult::FailedFetch => {
                println!("  ❌ {}: fetch failed", repo_name);
                failed += 1;
            }
            SyncResult::FailedUpdate(branch) => {
                println!("  ❌ {} ({}): update failed", repo_name, branch);
                failed += 1;
            }
        }
    }

    println!();
    println!(
        "📈 Sync complete: {} synced, {} skipped, {} failed",
        synced, skipped, failed
    );
    Ok(())
}

enum SyncResult {
    UpToDate(String),
    FastForwarded { branch: String, ahead: u32 },
    SkippedNoBranch,
    SkippedDiverged(String),
    SkippedNoLocal(String),
    FailedFetch,
    FailedUpdate(String),
}

fn sync_repo(dir: &Path) -> SyncResult {
    let default_branch = match resolve_default_branch(dir) {
        Some(b) => b,
        None => return SyncResult::SkippedNoBranch,
    };

    if git(dir, &["fetch", "origin", &default_branch, "--quiet"]).is_err() {
        return SyncResult::FailedFetch;
    }

    let local_ref = format!("refs/heads/{}", default_branch);
    let remote_ref = format!("refs/remotes/origin/{}", default_branch);
    if !ref_exists(dir, &local_ref) {
        return SyncResult::SkippedNoLocal(default_branch);
    }

    let local_sha = capture(dir, &["rev-parse", &local_ref]);
    let remote_sha = capture(dir, &["rev-parse", &remote_ref]);
    if local_sha.is_empty() || remote_sha.is_empty() {
        return SyncResult::FailedFetch;
    }

    if local_sha == remote_sha {
        return SyncResult::UpToDate(default_branch);
    }

    if git(dir, &["merge-base", "--is-ancestor", &local_ref, &remote_ref]).is_err() {
        return SyncResult::SkippedDiverged(default_branch);
    }

    if git(
        dir,
        &["update-ref", &local_ref, &remote_sha, &local_sha],
    )
    .is_err()
    {
        return SyncResult::FailedUpdate(default_branch);
    }

    let count = capture(
        dir,
        &[
            "rev-list",
            &format!("{}..{}", local_sha, remote_sha),
            "--count",
        ],
    );
    let ahead: u32 = count.parse().unwrap_or(0);
    SyncResult::FastForwarded {
        branch: default_branch,
        ahead,
    }
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
