//! TUI state machine.
//!
//! The visible "rows" are flattened from the snapshot — workspace headers
//! interleaved with pane rows. Headers are not selectable; `selected` always
//! lands on a pane.

use crate::dash::state::{DormantWorkspace, PaneState, Snapshot};

/// One displayable line.
#[derive(Debug, Clone)]
pub enum Row {
    /// `▾ <workspace>   aw-<workspace>` (or `▸ ...` collapsed).
    Header { workspace: String, session_hint: String, collapsed: bool },
    /// A single pane line.
    Pane(PaneState),
    /// "─ Dormant ─" divider above the dormant block. Not selectable.
    DormantDivider,
    /// A workspace-on-disk row with no live tmux session. Selectable;
    /// Enter spawns an `aw-<name>` session and switches to it.
    Dormant(DormantWorkspace),
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
    /// Open a dormant workspace by name (creates the `aw-<name>` session
    /// if needed and switches to it).
    OpenWorkspace(String),
}

pub struct App {
    pub rows: Vec<Row>,
    pub selected: usize,
    pub mode: Mode,
    pub filter: String,
    pub show_preview: bool,
    pub collapsed: std::collections::HashSet<String>,
    /// Whether the Dormant section is rendered. Toggled with `H`.
    pub show_dormant: bool,
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
            show_dormant: true,
            snapshot: snap,
        };
        app.rebuild_rows();
        app
    }

    pub fn reload(&mut self, snap: Snapshot) {
        // Preserve selection across reloads. Try pane_id first, then dormant
        // workspace name, so the cursor doesn't jump when the snapshot
        // reshuffles.
        let prior_pane = self.selected_pane().map(|p| p.pane_id.clone());
        let prior_dormant = self.selected_dormant().map(|d| d.name.clone());
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
        if let Some(name) = prior_dormant {
            if let Some(idx) = self
                .rows
                .iter()
                .position(|r| matches!(r, Row::Dormant(d) if d.name == name))
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

        if self.show_dormant {
            let dormant = filter_dormant(&self.snapshot.dormant, &self.filter);
            if !dormant.is_empty() {
                rows.push(Row::DormantDivider);
                for d in dormant {
                    rows.push(Row::Dormant(d));
                }
            }
        }

        self.rows = rows;
        self.clamp_selection();
        self.snap_to_selectable();
    }

    fn clamp_selection(&mut self) {
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
    }

    /// Move selection to the nearest selectable row (Pane or Dormant).
    /// Skips headers and the dormant divider.
    fn snap_to_selectable(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        if is_selectable(&self.rows[self.selected]) {
            return;
        }
        // Search forward.
        for i in self.selected..self.rows.len() {
            if is_selectable(&self.rows[i]) {
                self.selected = i;
                return;
            }
        }
        // Search backward.
        for i in (0..self.selected).rev() {
            if is_selectable(&self.rows[i]) {
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
            if is_selectable(&self.rows[i]) {
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
            if is_selectable(&self.rows[i]) {
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

    pub fn selected_dormant(&self) -> Option<&DormantWorkspace> {
        self.rows.get(self.selected).and_then(|r| match r {
            Row::Dormant(d) => Some(d),
            _ => None,
        })
    }

    pub fn toggle_dormant(&mut self) {
        self.show_dormant = !self.show_dormant;
        self.rebuild_rows();
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

fn is_selectable(row: &Row) -> bool {
    matches!(row, Row::Pane(_) | Row::Dormant(_))
}

/// Filter dormant workspaces by name/base via the same fuzzy matcher used
/// for live entries. Empty filter returns the input unchanged. Sort order
/// is alphabetical (input is already sorted by `Snapshot::load`).
fn filter_dormant(dormant: &[DormantWorkspace], filter: &str) -> Vec<DormantWorkspace> {
    if filter.trim().is_empty() {
        // Defensive sort. `compute_dormant` already sorts on load, but
        // App::new can be constructed with arbitrary input (tests, custom
        // tooling) — keep alphabetical ordering as a hard guarantee.
        let mut out = dormant.to_vec();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        return out;
    }
    let mut matcher = nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT);
    let pat = nucleo_matcher::pattern::Pattern::parse(
        filter,
        nucleo_matcher::pattern::CaseMatching::Smart,
        nucleo_matcher::pattern::Normalization::Smart,
    );
    let mut keep: Vec<(u32, DormantWorkspace)> = dormant
        .iter()
        .filter_map(|d| {
            let hay = format!("{} {}", d.name, d.base);
            let mut buf = Vec::new();
            let utf32 = nucleo_matcher::Utf32Str::new(&hay, &mut buf);
            pat.score(utf32, &mut matcher).map(|score| (score, d.clone()))
        })
        .collect();
    keep.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    keep.into_iter().map(|(_, d)| d).collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dash::state::Status;

    fn pane(pane_id: &str, ws: &str, agent: &str) -> PaneState {
        PaneState {
            schema_version: 1,
            pane_id: pane_id.into(),
            session: format!("aw-{}", ws),
            workspace: ws.into(),
            cwd: format!("/tmp/{}", ws),
            agent: agent.into(),
            status: Status::Idle,
            last_event: String::new(),
            last_activity: 0,
            last_prompt: String::new(),
            parked: false,
        }
    }

    fn dormant(name: &str, base: &str) -> DormantWorkspace {
        DormantWorkspace {
            name: name.into(),
            base: base.into(),
            created: "2026-03-01T10:00:00Z".into(),
        }
    }

    fn snap(panes: Vec<PaneState>, dormant: Vec<DormantWorkspace>) -> Snapshot {
        Snapshot { entries: panes, dormant }
    }

    #[test]
    fn rows_include_dormant_section_after_panes() {
        let app = App::new(snap(
            vec![pane("%1", "alpha", "claude")],
            vec![dormant("zeta", "default"), dormant("yankee", "python")],
        ));
        // header(alpha), pane(%1), divider, dormant(yankee), dormant(zeta)
        assert_eq!(app.rows.len(), 5);
        assert!(matches!(app.rows[0], Row::Header { .. }));
        assert!(matches!(app.rows[1], Row::Pane(_)));
        assert!(matches!(app.rows[2], Row::DormantDivider));
        match &app.rows[3] {
            Row::Dormant(d) => assert_eq!(d.name, "yankee"),
            _ => panic!("expected dormant"),
        }
        match &app.rows[4] {
            Row::Dormant(d) => assert_eq!(d.name, "zeta"),
            _ => panic!("expected dormant"),
        }
    }

    #[test]
    fn toggle_dormant_hides_section() {
        let mut app = App::new(snap(
            vec![pane("%1", "alpha", "claude")],
            vec![dormant("dorm", "default")],
        ));
        assert!(app.rows.iter().any(|r| matches!(r, Row::Dormant(_))));
        app.toggle_dormant();
        assert!(!app.rows.iter().any(|r| matches!(r, Row::Dormant(_))));
        assert!(!app.rows.iter().any(|r| matches!(r, Row::DormantDivider)));
    }

    #[test]
    fn navigation_skips_divider_and_header() {
        let mut app = App::new(snap(
            vec![pane("%1", "alpha", "claude")],
            vec![dormant("dorm", "default")],
        ));
        // After construction, snap_to_selectable lands on the pane row (idx 1).
        assert_eq!(app.selected, 1);
        app.move_down();
        // Should jump straight to the dormant row, skipping the divider at idx 2.
        match &app.rows[app.selected] {
            Row::Dormant(d) => assert_eq!(d.name, "dorm"),
            _ => panic!("move_down should land on dormant row, got {:?}", app.rows[app.selected]),
        }
        app.move_up();
        match &app.rows[app.selected] {
            Row::Pane(p) => assert_eq!(p.pane_id, "%1"),
            _ => panic!("move_up should return to pane row"),
        }
    }

    #[test]
    fn selected_dormant_returns_correct_entry() {
        let mut app = App::new(snap(
            vec![],
            vec![dormant("foo", "default"), dormant("bar", "python")],
        ));
        // alphabetical: bar, foo. Selection lands on first selectable = bar.
        let d = app.selected_dormant().expect("dormant selected");
        assert_eq!(d.name, "bar");
        app.move_down();
        let d = app.selected_dormant().expect("dormant selected");
        assert_eq!(d.name, "foo");
        // Pane API returns None on a dormant row.
        assert!(app.selected_pane().is_none());
    }

    #[test]
    fn reload_preserves_dormant_selection_by_name() {
        let mut app = App::new(snap(
            vec![],
            vec![dormant("apple", "d"), dormant("banana", "d"), dormant("cherry", "d")],
        ));
        // Move selection to "banana".
        app.move_down();
        assert_eq!(app.selected_dormant().unwrap().name, "banana");

        // New snapshot reorders dormant entries; reload should still find banana.
        app.reload(snap(
            vec![],
            vec![dormant("banana", "d"), dormant("apple", "d"), dormant("zzz", "d")],
        ));
        // After reload, snapshot is sorted by row builder (which preserves the
        // order from compute_dormant — but the App stores raw input here).
        // We expect banana to remain selected by name.
        assert_eq!(app.selected_dormant().unwrap().name, "banana");
    }

    #[test]
    fn filter_dormant_matches_by_name_or_base() {
        let pool = vec![
            dormant("frontend-rebuild", "node"),
            dormant("backend-spike", "python"),
            dormant("docs-cleanup", "default"),
        ];
        let by_name = filter_dormant(&pool, "front");
        assert_eq!(by_name.len(), 1);
        assert_eq!(by_name[0].name, "frontend-rebuild");

        let by_base = filter_dormant(&pool, "python");
        assert_eq!(by_base.len(), 1);
        assert_eq!(by_base[0].name, "backend-spike");

        let nothing = filter_dormant(&pool, "nonexistent-xyz");
        assert!(nothing.is_empty());

        let empty_filter = filter_dormant(&pool, "");
        assert_eq!(empty_filter.len(), 3);
    }
}
