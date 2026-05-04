//! State files: `~/.cache/aw/panes/<pane_id>.json`, one per active pane.
//!
//! All writes are atomic (tempfile + rename on the same filesystem).
//! Reads tolerate missing or malformed files (skip with a warning).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::dash::{panes_dir, parked_dir};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Working,
    Waiting,
    Idle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneState {
    pub schema_version: u32,
    pub pane_id: String,
    pub session: String,
    pub workspace: String,
    pub cwd: String,
    pub agent: String,
    pub status: Status,
    pub last_event: String,
    /// Unix epoch seconds.
    pub last_activity: u64,
    pub last_prompt: String,
    /// Filled in at load time from `parked/<pane>` sentinel; not persisted
    /// in the per-pane JSON.
    #[serde(skip)]
    pub parked: bool,
}

impl PaneState {
    pub fn new(pane_id: &str, agent: &str) -> Self {
        Self {
            schema_version: 1,
            pane_id: pane_id.to_string(),
            session: String::new(),
            workspace: String::new(),
            cwd: String::new(),
            agent: agent.to_string(),
            status: Status::Idle,
            last_event: String::new(),
            last_activity: now_epoch(),
            last_prompt: String::new(),
            parked: false,
        }
    }

    pub fn write_atomic(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir {}", parent.display()))?;
        }
        let tmp = path.with_extension("json.tmp");
        let raw = serde_json::to_string_pretty(self)? + "\n";
        std::fs::write(&tmp, raw)
            .with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))
    }

    pub fn read(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?;
        let s: Self = serde_json::from_str(&raw)
            .with_context(|| format!("parse {}", path.display()))?;
        Ok(s)
    }
}

#[derive(Debug)]
pub struct Snapshot {
    pub entries: Vec<PaneState>,
}

impl Snapshot {
    pub fn load() -> Result<Self> {
        let dir = panes_dir()?;
        let parked = parked_dir().ok();
        let mut entries = Vec::new();
        if let Ok(read) = std::fs::read_dir(&dir) {
            for d in read.flatten() {
                if d.path().extension().map_or(true, |e| e != "json") {
                    continue;
                }
                if let Ok(mut s) = PaneState::read(&d.path()) {
                    if let Some(ref park_dir) = parked {
                        s.parked = park_dir.join(&s.pane_id).exists();
                    }
                    entries.push(s);
                }
            }
        }
        // Stable order: workspace, then pane_id, for snapshot/test stability.
        entries.sort_by(|a, b| {
            a.workspace
                .cmp(&b.workspace)
                .then_with(|| a.pane_id.cmp(&b.pane_id))
        });
        Ok(Self { entries })
    }

    /// Counts (working, waiting, idle). Parked panes are excluded — bash
    /// equivalent of "set aside, don't bug me about these."
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut w = 0;
        let mut wt = 0;
        let mut i = 0;
        for e in &self.entries {
            if e.parked {
                continue;
            }
            match e.status {
                Status::Working => w += 1,
                Status::Waiting => wt += 1,
                Status::Idle => i += 1,
            }
        }
        (w, wt, i)
    }
}

pub fn pane_state_path(pane_id: &str) -> Result<PathBuf> {
    let dir = panes_dir()?;
    Ok(dir.join(format!("{}.json", pane_id.replace('/', "_"))))
}

pub fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
