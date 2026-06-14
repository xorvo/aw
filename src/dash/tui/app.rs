//! TUI state machine.
//!
//! The visible "rows" are flattened from the snapshot — workspace headers
//! interleaved with pane rows. Headers are not selectable; `selected` always
//! lands on a pane.

use std::cell::Cell;

use crate::dash::state::{DormantWorkspace, PaneState, Snapshot};

/// One displayable line.
#[derive(Debug, Clone)]
pub enum Row {
    /// `▾ <workspace>   aw-<workspace>` (or `▸ ...` collapsed).
    Header { workspace: String, session_hint: String, collapsed: bool, pinned: bool },
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
    /// Active when `app.create` is `Some`. The form state lives there so
    /// `Mode` can stay `Copy`.
    Create,
    /// Phone-pairing QR overlay. Active when `app.qr` is `Some`.
    Qr,
}

/// State for the phone-pairing overlay (`Q` from Normal mode). Resolved
/// once when the overlay opens — the URL doesn't change while it's up.
#[derive(Debug, Clone)]
pub struct QrOverlay {
    /// The pairing URL, shown verbatim for copy/typing. `None` when
    /// resolution failed (e.g. no home dir / no urandom).
    pub url: Option<String>,
    /// Pre-rendered half-block QR lines; empty when `url` is `None` or
    /// encoding failed (the URL line still renders).
    pub lines: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateField {
    Name,
    Base,
}

/// State for the in-popup "new workspace" form. Created when the user
/// presses `c` from Normal mode; cleared on Esc or successful submit.
#[derive(Debug, Clone)]
pub struct CreateForm {
    pub name: String,
    /// Bases pulled from the config when the form opened. Empty list is
    /// a real possibility (fresh install) — the form should still render
    /// and call it out instead of letting submit no-op.
    pub bases: Vec<String>,
    pub base_idx: usize,
    /// Names of workspaces already on disk, for conflict validation.
    pub existing: std::collections::HashSet<String>,
    pub field: CreateField,
    /// Transient validation message shown below the form. Set on a failed
    /// submit; cleared on the next keystroke.
    pub error: Option<String>,
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
    /// Create a brand-new workspace from a base, then open it.
    CreateWorkspace { name: String, base: String },
    /// Toggle the pinned sentinel for a workspace.
    TogglePin(String),
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
    /// First displayed line in the Agents pane (counted in rendered lines,
    /// including blank separators between groups — not raw row indices).
    /// Updated by the renderer to keep the selection in view; held in a
    /// `Cell` so the immutable `&App` borrow in `view::render` can mutate it.
    pub scroll_offset: Cell<u16>,
    /// Cached snapshot — for `apply()` resolving Jump/Park targets.
    pub snapshot: Snapshot,
    /// In-popup workspace-creation form. `Some` iff `mode == Mode::Create`.
    pub create: Option<CreateForm>,
    /// Phone-pairing QR overlay. `Some` iff `mode == Mode::Qr`.
    pub qr: Option<QrOverlay>,
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
            scroll_offset: Cell::new(0),
            snapshot: snap,
            create: None,
            qr: None,
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
            // Pin is workspace-level; every pane in the group shares it,
            // so the first one is authoritative.
            let pinned = panes.first().map(|p| p.pinned).unwrap_or(false);
            rows.push(Row::Header {
                workspace: workspace.clone(),
                session_hint,
                collapsed,
                pinned,
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

    /// Enter the workspace-creation form. Loads bases from the config and
    /// existing workspace names from disk for conflict validation. Both
    /// reads are best-effort: an empty config or missing dir just means
    /// the form shows up with no bases or no conflicts, never an error.
    pub fn enter_create(&mut self) {
        let bases: Vec<String> = match crate::paths::Paths::from_env()
            .ok()
            .and_then(|p| crate::config::Config::load(&p.config_file).ok())
        {
            Some(cfg) => cfg.base_names().into_iter().map(String::from).collect(),
            None => Vec::new(),
        };
        let existing: std::collections::HashSet<String> =
            crate::workspace::listing::enumerate_workspaces()
                .into_iter()
                .map(|m| m.name)
                .collect();
        self.create = Some(CreateForm {
            name: String::new(),
            bases,
            base_idx: 0,
            existing,
            field: CreateField::Name,
            error: None,
        });
        self.mode = Mode::Create;
    }

    pub fn exit_create(&mut self) {
        self.create = None;
        self.mode = Mode::Normal;
    }

    /// Open the phone-pairing overlay. Resolving the URL touches disk
    /// (token cache) and does a route lookup, so it happens once here
    /// rather than per-frame; failures land in `QrOverlay::error` and
    /// render inside the overlay instead of killing the TUI.
    pub fn enter_qr(&mut self) {
        let overlay = match crate::dash::remote_link::pairing_url() {
            Ok(url) => {
                let lines =
                    crate::dash::remote_link::qr_lines(&url).unwrap_or_default();
                QrOverlay { url: Some(url), lines, error: None }
            }
            Err(e) => QrOverlay { url: None, lines: Vec::new(), error: Some(e.to_string()) },
        };
        self.qr = Some(overlay);
        self.mode = Mode::Qr;
    }

    pub fn exit_qr(&mut self) {
        self.qr = None;
        self.mode = Mode::Normal;
    }

    /// Mutate the form via the supplied closure; clears any prior error
    /// (a keystroke counts as "user is fixing the problem").
    pub fn with_create<F: FnOnce(&mut CreateForm)>(&mut self, f: F) {
        if let Some(ref mut form) = self.create {
            form.error = None;
            f(form);
        }
    }

    /// Validate the form and produce a `CreateWorkspace` action on
    /// success. On failure, store the error on the form (rendered below
    /// the inputs) and return `Action::Continue`.
    pub fn submit_create(&mut self) -> Action {
        let (name, base) = match self.create.as_ref() {
            Some(f) => {
                let name = f.name.trim().to_string();
                if name.is_empty() {
                    return self.fail_create("name cannot be empty");
                }
                if name.contains('/') || name.contains(' ') {
                    return self.fail_create("name cannot contain '/' or spaces");
                }
                if f.existing.contains(&name) {
                    return self.fail_create(&format!("workspace '{}' already exists", name));
                }
                let base = match f.bases.get(f.base_idx) {
                    Some(b) => b.clone(),
                    None => return self.fail_create("no bases configured; run `aw init` first"),
                };
                (name, base)
            }
            None => return Action::Continue,
        };
        // Clear the form before yielding the action; handle_exit_action
        // will tear down the popup either way.
        self.create = None;
        self.mode = Mode::Normal;
        Action::CreateWorkspace { name, base }
    }

    fn fail_create(&mut self, msg: &str) -> Action {
        if let Some(ref mut f) = self.create {
            f.error = Some(msg.to_string());
        }
        Action::Continue
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
            Action::TogglePin(workspace) => {
                let pinned_dir = match crate::dash::pinned_dir() {
                    Ok(p) => p,
                    Err(_) => return None,
                };
                let _ = std::fs::create_dir_all(&pinned_dir);
                // Workspace names disallow `/` (validated on create), but
                // mirror the path-sanitization the CLI uses, just in case.
                let safe = workspace.replace('/', "_");
                let s = pinned_dir.join(&safe);
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
            other => Some(other),
        }
    }
}

fn is_selectable(row: &Row) -> bool {
    matches!(row, Row::Pane(_) | Row::Dormant(_))
}

/// Whether a blank separator line is rendered before `curr` to give
/// groups vertical breathing room. Single source of truth used by both
/// the line builder in `view::render_list` and the scroll math.
pub fn injects_blank_before(prev_was_selectable: bool, curr: &Row) -> bool {
    prev_was_selectable && matches!(curr, Row::Header { .. } | Row::DormantDivider)
}

/// Index of the rendered line corresponding to `app.rows[target]`,
/// counting injected blank separators. Returns 0 if `target` is out of
/// range.
pub fn displayed_line_index(rows: &[Row], target: usize) -> u16 {
    let mut line: u16 = 0;
    let mut prior_selectable = false;
    for (i, row) in rows.iter().enumerate() {
        if injects_blank_before(prior_selectable, row) {
            line = line.saturating_add(1);
        }
        if i == target {
            return line;
        }
        line = line.saturating_add(1);
        prior_selectable = matches!(row, Row::Pane(_) | Row::Dormant(_));
    }
    line.saturating_sub(1)
}

/// Total number of rendered lines in the Agents pane (rows + injected
/// blanks). Used to clamp scroll offset on row-list shrink.
pub fn total_displayed_lines(rows: &[Row]) -> u16 {
    let mut line: u16 = 0;
    let mut prior = false;
    for row in rows {
        if injects_blank_before(prior, row) {
            line = line.saturating_add(1);
        }
        line = line.saturating_add(1);
        prior = matches!(row, Row::Pane(_) | Row::Dormant(_));
    }
    line
}

/// Filter dormant workspaces by name/base via the same fuzzy matcher used
/// for live entries. Empty filter returns the input unchanged. Sort order
/// is alphabetical (input is already sorted by `Snapshot::load`).
fn filter_dormant(dormant: &[DormantWorkspace], filter: &str) -> Vec<DormantWorkspace> {
    if filter.trim().is_empty() {
        // Defensive sort matching `compute_dormant`: pinned first, then
        // mtime desc, then name asc. App::new can be constructed with
        // arbitrary input (tests, custom tooling), so we don't rely on
        // the caller's order — and crucially we DON'T strip the
        // pinned-first ordering that compute_dormant has already applied.
        let mut out = dormant.to_vec();
        out.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then_with(|| b.mtime.cmp(&a.mtime))
                .then_with(|| a.name.cmp(&b.name))
        });
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
                // Include both `agent` (claude/codex/pi — so `/codex`
                // matches all codex panes) and `label` (the tmux
                // window_name / pane_title, which carries a `/rename`'d
                // Claude session name).
                let hay = format!(
                    "{} {} {} {} {}",
                    p.workspace, p.agent, p.label, p.last_prompt, p.cwd
                );
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
            label: String::new(),
            pinned: false,
        }
    }

    fn dormant(name: &str, base: &str) -> DormantWorkspace {
        DormantWorkspace {
            name: name.into(),
            base: base.into(),
            created: "2026-03-01T10:00:00Z".into(),
            pinned: false,
            mtime: 0,
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
    fn header_carries_pinned_flag_from_panes() {
        // A pane whose `pinned` is true should yield a Header { pinned: true }.
        let mut pinned_pane = pane("%1", "alpha", "claude");
        pinned_pane.pinned = true;
        let unpinned_pane = pane("%2", "beta", "claude");

        let app = App::new(snap(vec![pinned_pane, unpinned_pane], vec![]));

        let alpha_header = app.rows.iter().find_map(|r| match r {
            Row::Header { workspace, pinned, .. } if workspace == "alpha" => Some(*pinned),
            _ => None,
        });
        let beta_header = app.rows.iter().find_map(|r| match r {
            Row::Header { workspace, pinned, .. } if workspace == "beta" => Some(*pinned),
            _ => None,
        });
        assert_eq!(alpha_header, Some(true), "alpha must surface as pinned");
        assert_eq!(beta_header, Some(false), "beta must surface as unpinned");
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

    #[test]
    fn displayed_line_index_counts_blank_separators() {
        // Layout:
        //   row 0: header alpha           (line 0)
        //   row 1: pane alpha             (line 1)
        //   row 2: header beta            (line 2 + blank before  → line 3)
        //   row 3: pane beta              (line 4)
        //   row 4: divider                (line 5 + blank before  → line 6)
        //   row 5: dormant gamma          (line 7)
        let app = App::new(snap(
            vec![pane("%1", "alpha", "claude"), pane("%2", "beta", "claude")],
            vec![dormant("gamma", "default")],
        ));
        // Sanity-check rebuilt rows.
        assert!(matches!(app.rows[0], Row::Header { .. }));
        assert!(matches!(app.rows[1], Row::Pane(_)));
        assert!(matches!(app.rows[2], Row::Header { .. }));
        assert!(matches!(app.rows[3], Row::Pane(_)));
        assert!(matches!(app.rows[4], Row::DormantDivider));
        assert!(matches!(app.rows[5], Row::Dormant(_)));

        assert_eq!(displayed_line_index(&app.rows, 0), 0); // alpha header
        assert_eq!(displayed_line_index(&app.rows, 1), 1); // alpha pane
        assert_eq!(displayed_line_index(&app.rows, 2), 3); // beta header (after blank)
        assert_eq!(displayed_line_index(&app.rows, 3), 4); // beta pane
        assert_eq!(displayed_line_index(&app.rows, 4), 6); // divider (after blank)
        assert_eq!(displayed_line_index(&app.rows, 5), 7); // gamma dormant

        assert_eq!(total_displayed_lines(&app.rows), 8);
    }

    #[test]
    fn pane_filter_searches_label_renamed_session() {
        // Regression: a hooked Claude pane's `agent` stays as "claude"
        // even after `/rename <something>` updates tmux's window_name.
        // `label` carries the renamed value and must be filter-matched.
        let mut hooked = pane("%1", "alpha", "claude");
        hooked.label = "my-renamed-task".into();
        let mut other = pane("%2", "alpha", "claude");
        other.label = "claude".into();

        let mut app = App::new(snap(vec![hooked.clone(), other.clone()], vec![]));
        for c in "renamed".chars() {
            app.filter_push(c);
        }
        let panes: Vec<&PaneState> = app.rows.iter().filter_map(|r| match r {
            Row::Pane(p) => Some(p),
            _ => None,
        }).collect();
        assert_eq!(panes.len(), 1, "label filter should narrow to one pane");
        assert_eq!(panes[0].pane_id, "%1");
    }

    #[test]
    fn pane_filter_still_matches_by_agent_type() {
        // `/codex` should match codex panes even when label is empty
        // (e.g. file-only fallback when tmux is unreachable).
        let mut a = pane("%1", "alpha", "claude");
        a.label = String::new();
        let mut b = pane("%2", "beta", "codex");
        b.label = String::new();
        let mut app = App::new(snap(vec![a, b], vec![]));
        for c in "codex".chars() {
            app.filter_push(c);
        }
        let panes: Vec<&PaneState> = app.rows.iter().filter_map(|r| match r {
            Row::Pane(p) => Some(p),
            _ => None,
        }).collect();
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].agent, "codex");
    }

    #[test]
    fn pane_filter_searches_cwd() {
        // Two panes with distinguishing cwd substrings; everything else
        // (workspace/agent/last_prompt) is identical so cwd is the only
        // signal in the haystack.
        let mut p1 = pane("%1", "alpha", "claude");
        p1.cwd = "/Users/horvo/work/billing-svc".into();
        let mut p2 = pane("%2", "alpha", "claude");
        p2.cwd = "/Users/horvo/work/marketing-site".into();

        let app = App::new(snap(vec![p1.clone(), p2.clone()], vec![]));
        // Sanity: with no filter both are visible.
        let panes_visible: Vec<&PaneState> = app.rows.iter().filter_map(|r| match r {
            Row::Pane(p) => Some(p),
            _ => None,
        }).collect();
        assert_eq!(panes_visible.len(), 2);

        // Filter by a substring unique to one cwd.
        let mut app = App::new(snap(vec![p1, p2], vec![]));
        for c in "billing".chars() {
            app.filter_push(c);
        }
        let panes_visible: Vec<&PaneState> = app.rows.iter().filter_map(|r| match r {
            Row::Pane(p) => Some(p),
            _ => None,
        }).collect();
        assert_eq!(panes_visible.len(), 1, "cwd filter should narrow to one pane");
        assert_eq!(panes_visible[0].pane_id, "%1");
    }
}
