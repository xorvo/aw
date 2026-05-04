//! Tiny printers used by tab-completion overrides in `aw shell-init`.
//!
//! Both functions are deliberately silent on errors: tab completion that
//! fails should fall back to "no suggestions" rather than spam the user's
//! terminal mid-keystroke.

use anyhow::Result;

use crate::config::Config;
use crate::paths::Paths;

pub fn list_workspaces() -> Result<()> {
    let paths = match Paths::from_env() {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };
    let read = match std::fs::read_dir(&paths.workspaces_dir) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    let mut names: Vec<String> = read
        .filter_map(|d| d.ok())
        .filter(|d| d.path().is_dir())
        .filter(|d| d.path().join(".agent-workspace/name").is_file())
        .map(|d| d.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for n in names {
        println!("{}", n);
    }
    Ok(())
}

pub fn list_bases() -> Result<()> {
    let paths = match Paths::from_env() {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };
    let cfg = match Config::load(&paths.config_file) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };
    for n in cfg.base_names() {
        println!("{}", n);
    }
    Ok(())
}
