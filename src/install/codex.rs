//! `aw install hooks --agent codex` — wire `aw hook` into
//! `~/.codex/hooks.json` and ensure `codex_hooks = true` in
//! `~/.codex/config.toml`.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};
use toml_edit::{value, DocumentMut};

const EVENTS: &[&str] = &["SessionStart", "UserPromptSubmit", "PreToolUse", "Stop"];

pub fn install() -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    let dir = home.join(".codex");
    std::fs::create_dir_all(&dir).ok();

    let hooks_path = dir.join("hooks.json");
    let added = ensure_hooks_json(&hooks_path)?;
    if added == 0 {
        println!("✅ Codex hooks already wired in {}", hooks_path.display());
    } else {
        println!(
            "✅ Wired {} Codex hook entries in {}",
            added,
            hooks_path.display()
        );
    }

    let cfg_path = dir.join("config.toml");
    let touched = ensure_codex_hooks_enabled(&cfg_path)?;
    if touched {
        println!("✅ Enabled codex_hooks in {}", cfg_path.display());
    } else {
        println!("✅ codex_hooks already enabled in {}", cfg_path.display());
    }
    Ok(())
}

fn ensure_hooks_json(path: &Path) -> Result<usize> {
    let mut root: Value = match std::fs::read_to_string(path) {
        Ok(s) if !s.trim().is_empty() => serde_json::from_str(&s)
            .with_context(|| format!("parse {}", path.display()))?,
        _ => Value::Object(Map::new()),
    };
    let added = ensure_entries(&mut root);
    let formatted = serde_json::to_string_pretty(&root)? + "\n";
    std::fs::write(path, formatted)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(added)
}

fn ensure_entries(root: &mut Value) -> usize {
    let obj = root.as_object_mut().expect("Codex hooks root must be object");
    let hooks = obj.entry("hooks").or_insert_with(|| Value::Object(Map::new()));
    let hooks_obj = hooks.as_object_mut().expect("hooks must be a table");

    let mut added = 0;
    for ev in EVENTS {
        let cmd_str = format!("aw hook --agent codex --event {}", ev);
        let entry = hooks_obj
            .entry(ev.to_string())
            .or_insert_with(|| Value::Array(vec![]));
        let arr = entry.as_array_mut().expect("event entry must be array");
        let already = arr.iter().any(|group| {
            group
                .get("hooks")
                .and_then(|v| v.as_array())
                .map(|inner| {
                    inner.iter().any(|h| {
                        h.get("command").and_then(|v| v.as_str()) == Some(&cmd_str)
                    })
                })
                .unwrap_or(false)
        });
        if already {
            continue;
        }
        arr.push(json!({
            "hooks": [{ "type": "command", "command": cmd_str }]
        }));
        added += 1;
    }
    added
}

fn ensure_codex_hooks_enabled(path: &Path) -> Result<bool> {
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    let mut doc: DocumentMut = if raw.trim().is_empty() {
        DocumentMut::new()
    } else {
        raw.parse()
            .with_context(|| format!("parse {}", path.display()))?
    };

    let already = doc
        .get("features")
        .and_then(|t| t.as_table())
        .and_then(|t| t.get("codex_hooks"))
        .and_then(|v| v.as_bool())
        == Some(true);

    if already {
        return Ok(false);
    }

    if doc.get("features").is_none() {
        doc["features"] = toml_edit::table();
    }
    doc["features"]["codex_hooks"] = value(true);
    std::fs::write(path, doc.to_string())
        .with_context(|| format!("write {}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_all_events() {
        let mut root = Value::Object(Map::new());
        assert_eq!(ensure_entries(&mut root), EVENTS.len());
        assert_eq!(ensure_entries(&mut root), 0);
    }

    #[test]
    fn enables_codex_hooks_in_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert!(ensure_codex_hooks_enabled(&path).unwrap());
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("codex_hooks = true"));
        // Idempotent.
        assert!(!ensure_codex_hooks_enabled(&path).unwrap());
    }

    #[test]
    fn preserves_other_features() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[features]\nother_thing = true\n").unwrap();
        ensure_codex_hooks_enabled(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("other_thing = true"));
        assert!(raw.contains("codex_hooks = true"));
    }
}
