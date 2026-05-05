//! `aw install hooks --agent claude` — wire `aw hook` into
//! `~/.claude/settings.json`.
//!
//! Schema (relevant slice):
//!
//! ```json
//! {
//!   "hooks": {
//!     "UserPromptSubmit": [
//!       { "hooks": [
//!           { "type": "command", "command": "aw hook --agent claude --event UserPromptSubmit" }
//!         ]
//!       }
//!     ],
//!     "PreToolUse":   [...],
//!     "Notification": [...],
//!     "Stop":         [...]
//!   }
//! }
//! ```
//!
//! We add an entry for each event under our own marker, leaving any
//! pre-existing entries from other tools intact. Re-running is a no-op.

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};

const EVENTS: &[&str] = &["UserPromptSubmit", "PreToolUse", "Notification", "Stop"];

pub fn install() -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    let path = home.join(".claude/settings.json");

    let mut root = read_json_or_default(&path);
    let added = ensure_entries(&mut root);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let formatted = serde_json::to_string_pretty(&root)? + "\n";
    std::fs::write(&path, formatted)
        .with_context(|| format!("write {}", path.display()))?;

    if added == 0 {
        println!("✅ Claude hooks already wired in {}", path.display());
    } else {
        println!(
            "✅ Wired {} Claude hook entries in {}",
            added,
            path.display()
        );
    }
    Ok(())
}

fn read_json_or_default(path: &std::path::Path) -> Value {
    match std::fs::read_to_string(path) {
        Ok(s) if !s.trim().is_empty() => {
            serde_json::from_str(&s).unwrap_or_else(|_| Value::Object(Map::new()))
        }
        _ => Value::Object(Map::new()),
    }
}

fn ensure_entries(root: &mut Value) -> usize {
    let obj = root.as_object_mut().expect("Claude settings root must be an object");
    let hooks = obj.entry("hooks").or_insert_with(|| Value::Object(Map::new()));
    let hooks_obj = match hooks.as_object_mut() {
        Some(m) => m,
        None => {
            *hooks = Value::Object(Map::new());
            hooks.as_object_mut().unwrap()
        }
    };

    let mut added = 0;
    for ev in EVENTS {
        let cmd_str = format!("aw hook --agent claude --event {}", ev);
        let entry = hooks_obj
            .entry(ev.to_string())
            .or_insert_with(|| Value::Array(vec![]));
        let arr = match entry.as_array_mut() {
            Some(a) => a,
            None => {
                *entry = Value::Array(vec![]);
                entry.as_array_mut().unwrap()
            }
        };
        if already_has(arr, &cmd_str) {
            continue;
        }
        arr.push(json!({
            "hooks": [
                { "type": "command", "command": cmd_str }
            ]
        }));
        added += 1;
    }
    added
}

fn already_has(group_array: &[Value], target_cmd: &str) -> bool {
    for group in group_array {
        let inner = group.get("hooks").and_then(|v| v.as_array());
        if let Some(arr) = inner {
            for h in arr {
                if h.get("command").and_then(|v| v.as_str()) == Some(target_cmd) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_all_events_to_empty_root() {
        let mut root = Value::Object(Map::new());
        let added = ensure_entries(&mut root);
        assert_eq!(added, EVENTS.len());
        // Re-run is a no-op.
        assert_eq!(ensure_entries(&mut root), 0);
    }

    #[test]
    fn preserves_unrelated_settings() {
        let mut root: Value = serde_json::from_str(
            r#"{"theme":"dark","hooks":{"Stop":[{"hooks":[{"type":"command","command":"other"}]}]}}"#,
        )
        .unwrap();
        ensure_entries(&mut root);
        assert_eq!(root["theme"], "dark");
        let stop = root["hooks"]["Stop"].as_array().unwrap();
        // Should still contain the unrelated entry.
        let cmds: Vec<&str> = stop
            .iter()
            .flat_map(|g| g["hooks"].as_array().unwrap())
            .map(|h| h["command"].as_str().unwrap())
            .collect();
        assert!(cmds.contains(&"other"));
        assert!(cmds.iter().any(|c| c.contains("aw hook --agent claude")));
    }
}
