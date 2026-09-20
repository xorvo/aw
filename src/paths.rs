//! Path resolution: env-var overrides + defaults.
//!
//! Defaults:
//!   AW_INSTALL_DIR    -> ~/.agent-workspaces
//!   AW_WORKSPACES_DIR -> ~/agent-workspaces
//!   AW_BIN_DIR        -> ~/.local/bin
//!   AW_CONFIG_FILE    -> $AW_INSTALL_DIR/config.yaml
//!   AW_BASE_DIR       -> $AW_INSTALL_DIR/base   (derived; not user-overridable)

use std::path::PathBuf;

use anyhow::{anyhow, Result};

#[derive(Debug, Clone)]
pub struct Paths {
    pub install_dir: PathBuf,
    pub workspaces_dir: PathBuf,
    /// Where the `aw` binary lives. Read by tests and by future install
    /// helpers; not currently exercised by the running CLI itself.
    #[allow(dead_code)]
    pub bin_dir: PathBuf,
    pub config_file: PathBuf,
}

impl Paths {
    pub fn from_env() -> Result<Self> {
        let home = home_dir()?;
        let install_dir = env_path("AW_INSTALL_DIR")
            .unwrap_or_else(|| home.join(".agent-workspaces"));
        let workspaces_dir = env_path("AW_WORKSPACES_DIR")
            .unwrap_or_else(|| home.join("agent-workspaces"));
        let bin_dir = env_path("AW_BIN_DIR").unwrap_or_else(|| home.join(".local/bin"));
        let config_file = env_path("AW_CONFIG_FILE")
            .unwrap_or_else(|| install_dir.join("config.yaml"));

        Ok(Self {
            install_dir,
            workspaces_dir,
            bin_dir,
            config_file,
        })
    }

    /// `$AW_INSTALL_DIR/base/<name>` — where init writes the cached mirror tree.
    pub fn base_dir(&self, name: &str) -> PathBuf {
        self.install_dir.join("base").join(name)
    }

    /// All bases live under `$AW_INSTALL_DIR/base/`.
    pub fn bases_root(&self) -> PathBuf {
        self.install_dir.join("base")
    }

    /// `$AW_WORKSPACES_DIR/<name>` — where create materializes a workspace.
    pub fn workspace_dir(&self, name: &str) -> PathBuf {
        self.workspaces_dir.join(name)
    }
}

fn env_path(key: &str) -> Option<PathBuf> {
    let v = std::env::var_os(key)?;
    if v.is_empty() {
        return None;
    }
    Some(PathBuf::from(v))
}

fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| anyhow!("could not determine home directory"))
}

/// First executable named `name` on `PATH`.
///
/// Scans PATH directories directly rather than asking a shell, so a shell
/// function or alias of the same name can't shadow the real binary (`aw`
/// itself is a shell function once `aw shell-init` is loaded, and oh-my-zsh
/// aliases `tmux`).
pub fn which(name: &str) -> Option<PathBuf> {
    which_in(&std::env::var("PATH").unwrap_or_default(), name, |p| p.is_file())
}

/// Testable core of [`which`].
pub fn which_in(
    path_var: &str,
    name: &str,
    is_file: impl Fn(&std::path::Path) -> bool,
) -> Option<PathBuf> {
    path_var.split(':').find_map(|dir| {
        if dir.is_empty() {
            return None;
        }
        let cand = std::path::Path::new(dir).join(name);
        is_file(&cand).then_some(cand)
    })
}

#[cfg(test)]
mod which_tests {
    use super::*;

    #[test]
    fn which_in_finds_the_first_hit_and_skips_empty_entries() {
        let hit = which_in("/a::/b", "tmux", |p| p == std::path::Path::new("/b/tmux"));
        assert_eq!(hit.unwrap(), PathBuf::from("/b/tmux"));
        assert!(which_in("/a:/b", "tmux", |_| false).is_none());
        assert!(which_in("", "tmux", |_| true).is_none(), "empty PATH matches nothing");
    }
}
