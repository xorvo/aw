//! Tiny printers used by tab-completion overrides in `aw shell-init`.
//!
//! Both functions are deliberately silent on errors: tab completion that
//! fails should fall back to "no suggestions" rather than spam the user's
//! terminal mid-keystroke.

use anyhow::Result;

use crate::config::Config;
use crate::paths::Paths;
use crate::workspace::meta::WorkspaceMeta;

/// Enumerate every workspace on disk with its metadata. Silent on errors —
/// callers (the dashboard) treat absence as "no dormant workspaces" rather
/// than failing the whole render.
pub fn enumerate_workspaces() -> Vec<WorkspaceMeta> {
    let paths = match Paths::from_env() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let read = match std::fs::read_dir(&paths.workspaces_dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<WorkspaceMeta> = read
        .filter_map(|d| d.ok())
        .filter(|d| d.path().is_dir())
        .filter_map(|d| WorkspaceMeta::read(&d.path()))
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    fn seed_workspace(root: &std::path::Path, name: &str, base: &str, created: &str) {
        let dir = root.join(name).join(".agent-workspace");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("name"), format!("{}\n", name)).unwrap();
        std::fs::write(dir.join("base"), format!("{}\n", base)).unwrap();
        std::fs::write(dir.join("created"), format!("{}\n", created)).unwrap();
    }

    #[test]
    #[serial]
    fn enumerate_returns_workspaces_sorted_by_name() {
        let tmp = TempDir::new().unwrap();
        seed_workspace(tmp.path(), "zeta", "default", "2026-03-01T10:00:00Z");
        seed_workspace(tmp.path(), "alpha", "python", "2026-03-02T10:00:00Z");
        // A bare directory without `.agent-workspace/` should be skipped.
        std::fs::create_dir_all(tmp.path().join("not-a-workspace")).unwrap();

        std::env::set_var("AW_WORKSPACES_DIR", tmp.path());
        let out = enumerate_workspaces();
        std::env::remove_var("AW_WORKSPACES_DIR");

        let names: Vec<&str> = out.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "zeta"]);
        assert_eq!(out[0].base, "python");
        assert_eq!(out[1].created, "2026-03-01T10:00:00Z");
    }

    #[test]
    #[serial]
    fn enumerate_returns_empty_when_dir_missing() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("nope");
        std::env::set_var("AW_WORKSPACES_DIR", &missing);
        let out = enumerate_workspaces();
        std::env::remove_var("AW_WORKSPACES_DIR");
        assert!(out.is_empty());
    }
}
