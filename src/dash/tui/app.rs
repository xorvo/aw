//! TUI state machine.
//!
//! The visible "rows" are flattened from the snapshot — workspace headers
//! interleaved with pane rows. Headers are not selectable; `selected` always
//! lands on a pane.

use crate::dash::state::{PaneState, Snapshot};

/// One displayable line.
#[derive(Debug, Clone)]
pub enum Row {
    /// `▾ <workspace>   aw-<workspace>` (or `▸ ...` collapsed).
    Header { workspace: String, session_hint: String, collapsed: bool },
    /// A single pane line.
    Pane(PaneState),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Filter,
}

#[derive(Debug)]
pub enum Action {
    Continue,
    Quit,
    Jump(String),
    Park(String),
    Refresh,
    NextReady,
}

pub struct App {
    pub rows: Vec<Row>,
    pub selected: usize,
    pub mode: Mode,
    pub filter: String,
    pub show_preview: bool,
    pub collapsed: std::collections::HashSet<String>,
    /// Cached snapshot — for `apply()` resolving Jump/Park targets.
    pub snapshot: Snapshot,
}

impl App {
    pub fn new(snap: Snapshot) -> Self {
        let mut app = Self {
            rows: Vec::new(),
            selected: 0,
            mode: Mode::Normal,
            filter: String::new(),
            show_preview: false,
            collapsed: std::collections::HashSet::new(),
            snapshot: snap,
        };
        app.rebuild_rows();
        app
    }

    pub fn reload(&mut self, snap: Snapshot) {
        // Preserve selection by pane_id when possible.
        let prior_pane = self.selected_pane().map(|p| p.pane_id.clone());
        self.snapshot = snap;
        self.rebuild_rows();
        if let Some(p) = prior_pane {
            if let Some(idx) = self
                .rows
                .iter()
                .position(|r| matches!(r, Row::Pane(s) if s.pane_id == p))
            {
                self.selected = idx;
                return;
            }
        }
        self.clamp_selection();
    }

    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        let groups = group_filtered(&self.snapshot.entries, &self.filter);
        for (workspace, panes) in groups {
            let collapsed = self.collapsed.contains(&workspace);
            let session_hint = panes
                .first()
                .map(|p| p.session.clone())
                .unwrap_or_default();
            rows.push(Row::Header {
                workspace: workspace.clone(),
                session_hint,
                collapsed,
            });
            if !collapsed {
                for p in panes {
                    rows.push(Row::Pane(p.clone()));
                }
            }
        }
        self.rows = rows;
        self.clamp_selection();
        self.snap_to_pane();
    }

    fn clamp_selection(&mut self) {
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
    }

    /// Move selection to the nearest pane row (skip headers).
    fn snap_to_pane(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        if matches!(self.rows[self.selected], Row::Pane(_)) {
            return;
        }
        // Search forward.
        for i in self.selected..self.rows.len() {
            if matches!(self.rows[i], Row::Pane(_)) {
                self.selected = i;
                return;
            }
        }
        // Search backward.
        for i in (0..self.selected).rev() {
            if matches!(self.rows[i], Row::Pane(_)) {
                self.selected = i;
                return;
            }
        }
    }

    pub fn move_down(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let mut i = self.selected;
        while i + 1 < self.rows.len() {
            i += 1;
            if matches!(self.rows[i], Row::Pane(_)) {
                self.selected = i;
                return;
            }
        }
    }

    pub fn move_up(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let mut i = self.selected;
        while i > 0 {
            i -= 1;
            if matches!(self.rows[i], Row::Pane(_)) {
                self.selected = i;
                return;
            }
        }
    }

    pub fn selected_pane(&self) -> Option<&PaneState> {
        self.rows.get(self.selected).and_then(|r| match r {
            Row::Pane(p) => Some(p),
            _ => None,
        })
    }

    pub fn toggle_collapse(&mut self) {
        if let Some(Row::Header { workspace, .. }) = self.rows.get(self.selected).cloned() {
            if self.collapsed.contains(&workspace) {
                self.collapsed.remove(&workspace);
            } else {
                self.collapsed.insert(workspace);
            }
            self.rebuild_rows();
        }
    }

    pub fn toggle_preview(&mut self) {
        self.show_preview = !self.show_preview;
    }

    pub fn enter_filter(&mut self) {
        self.mode = Mode::Filter;
    }

    pub fn exit_filter(&mut self) {
        self.mode = Mode::Normal;
    }

    pub fn filter_push(&mut self, c: char) {
        self.filter.push(c);
        self.rebuild_rows();
    }

    pub fn filter_pop(&mut self) {
        self.filter.pop();
        self.rebuild_rows();
    }

    pub fn filter_clear(&mut self) {
        self.filter.clear();
        self.rebuild_rows();
    }

    /// Apply a stateful action; if the action is "stop the loop", returns
    /// what to do post-loop. Otherwise mutates state and returns None.
    pub fn apply(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::Park(pane) => {
                let parked_dir = match crate::dash::parked_dir() {
                    Ok(p) => p,
                    Err(_) => return None,
                };
                let _ = std::fs::create_dir_all(&parked_dir);
                let s = parked_dir.join(&pane);
                if s.exists() {
                    let _ = std::fs::remove_file(&s);
                } else {
                    let _ = std::fs::write(&s, "");
                }
                if let Ok(snap) = Snapshot::load() {
                    self.reload(snap);
                }
                None
            }
            Action::Refresh => {
                if let Ok(snap) = Snapshot::load() {
                    self.reload(snap);
                }
                None
            }
            other => Some(other),
        }
    }
}

/// Apply nucleo fuzzy filter to entries, then group by workspace, preserving
/// first-appearance order.
fn group_filtered(panes: &[PaneState], filter: &str) -> Vec<(String, Vec<PaneState>)> {
    let filtered: Vec<PaneState> = if filter.trim().is_empty() {
        panes.to_vec()
    } else {
        let mut matcher = nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT);
        let pat = nucleo_matcher::pattern::Pattern::parse(
            filter,
            nucleo_matcher::pattern::CaseMatching::Smart,
            nucleo_matcher::pattern::Normalization::Smart,
        );
        let mut keep: Vec<(u32, PaneState)> = panes
            .iter()
            .filter_map(|p| {
                let hay = format!("{} {} {}", p.workspace, p.agent, p.last_prompt);
                let mut buf = Vec::new();
                let utf32 = nucleo_matcher::Utf32Str::new(&hay, &mut buf);
                pat.score(utf32, &mut matcher).map(|score| (score, p.clone()))
            })
            .collect();
        keep.sort_by(|a, b| b.0.cmp(&a.0));
        keep.into_iter().map(|(_, p)| p).collect()
    };

    let mut order: Vec<String> = Vec::new();
    let mut grouped: std::collections::BTreeMap<String, Vec<PaneState>> =
        std::collections::BTreeMap::new();
    for p in filtered {
        if !order.contains(&p.workspace) {
            order.push(p.workspace.clone());
        }
        grouped.entry(p.workspace.clone()).or_default().push(p);
    }
    order
        .into_iter()
        .map(|k| (k.clone(), grouped.remove(&k).unwrap_or_default()))
        .collect()
}
