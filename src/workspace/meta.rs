//! `.agent-workspace/` metadata files.
//!
//!   .agent-workspace/name      — the workspace name
//!   .agent-workspace/base      — the base it was created from
//!   .agent-workspace/created   — free-form timestamp (RFC3339 today; older
//!                                workspaces may carry a locale `date` string)
//!
//! `list` reads these back.

use std::path::Path;

use anyhow::{Context, Result};

const META_DIR: &str = ".agent-workspace";

#[derive(Debug, Clone)]
pub struct WorkspaceMeta {
    pub name: String,
    pub base: String,
    /// Free-form, exactly as bash wrote it (`date` output). May be missing
    /// for older or partial workspaces; surfaced as `unknown`.
    pub created: String,
}

impl WorkspaceMeta {
    pub fn read(workspace_dir: &Path) -> Option<Self> {
        let meta_dir = workspace_dir.join(META_DIR);
        let name = read_trimmed(&meta_dir.join("name"))?;
        let base = read_trimmed(&meta_dir.join("base")).unwrap_or_else(|| "default".into());
        let created = read_trimmed(&meta_dir.join("created")).unwrap_or_else(|| "unknown".into());
        Some(Self { name, base, created })
    }

    pub fn write(workspace_dir: &Path, name: &str, base: &str, created: &str) -> Result<()> {
        let meta_dir = workspace_dir.join(META_DIR);
        std::fs::create_dir_all(&meta_dir)
            .with_context(|| format!("mkdir {}", meta_dir.display()))?;
        std::fs::write(meta_dir.join("name"), format!("{}\n", name))?;
        std::fs::write(meta_dir.join("base"), format!("{}\n", base))?;
        std::fs::write(meta_dir.join("created"), format!("{}\n", created))?;
        Ok(())
    }
}

fn read_trimmed(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    Some(raw.trim().to_string())
}
