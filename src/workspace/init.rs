//! `aw init [base]` — clone repos as bare mirrors into the base workspace.
//!
//! For each repo URL in the config, we maintain a bare mirror at
//! `<install_dir>/base/<base>/.agent-workspace/repo-cache/<name>.git`. On
//! re-init, an existing mirror is updated via `git fetch --all --prune`
//! rather than re-cloned. Mirrors serve as `--reference-if-able` source for
//! `aw create` so workspace clones are bandwidth-cheap.
//!
//! Cloning is parallelized across repos. Output ordering is stable (one line
//! per repo, in config order), printed as each thread joins.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;

use crate::config::Config;
use crate::git;
use crate::paths::Paths;

pub fn run(base_name: &str) -> Result<()> {
    let paths = Paths::from_env()?;
    if !paths.config_file.is_file() {
        eprintln!("❌ Config file not found at {}", paths.config_file.display());
        std::process::exit(1);
    }

    let cfg = Config::load(&paths.config_file)?;
    let base = match cfg.base(base_name) {
        Some(b) => b,
        None => {
            eprintln!(
                "❌ Base configuration '{}' not found in {}",
                base_name,
                paths.config_file.display()
            );
            eprintln!("Available bases:");
            for n in cfg.base_names() {
                eprintln!("  • {}", n);
            }
            std::process::exit(1);
        }
    };

    let base_dir = paths.base_dir(base_name);
    println!("🔧 Initializing base workspace: {}", base_name);
    println!("📂 Location: {}", base_dir.display());
    println!();

    let started = Instant::now();
    std::fs::create_dir_all(&base_dir)?;

    let repo_cache = base_dir.join(".agent-workspace/repo-cache");
    let repos = &base.repos;

    if !repos.is_empty() {
        std::fs::create_dir_all(&repo_cache)?;
        println!("📦 Caching {} repositories in parallel...", repos.len());
        println!();

        let handles: Vec<_> = repos
            .iter()
            .map(|url| {
                let url = url.clone();
                let cache_path: PathBuf =
                    repo_cache.join(format!("{}.git", git::repo_basename(&url)));
                std::thread::spawn(move || cache_one(&url, &cache_path))
            })
            .collect();

        // Join in spawn order so the printed order matches the config order.
        for (h, url) in handles.into_iter().zip(repos.iter()) {
            let name = git::repo_basename(url);
            match h.join().expect("worker panic") {
                Ok(CacheOutcome::Updated) => println!("   ✓ {} cache updated", name),
                Ok(CacheOutcome::Cloned) => println!("   ✓ {} cached", name),
                Err(_) => println!("   ❌ Failed to cache {}", name),
            }
        }
        println!();
    }

    // Match bash: ensure .agent-workspace/ exists even when there are no repos
    // (so subsequent `aw create` always finds a metadata anchor).
    std::fs::create_dir_all(base_dir.join(".agent-workspace"))?;

    println!();
    println!(
        "💡 Tip: Add files to {}/ (e.g. .claude/, CLAUDE.md, docs/)",
        base_dir.display()
    );
    println!("   They will be copied to the root of every new workspace created from this base.");
    println!();
    println!("📈 Initialization Summary:");
    println!("   • Total time: {}s", started.elapsed().as_secs());
    println!();
    println!(
        "✅ Base workspace '{}' initialized at {}",
        base_name,
        base_dir.display()
    );
    Ok(())
}

enum CacheOutcome {
    Cloned,
    Updated,
}

fn cache_one(url: &str, cache_path: &std::path::Path) -> Result<CacheOutcome> {
    if cache_path.is_dir() {
        git::run(&[
            "-C", cache_path.to_str().unwrap(),
            "fetch", "--all", "--prune",
        ])?;
        Ok(CacheOutcome::Updated)
    } else {
        git::run(&["clone", "--mirror", url, cache_path.to_str().unwrap()])?;
        Ok(CacheOutcome::Cloned)
    }
}
