//! `aw config` — show config file location + available bases.
//!
//! Output format mirrors the bash CLI for a smooth migration. The exit code
//! is 0 even when the file is missing (matching bash, which only prints "❌
//! File not found" without exiting non-zero).

use anyhow::Result;

use crate::config::Config;
use crate::paths::Paths;

pub fn run() -> Result<()> {
    let paths = Paths::from_env()?;
    println!("📋 Configuration file: {}", paths.config_file.display());
    if !paths.config_file.is_file() {
        println!("❌ File not found");
        return Ok(());
    }
    println!("✓ File exists");
    println!();
    println!("Available base configurations:");
    match Config::load(&paths.config_file) {
        Ok(cfg) => {
            for name in cfg.base_names() {
                println!("  • {}", name);
            }
        }
        Err(e) => {
            // Match bash spirit: degrade gracefully if the file is malformed.
            // Show the error on stderr but don't change exit code.
            eprintln!("(warning: could not parse config: {})", e);
        }
    }
    Ok(())
}
