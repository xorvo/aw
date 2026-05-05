//! `aw create <name> [--base <base>]` — materialize a workspace from a base.
//!
//! Steps (must match bash semantics for parity):
//!
//! 1. Validate base exists, workspace name doesn't.
//! 2. Create the workspace dir + `.agent-workspace/` metadata files
//!    EARLY — bash does this before any cloning so a partial failure
//!    still leaves a "registered" workspace that `list` can see.
//! 3. Copy base files (everything except `.agent-workspace/`).
//!    `CLAUDE.md` and `AGENTS.md` are SYMLINKED so edits propagate from the
//!    base to all workspaces; everything else is a deep copy.
//! 4. Special case: if base has `CLAUDE.md` but no `AGENTS.md`, write
//!    `@CLAUDE.md` into the base's `AGENTS.md` first so the symlink works.
//! 5. Clone each repo with `--reference-if-able <cache>` against the base's
//!    bare mirror, falling back to a plain clone if no cache exists.
//! 6. Copy `local_files`, supporting `<src> -> <dest>` rename syntax and
//!    tilde expansion.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result};

use crate::config::Config;
use crate::git;
use crate::paths::Paths;
use crate::workspace::meta::WorkspaceMeta;

pub fn run(name: &str, base_name: &str) -> Result<()> {
    let paths = Paths::from_env()?;
    let base_dir = paths.base_dir(base_name);
    if !base_dir.is_dir() {
        eprintln!(
            "❌ Base workspace '{}' not found. Run 'aw init {}' first.",
            base_name, base_name
        );
        std::process::exit(1);
    }

    let workspace_dir = paths.workspace_dir(name);
    if workspace_dir.exists() {
        eprintln!(
            "❌ Workspace '{}' already exists at {}",
            name,
            workspace_dir.display()
        );
        std::process::exit(1);
    }

    println!("🏗️  Creating workspace: {}", name);
    println!("📂 From base: {}", base_name);
    println!("📍 Location: {}", workspace_dir.display());
    println!();

    let started = Instant::now();
    std::fs::create_dir_all(&workspace_dir)?;

    // Metadata: written first so partial failures still leave the workspace
    // visible to `list`. Bash uses `date` which is locale-formatted; we use
    // RFC3339 for stability across machines.
    let created_stamp = format_now_rfc3339();
    WorkspaceMeta::write(&workspace_dir, name, base_name, &created_stamp)?;

    // Ensure AGENTS.md exists in the base if CLAUDE.md is present (so the
    // symlink we create below has something to point at).
    let claude_in_base = base_dir.join("CLAUDE.md");
    let agents_in_base = base_dir.join("AGENTS.md");
    if claude_in_base.is_file() && !agents_in_base.exists() {
        std::fs::write(&agents_in_base, "@CLAUDE.md\n")?;
    }

    copy_base_files(&base_dir, &workspace_dir)?;

    let cfg = Config::load(&paths.config_file)?;
    let base = cfg.base(base_name).expect("base validated above");

    if !base.repos.is_empty() {
        println!("🔄 Updating cached repositories and cloning in parallel...");
        println!();
        clone_repos_with_reference(&base.repos, &base_dir, &workspace_dir);
        println!();
        println!("   Cloned {} repos in {}s", base.repos.len(), started.elapsed().as_secs());
    }

    if !base.local_files.is_empty() {
        println!(
            "📁 Copying {} local directories/files...",
            base.local_files.len()
        );
        println!();
        for entry in &base.local_files {
            copy_local_entry(entry, &workspace_dir);
        }
        println!();
    }

    println!("📈 Creation Summary:");
    println!("   • Total time: {}s", started.elapsed().as_secs());
    println!();
    println!("✅ Workspace '{}' created at {}", name, workspace_dir.display());
    println!();
    println!("To start working in the workspace:");
    println!("  aw open {}", name);
    Ok(())
}

/// Copy everything in `base_dir` (except `.agent-workspace/`) into `dest`.
/// `CLAUDE.md` and `AGENTS.md` are symlinked, not copied.
fn copy_base_files(base_dir: &Path, dest: &Path) -> Result<()> {
    let mut iter = std::fs::read_dir(base_dir)?
        .filter_map(|d| d.ok())
        .filter(|d| d.file_name() != ".agent-workspace")
        .peekable();

    if iter.peek().is_none() {
        return Ok(());
    }
    println!("📋 Copying base workspace files...");

    for dirent in iter {
        let item_name = dirent.file_name();
        let src = dirent.path();
        let dst = dest.join(&item_name);
        if item_name == "CLAUDE.md" || item_name == "AGENTS.md" {
            // Replace any existing entry with a symlink to the base copy.
            let _ = std::fs::remove_file(&dst);
            symlink(&src, &dst).with_context(|| {
                format!("symlink {} -> {}", dst.display(), src.display())
            })?;
        } else {
            cp_recursive(&src, &dst)?;
        }
    }
    println!();
    Ok(())
}

#[cfg(unix)]
fn symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(not(unix))]
fn symlink(_src: &Path, _dst: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symlinks unsupported on this platform",
    ))
}

/// `cp -R src dst`, preferring the system `cp` so semantics (permissions,
/// resource forks on macOS, hard links) match bash exactly.
fn cp_recursive(src: &Path, dst: &Path) -> Result<()> {
    let status = Command::new("cp")
        .arg("-R")
        .arg(src)
        .arg(dst)
        .status()
        .with_context(|| format!("invoking cp -R {} {}", src.display(), dst.display()))?;
    if !status.success() {
        anyhow::bail!("cp -R failed for {} -> {}", src.display(), dst.display());
    }
    Ok(())
}

fn clone_repos_with_reference(repos: &[String], base_dir: &Path, workspace_dir: &Path) {
    let cache_root = base_dir.join(".agent-workspace/repo-cache");
    let handles: Vec<_> = repos
        .iter()
        .map(|url| {
            let url = url.clone();
            let cache_root = cache_root.clone();
            let workspace_dir = workspace_dir.to_path_buf();
            std::thread::spawn(move || {
                let name = git::repo_basename(&url);
                let cache = cache_root.join(format!("{}.git", name));
                // Refresh cache if present (mirrors bash's "fetch first")
                if cache.is_dir() {
                    let _ = git::run(&[
                        "-C", cache.to_str().unwrap(),
                        "fetch", "--all", "--prune",
                    ]);
                }
                let dest = workspace_dir.join(&name);
                if cache.is_dir() {
                    git::run(&[
                        "clone",
                        "--reference-if-able",
                        cache.to_str().unwrap(),
                        &url,
                        dest.to_str().unwrap(),
                    ])
                } else {
                    git::run(&["clone", &url, dest.to_str().unwrap()])
                }
            })
        })
        .collect();

    // Print results in spawn order to match bash output ordering.
    for (h, url) in handles.into_iter().zip(repos.iter()) {
        let name = git::repo_basename(url);
        match h.join().expect("worker panic") {
            Ok(()) => println!("   ✓ Cloned {}", name),
            Err(_) => println!("   ❌ Failed to clone {}", name),
        }
    }
}

/// Parse and execute one `local_files` entry.
///
/// Mirrors bash's `parse_local_file_entry` + `copy_local_entry`:
///   "src"            -> copy <src> into target/<basename(src)>
///   "src -> name"    -> copy <src> into target/<name>
///   tilde at start of src expands to $HOME.
fn copy_local_entry(entry: &str, target_dir: &Path) {
    let (src_str, dest_name_opt) = if let Some((s, d)) = entry.split_once(" -> ") {
        (s.to_string(), Some(d.to_string()))
    } else {
        (entry.to_string(), None)
    };
    let src = expand_tilde(&src_str);

    if !src.exists() {
        println!("⚠️  Warning: Local path not found: {}", entry);
        return;
    }

    let basename = src
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dest_name = dest_name_opt.clone().unwrap_or_else(|| basename.clone());
    let dest = target_dir.join(&dest_name);

    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if src.is_dir() {
        println!("⏳ Copying {} -> {}...", basename, dest_name);
        if cp_recursive(&src, &dest).is_ok() {
            println!("   ✓ Copied using standard copy in 0s");
        } else {
            println!("   ❌ Failed to copy {}", entry);
        }
    } else if src.is_file() {
        println!("📄 Copying {} -> {}", basename, dest_name);
        let _ = std::fs::copy(&src, &dest);
    }
}

fn expand_tilde(s: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(s).into_owned())
}

fn format_now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_unix(secs as i64)
}

/// Cheap RFC3339 formatter for UTC unix epoch — avoids pulling in chrono.
fn format_unix(t: i64) -> String {
    // 1970-01-01 baseline. Cumulative days per month for non-leap.
    let days_in_year = |y: i64| if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
    let secs_per_day: i64 = 86_400;
    let mut days = t / secs_per_day;
    let mut rem = t % secs_per_day;
    if rem < 0 {
        rem += secs_per_day;
        days -= 1;
    }
    let mut year = 1970_i64;
    loop {
        let dy = days_in_year(year);
        if days < dy {
            break;
        }
        days -= dy;
        year += 1;
    }
    let leap = days_in_year(year) == 366;
    let dim = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 0usize;
    while month < 12 && days >= dim[month] {
        days -= dim[month];
        month += 1;
    }
    let day = days + 1;
    let h = rem / 3600;
    let m = (rem / 60) % 60;
    let s = rem % 60;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month + 1, day, h, m, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_basic() {
        // 1970-01-01T00:00:00Z
        assert_eq!(format_unix(0), "1970-01-01T00:00:00Z");
        // 2025-01-01T00:00:00Z
        assert_eq!(format_unix(1_735_689_600), "2025-01-01T00:00:00Z");
    }
}
