//! `aw delete <name>` — remove a workspace after confirmation.

use std::io::{self, BufRead, Write};

use anyhow::Result;

use crate::paths::Paths;

pub fn run(name: &str) -> Result<()> {
    let paths = Paths::from_env()?;
    let workspace_dir = paths.workspace_dir(name);
    if !workspace_dir.is_dir() {
        eprintln!("❌ Workspace '{}' not found", name);
        std::process::exit(1);
    }

    println!("⚠️  This will permanently delete workspace: {}", name);
    println!("📂 Location: {}", workspace_dir.display());
    print!("Are you sure? (y/N) ");
    io::stdout().flush().ok();

    let mut line = String::new();
    let _ = io::stdin().lock().read_line(&mut line);
    println!();
    let confirm = line.chars().next().map(|c| c == 'y' || c == 'Y').unwrap_or(false);
    if !confirm {
        println!("Cancelled");
        return Ok(());
    }

    println!("🗑️  Deleting workspace...");
    std::fs::remove_dir_all(&workspace_dir)?;
    println!("✅ Workspace '{}' deleted", name);
    Ok(())
}
