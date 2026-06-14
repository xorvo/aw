//! Session list for the phone client: the dash snapshot plus the two
//! fields the PWA keys off (`needsAttention`, `ageSec`), sorted
//! attention-first then most-recently-active — same shape and order the
//! Node prototype produced from `aw dash json`.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::dash::state::{Snapshot, Status};

pub fn sessions_value() -> Result<serde_json::Value> {
    let snap = Snapshot::load()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut entries: Vec<(bool, u64, serde_json::Value)> = Vec::with_capacity(snap.entries.len());
    for p in &snap.entries {
        let needs_attention = p.status == Status::Waiting;
        let age_sec = now.saturating_sub(p.last_activity);
        let mut v = serde_json::to_value(p)?;
        v["needsAttention"] = needs_attention.into();
        v["ageSec"] = age_sec.into();
        entries.push((needs_attention, p.last_activity, v));
    }
    // waiting first, then most recently active
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    Ok(serde_json::Value::Array(entries.into_iter().map(|e| e.2).collect()))
}

/// Pane-target allowlist: only panes present in the current snapshot may
/// be addressed. Cached ~2 s so fast screen polling doesn't re-read the
/// state dir on every request.
pub struct KnownPanes {
    cache: Mutex<Option<(Instant, HashSet<String>)>>,
}

impl KnownPanes {
    pub fn new() -> Self {
        Self { cache: Mutex::new(None) }
    }

    pub fn contains(&self, pane: &str) -> bool {
        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let fresh = matches!(&*guard, Some((t, _)) if t.elapsed() < Duration::from_secs(2));
        if !fresh {
            let set: HashSet<String> = Snapshot::load()
                .map(|s| s.entries.into_iter().map(|p| p.pane_id).collect())
                .unwrap_or_default();
            *guard = Some((Instant::now(), set));
        }
        match &*guard {
            Some((_, set)) => set.contains(pane),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dash::state::PaneState;

    fn pane(id: &str, status: Status, last_activity: u64) -> PaneState {
        let mut p = PaneState::new(id, "claude");
        p.status = status;
        p.last_activity = last_activity;
        p
    }

    #[test]
    fn sessions_sort_waiting_first_then_recent() {
        // Build the value list through the same transform sessions_value
        // applies, but from a fixture snapshot (no disk).
        let entries = [
            pane("%1", Status::Idle, 100),
            pane("%2", Status::Waiting, 50),
            pane("%3", Status::Working, 200),
        ];
        let mut tagged: Vec<(bool, u64, String)> = entries
            .iter()
            .map(|p| (p.status == Status::Waiting, p.last_activity, p.pane_id.clone()))
            .collect();
        tagged.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        let order: Vec<&str> = tagged.iter().map(|t| t.2.as_str()).collect();
        assert_eq!(order, vec!["%2", "%3", "%1"], "waiting first, then recency");
    }

    #[test]
    fn session_value_carries_added_fields() {
        let p = pane("%9", Status::Waiting, 0);
        let mut v = serde_json::to_value(&p).unwrap();
        v["needsAttention"] = (p.status == Status::Waiting).into();
        v["ageSec"] = 5u64.into();
        assert_eq!(v["needsAttention"], serde_json::Value::Bool(true));
        assert_eq!(v["ageSec"], serde_json::json!(5));
        assert_eq!(v["pane_id"], serde_json::json!("%9"));
        // Fields the phone client reads must exist in the serialized shape.
        for key in ["workspace", "agent", "status", "last_event", "last_prompt"] {
            assert!(v.get(key).is_some(), "missing {}", key);
        }
    }
}
