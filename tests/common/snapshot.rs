//! Snapshot helpers: path/timestamp normalization for stdout/stderr, and
//! tree manifests (relative path → sha256 hash + file kind) for asserting on
//! created directory contents without false positives from absolute paths,
//! .git internals, or timestamps.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use super::TestEnv;

/// Replace test-specific absolute paths and obvious time-like strings in a
/// captured stream so snapshots are stable across machines/runs.
pub fn normalize(env: &TestEnv, s: &str) -> String {
    let mut out = s.to_string();
    // Order matters: replace longest paths first so the substring of a parent
    // doesn't pre-empt the match.
    let replacements = [
        (env.config_path.display().to_string(), "<CONFIG>".to_string()),
        (env.install_dir.display().to_string(), "<INSTALL_DIR>".to_string()),
        (env.workspaces_dir.display().to_string(), "<WORKSPACES_DIR>".to_string()),
        (env.bin_dir.display().to_string(), "<BIN_DIR>".to_string()),
        (env.remotes_dir.display().to_string(), "<REMOTES>".to_string()),
        (env.locals_dir.display().to_string(), "<LOCALS>".to_string()),
        (env.home.display().to_string(), "<HOME>".to_string()),
        (env.tmp.path().display().to_string(), "<TMP>".to_string()),
    ];
    for (from, to) in replacements {
        out = out.replace(&from, &to);
    }
    // Strip ANSI escapes — bash CLI prints them on some terminals.
    out = strip_ansi(&out);
    // Trim trailing whitespace per line for diff stability.
    out = out
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    if !out.ends_with('\n') && !s.is_empty() {
        out.push('\n');
    }
    out
}

fn strip_ansi(s: &str) -> String {
    // Walk chars (not bytes) so multi-byte UTF-8 (e.g. emoji 📂, bullet •) is
    // preserved intact. The CSI scanner stays in ASCII land — once it sees ESC
    // '[' it consumes only ASCII parameter bytes until a final byte 0x40–0x7E.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // ESC; if followed by '[', skip a CSI sequence.
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                while let Some(&p) = chars.peek() {
                    chars.next();
                    if matches!(p, '\x40'..='\x7E') {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Manifest of a directory tree: `<relative-path> -> Entry`.
///
/// Designed to be snapshot-stable: `.git/` internals (object hashes vary by
/// commit time), tracked timestamp files, and absolute paths are scrubbed.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub entries: BTreeMap<String, Entry>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Entry {
    File { sha256: String, size: u64 },
    Dir,
    Symlink { target: String },
}

impl Manifest {
    pub fn of(root: &Path) -> Self {
        let mut entries = BTreeMap::new();
        if !root.exists() {
            return Self { entries };
        }
        for dirent in WalkDir::new(root).follow_links(false).sort_by_file_name() {
            let dirent = match dirent {
                Ok(d) => d,
                Err(_) => continue,
            };
            let path = dirent.path();
            if path == root {
                continue;
            }
            let rel = match path.strip_prefix(root) {
                Ok(r) => r.to_path_buf(),
                Err(_) => continue,
            };
            if should_skip(&rel) {
                // Skip the directory entirely — walkdir continues into it
                // anyway, but we drop both the dir and its descendants.
                continue;
            }
            let key = rel.to_string_lossy().replace('\\', "/");
            let meta = match path.symlink_metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let entry = if meta.file_type().is_symlink() {
                let target = std::fs::read_link(path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                Entry::Symlink { target }
            } else if meta.is_dir() {
                Entry::Dir
            } else {
                let bytes = std::fs::read(path).unwrap_or_default();
                let mut h = Sha256::new();
                h.update(&bytes);
                Entry::File {
                    sha256: format!("{:x}", h.finalize()),
                    size: bytes.len() as u64,
                }
            };
            entries.insert(key, entry);
        }
        // After collection, prune anything whose ancestor was meant to be skipped
        // but slipped through (e.g. `.git/objects/...`). `should_skip` already
        // catches most of those, but this is the safety net.
        entries.retain(|k, _| !path_matches_any_skip(&PathBuf::from(k)));
        Self { entries }
    }
}

fn should_skip(rel: &Path) -> bool {
    path_matches_any_skip(rel)
}

/// Skip noisy or non-deterministic content. Two rules:
///
/// 1. Anywhere a path component ends in `.git` (a bare repo dir) or is exactly
///    `.git`, drop everything *under* it. The directory itself still appears
///    as `Entry::Dir` so the manifest records that the repo exists.
/// 2. `.agent-workspace/created` is a timestamp; drop it.
fn path_matches_any_skip(rel: &Path) -> bool {
    let s = rel.to_string_lossy().replace('\\', "/");
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() >= 2 {
        // Any non-final component named `.git` or ending `.git` ⇒ inside a repo.
        for p in &parts[..parts.len() - 1] {
            if *p == ".git" || p.ends_with(".git") {
                return true;
            }
        }
    }
    if s.ends_with(".agent-workspace/created") {
        return true;
    }
    false
}
