//! `aw switch` — pane-centric quick switcher.
//!
//! `aw dash` answers "what is every agent doing, across every workspace?".
//! This answers a narrower question: "which agent do I want to be looking at
//! right now?". So it drops the workspace grouping, the preview, and every
//! control key, and shows one card per *pane* that has seen agent activity
//! recently, most recent first. Press a card's number to land there.
//!
//! Everything rendered comes from [`Snapshot::load`] — the same loader the
//! dashboard uses. No new state files, no extra tmux queries.
//!
//! Cards carry no borders: the hierarchy is typographic (bold label, dim
//! metadata, colored status glyph and workspace) plus a blank row between
//! cards. The layout adapts to the popup it lands in — see [`Metrics`].

use std::io::stdout;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    cursor::{Hide, Show},
    event::{poll, read, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame, Terminal,
};

use serde::Serialize;

use crate::dash::render::{humanize_age, status_glyph};
use crate::dash::state::{PaneState, Snapshot, Status};
use crate::dash::tmux;

/// A pane quiet for longer than this is not "active" any more.
///
/// A rolling window rather than "since local midnight": the repo carries no
/// date library on purpose (see `workspace::create::format_unix`), so a
/// calendar cut-off would need the local UTC offset — and it would blank the
/// view at 00:05 for work done at 23:50.
const WINDOW_SECS: u64 = 86_400;

/// Rows one card occupies: headline, byline, and the blank row under it.
const CARD_ROWS: u16 = 3;

/// Cards stop widening past this so lines stay readable in a big popup.
const MAX_CARD_WIDTH: u16 = 80;

/// How many cards get a number hotkey. Past this, `j`/`k` + Enter.
const HOTKEYS: usize = 9;

/// Columns for the `" 1 "` hotkey gutter + status glyph + the gap after it.
const PREFIX_COLS: u16 = 6;

/// Columns reserved for the right-aligned age, including one leading space.
const AGE_COLS: u16 = 5;

/// Below this height the header and footer are dropped to make room for
/// cards; below [`CARD_ROWS`] we still always draw one card.
const CHROME_MIN_HEIGHT: u16 = 8;

/// Agent panes worth offering as a jump target, best first.
///
/// Two groups, in this order:
///
///  1. panes with agent activity inside [`WINDOW_SECS`], most recent first;
///  2. live agent panes with no recorded activity at all.
///
/// The second group is the whole reason this isn't a one-line filter. A
/// session `aw resurrect` restored fires no hook until somebody types in it,
/// so it has no activity to sort by — but it is a live agent holding a real
/// conversation, and refusing to list it makes the picker useless for exactly
/// the sessions you most want to get back to. They sort last, since "no idea
/// when" should not outrank "two minutes ago".
///
/// A pane counts as an agent pane when [`PaneState::agent_known`] is set: a
/// hook told us, or the pane carries our `@aw_agent` stamp. Checking `agent`
/// itself would not do — it falls back to the tmux label, so a plain shell
/// would claim to be an agent called "zsh".
///
/// Parked panes are the user saying "set this aside", the opposite of "offer
/// it to me".
pub fn active_panes(entries: &[PaneState], now: u64) -> Vec<PaneState> {
    let mut out: Vec<PaneState> = entries
        .iter()
        .filter(|p| !p.parked && p.agent_known && !stale(p, now))
        .cloned()
        .collect();
    // Known activity first, then unknown; within each, newest first. Pane id
    // breaks ties so the order never flickers between reloads.
    out.sort_by(|a, b| {
        let known = (a.last_activity != 0, b.last_activity != 0);
        known
            .1
            .cmp(&known.0)
            .then_with(|| b.last_activity.cmp(&a.last_activity))
            .then_with(|| a.pane_id.cmp(&b.pane_id))
    });
    out
}

/// Has this pane's last known activity aged out of the window?
///
/// `last_activity == 0` means "never recorded", which is unknown rather than
/// old, so it is never stale — that is the restored-session case.
fn stale(p: &PaneState, now: u64) -> bool {
    p.last_activity != 0 && now.saturating_sub(p.last_activity) >= WINDOW_SECS
}

/// How the current terminal size is spent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metrics {
    /// Cards that fit in the card region.
    pub visible: usize,
    /// Card column width (capped, so a wide popup doesn't stretch lines).
    pub width: u16,
    /// Left inset that centers the card column.
    pub left: u16,
    /// Whether there is room for the header and footer.
    pub chrome: bool,
}

impl Metrics {
    pub fn for_area(area: Rect) -> Self {
        let chrome = area.height >= CHROME_MIN_HEIGHT;
        // header (1) + blank (1) + footer (1) + blank (1)
        let reserved = if chrome { 4 } else { 0 };
        let card_rows = area.height.saturating_sub(reserved);
        // n cards need 3n - 1 rows (the last one drops its trailing blank).
        let visible = (((card_rows + 1) / CARD_ROWS) as usize).max(1);
        let width = area.width.min(MAX_CARD_WIDTH);
        Self {
            visible,
            width,
            left: (area.width.saturating_sub(width)) / 2,
            chrome,
        }
    }

    /// Columns the label (and the byline under it) may occupy.
    pub fn text_cols(&self) -> usize {
        self.width.saturating_sub(PREFIX_COLS + AGE_COLS) as usize
    }
}

/// Which card the cursor is on, and the scroll window around it.
struct Switcher {
    cards: Vec<PaneState>,
    cursor: usize,
}

impl Switcher {
    fn new(snap: &Snapshot) -> Self {
        Self {
            cards: active_panes(&snap.entries, crate::dash::state::now_epoch()),
            cursor: 0,
        }
    }

    /// Refresh from a new snapshot, keeping the cursor on the *same pane*
    /// rather than the same index — the list re-sorts as agents work, and
    /// the selection must not slide under the user's fingers.
    fn reload(&mut self, snap: &Snapshot) {
        let anchor = self.cards.get(self.cursor).map(|p| p.pane_id.clone());
        self.cards = active_panes(&snap.entries, crate::dash::state::now_epoch());
        self.cursor = anchor
            .and_then(|id| self.cards.iter().position(|p| p.pane_id == id))
            .unwrap_or(0)
            .min(self.cards.len().saturating_sub(1));
    }

    fn move_by(&mut self, delta: isize) {
        if self.cards.is_empty() {
            return;
        }
        let last = self.cards.len() - 1;
        self.cursor = match delta {
            d if d < 0 => self.cursor.saturating_sub(d.unsigned_abs()),
            d => (self.cursor + d as usize).min(last),
        };
    }

    /// First card index to draw so the cursor stays on screen.
    fn scroll_offset(&self, visible: usize) -> usize {
        if self.cursor < visible {
            return 0;
        }
        self.cursor + 1 - visible
    }

    fn selected_pane(&self) -> Option<String> {
        self.cards.get(self.cursor).map(|p| p.pane_id.clone())
    }
}

enum Exit {
    Quit,
    Jump(String),
}

/// `aw switch` — draw the switcher, jump where the user points, return.
pub fn run() -> Result<()> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, Hide)?;

    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;
    let result = switch_loop(&mut terminal);

    // Always restore the terminal, even if the loop errored.
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen, Show).ok();
    terminal.show_cursor().ok();

    if let Exit::Jump(pane) = result? {
        tmux::switch_to_pane(&pane);
    }
    Ok(())
}

fn switch_loop<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>) -> Result<Exit> {
    let mut sw = Switcher::new(&Snapshot::load()?);
    let mut last_reload = Instant::now();

    loop {
        terminal.draw(|f| render(f, &sw))?;

        // Poll rather than block so the ages stay current and a pane that
        // just went `waiting` shows up without a keystroke.
        if poll(Duration::from_millis(250))? {
            if let Event::Key(key) = read()? {
                match on_key(&mut sw, key) {
                    Some(exit) => return Ok(exit),
                    None => {}
                }
            }
        }

        if last_reload.elapsed() >= Duration::from_millis(500) {
            sw.reload(&Snapshot::load()?);
            last_reload = Instant::now();
        }
    }
}

/// The whole keymap. `Some(_)` closes the switcher.
fn on_key(sw: &mut Switcher, key: KeyEvent) -> Option<Exit> {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Exit::Quit),
        KeyCode::Char('q') | KeyCode::Esc => Some(Exit::Quit),
        KeyCode::Char('j') | KeyCode::Down => {
            sw.move_by(1);
            None
        }
        KeyCode::Char('k') | KeyCode::Up => {
            sw.move_by(-1);
            None
        }
        KeyCode::Enter => sw.selected_pane().map(Exit::Jump),
        // `1`..`9` jump straight to that card. Indexes the sorted list, not
        // the scroll window, so the digits mean the same thing after a
        // scroll as before it.
        KeyCode::Char(c @ '1'..='9') => {
            let idx = c as usize - '1' as usize;
            sw.cards.get(idx).map(|p| Exit::Jump(p.pane_id.clone()))
        }
        _ => None,
    }
}

fn render(f: &mut Frame, sw: &Switcher) {
    let area = f.area();
    let m = Metrics::for_area(area);

    if sw.cards.is_empty() {
        render_empty(f, area, &m);
        return;
    }

    let mut y = area.y;
    if m.chrome {
        draw_line(f, header_line(sw), area.x + m.left, y, m.width);
        y += 2;
    }

    let offset = sw.scroll_offset(m.visible);
    let end = (offset + m.visible).min(sw.cards.len());
    for (row, i) in (offset..end).enumerate() {
        let card_y = y + row as u16 * CARD_ROWS;
        let (headline, byline) = card_lines(&sw.cards[i], i, i == sw.cursor, &m);
        draw_line(f, headline, area.x + m.left, card_y, m.width);
        draw_line(f, byline, area.x + m.left, card_y + 1, m.width);
    }

    if m.chrome {
        let hidden = sw.cards.len() - end + offset;
        let footer_y = area.y + area.height - 1;
        draw_line(f, footer_line(hidden), area.x + m.left, footer_y, m.width);
    }
}

/// `aw switch   3 waiting · 5 idle` — the counts are the reason to look.
fn header_line(sw: &Switcher) -> Line<'static> {
    let mut working = 0usize;
    let mut waiting = 0usize;
    let mut idle = 0usize;
    for c in &sw.cards {
        match c.status {
            Status::Working => working += 1,
            Status::Waiting => waiting += 1,
            Status::Idle => idle += 1,
        }
    }
    // No "last 24h" label any more: the list is every live agent pane, and
    // only a pane we *know* has been quiet for longer than that is left out.
    let mut spans = vec![Span::styled(
        "active agents",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )];
    for (n, label, color) in [
        (waiting, "waiting", Color::Red),
        (working, "working", Color::Yellow),
        (idle, "idle", Color::Green),
    ] {
        if n > 0 {
            spans.push(Span::raw("   "));
            spans.push(Span::styled(
                format!("{} {}", n, label),
                Style::default().fg(color),
            ));
        }
    }
    Line::from(spans)
}

fn footer_line(hidden: usize) -> Line<'static> {
    let hint = if hidden > 0 {
        format!("1-9 jump · j/k select · enter jump · q quit        +{} more", hidden)
    } else {
        "1-9 jump · j/k select · enter jump · q quit".to_string()
    };
    Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray)))
}

/// A card's two lines.
///
/// ```text
/// ▌1 <glyph>  Claude Code                                  2m
///             claude · team
/// ```
///
/// No border, so the grouping comes from the blank row below and from the
/// byline being indented to the label's column.
fn card_lines(
    p: &PaneState,
    index: usize,
    selected: bool,
    m: &Metrics,
) -> (Line<'static>, Line<'static>) {
    let cols = m.text_cols();

    // Hotkey gutter doubles as the selection marker, so selecting a card
    // costs no extra columns.
    let marker = if selected { "▌" } else { " " };
    let hotkey = if index < HOTKEYS {
        format!("{}{} ", marker, index + 1)
    } else {
        format!("{}  ", marker)
    };

    let glyph_style = match p.status {
        Status::Working => Style::default().fg(Color::Yellow),
        Status::Waiting => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        Status::Idle => Style::default().fg(Color::Green),
    };

    let name = card_name(p);
    let headline = Line::from(vec![
        Span::styled(
            hotkey,
            if selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            },
        ),
        Span::styled(status_glyph(p.status).to_string(), glyph_style),
        Span::raw("  "),
        Span::styled(
            pad(&truncate(name, cols), cols),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{:>4}", humanize_age(p.last_activity)),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    // Byline: agent type is the dimmest (you mostly know it), the workspace
    // gets the accent colour because it answers "which project is this?".
    let agent_cols = p.agent.chars().count().min(cols);
    let ws_cols = cols.saturating_sub(agent_cols + 3);
    let workspace = truncate(&p.workspace, ws_cols);
    let byline = Line::from(vec![
        Span::raw(" ".repeat(PREFIX_COLS as usize)),
        Span::styled(truncate(&p.agent, cols), Style::default().fg(Color::DarkGray)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            pad(&workspace, ws_cols),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw(" ".repeat(AGE_COLS as usize)),
    ]);

    if selected {
        let bg = Style::default().bg(Color::Rgb(38, 42, 54));
        return (headline.style(bg), byline.style(bg));
    }
    (headline, byline)
}

fn render_empty(f: &mut Frame, area: Rect, m: &Metrics) {
    let mut y = area.y;
    if m.chrome {
        draw_line(
            f,
            Line::from(Span::styled(
                "active agents",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            area.x + m.left,
            y,
            m.width,
        );
        y += 2;
    }
    for (i, text) in [
        "No agent sessions are running.",
        "`aw dash` also lists workspaces with no session.",
    ]
    .iter()
    .enumerate()
    {
        draw_line(
            f,
            Line::from(Span::styled(*text, Style::default().fg(Color::DarkGray))),
            area.x + m.left,
            y + i as u16,
            m.width,
        );
    }
}

fn draw_line(f: &mut Frame, line: Line<'static>, x: u16, y: u16, width: u16) {
    let area = Rect { x, y, width, height: 1 };
    if area.bottom() <= f.area().bottom() {
        f.render_widget(Paragraph::new(line), area);
    }
}

/// Truncate to `cols` display columns, ellipsising when it doesn't fit.
fn truncate(s: &str, cols: usize) -> String {
    if s.chars().count() <= cols {
        return s.to_string();
    }
    if cols == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(cols.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Right-pad to `cols` so a selected card's highlight spans the full width.
fn pad(s: &str, cols: usize) -> String {
    let len = s.chars().count();
    if len >= cols {
        return s.to_string();
    }
    let mut out = s.to_string();
    out.push_str(&" ".repeat(cols - len));
    out
}

/// The card's headline: the tmux-derived pane name, falling back to the agent
/// when tmux is unreachable and the row came from a state file alone.
///
/// `label` is refreshed from tmux on every load, so a `/rename`'d Claude
/// session shows the name it chose. One function so the TUI and
/// `aw switch --json` can never disagree about what a pane is called.
pub fn card_name(p: &PaneState) -> &str {
    if p.label.is_empty() {
        p.agent.as_str()
    } else {
        p.label.as_str()
    }
}

/// One row of `aw switch --json`, for external selectors (the Hammerspoon
/// menu).
///
/// `PaneState` can't be serialized directly for this: its `label`, `parked`
/// and `pinned` fields are `#[serde(skip)]` because they are recomputed from
/// tmux on every load, so the pane name would come out empty.
#[derive(Debug, Serialize)]
pub struct Entry {
    pub pane_id: String,
    /// tmux session (`aw-<workspace>`). With `set-titles-string "#S"` this is
    /// also the terminal window title, which is how a GUI caller finds the
    /// window hosting it.
    pub session: String,
    pub workspace: String,
    pub agent: String,
    /// Pane name as the card shows it.
    pub name: String,
    pub status: Status,
    pub last_activity: u64,
    /// Seconds since the last agent event, so a caller need not share our clock.
    pub age_secs: u64,
    /// The same short string the cards print ("2m", "16h").
    pub age: String,
}

/// `aw switch --json` — the switcher's list as JSON, same filter and order.
pub fn cmd_json() -> Result<()> {
    let snap = Snapshot::load()?;
    let now = crate::dash::state::now_epoch();
    let rows: Vec<Entry> = active_panes(&snap.entries, now)
        .iter()
        .map(|p| Entry {
            pane_id: p.pane_id.clone(),
            session: p.session.clone(),
            workspace: p.workspace.clone(),
            agent: p.agent.clone(),
            name: card_name(p).to_string(),
            status: p.status,
            last_activity: p.last_activity,
            age_secs: now.saturating_sub(p.last_activity),
            age: humanize_age(p.last_activity),
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    /// `humanize_age` reads the real clock, so test panes are anchored to it
    /// too — a synthetic epoch renders every age as "0s".
    fn now() -> u64 {
        crate::dash::state::now_epoch()
    }

    fn pane(id: &str, ws: &str, agent: &str, age_secs: u64, status: Status) -> PaneState {
        PaneState {
            schema_version: 1,
            pane_id: id.into(),
            session: format!("aw-{}", ws),
            workspace: ws.into(),
            cwd: format!("/tmp/{}", ws),
            agent: agent.into(),
            status,
            last_event: String::new(),
            last_activity: now() - age_secs,
            last_prompt: String::new(),
            session_id: String::new(),
            parked: false,
            label: String::new(),
            pinned: false,
            agent_known: true,
        }
    }

    #[test]
    fn active_panes_keeps_recent_sorted_newest_first() {
        let entries = vec![
            pane("%1", "alpha", "claude", 7200, Status::Idle),
            pane("%2", "beta", "codex", 60, Status::Working),
            pane("%3", "gamma", "claude", 90_000, Status::Idle), // > 24h
        ];
        let out = active_panes(&entries, now());
        let ids: Vec<&str> = out.iter().map(|p| p.pane_id.as_str()).collect();
        assert_eq!(ids, vec!["%2", "%1"], "stale pane must drop out");
    }

    #[test]
    fn active_panes_drops_parked_and_shells_but_keeps_never_active_agents() {
        let mut parked = pane("%1", "alpha", "claude", 60, Status::Idle);
        parked.parked = true;
        // A restored agent: live, real conversation, no hook has fired yet.
        let mut restored = pane("%2", "beta", "claude", 0, Status::Idle);
        restored.last_activity = 0;
        // A plain shell: no agent, so nothing to jump to.
        let mut shell = pane("%4", "delta", "zsh", 0, Status::Idle);
        shell.last_activity = 0;
        shell.agent_known = false;   // label-derived, not a real agent
        let fresh = pane("%3", "gamma", "claude", 60, Status::Idle);
        let out = active_panes(&[parked, restored, shell, fresh], now());
        let ids: Vec<&str> = out.iter().map(|p| p.pane_id.as_str()).collect();
        assert_eq!(ids, vec!["%3", "%2"], "known activity first, restored still listed");
    }

    /// The bug this guards: a session restored by `aw resurrect` was missing
    /// from the picker entirely, because it had no activity to filter on.
    #[test]
    fn restored_agent_is_listed_even_though_nothing_has_aged() {
        let mut restored = pane("%17", "video-editing", "claude", 0, Status::Idle);
        restored.last_activity = 0;
        let out = active_panes(&[restored], now());
        assert_eq!(out.len(), 1, "a live agent must be offered as a jump target");
        assert_eq!(out[0].pane_id, "%17");
    }

    /// Unknown age must not be mistaken for "very old" and dropped.
    #[test]
    fn stale_only_applies_to_panes_with_a_recorded_time() {
        let n = now();
        let mut never = pane("%1", "a", "claude", 0, Status::Idle);
        never.last_activity = 0;
        assert!(!stale(&never, n), "never-recorded is unknown, not stale");
        let old = pane("%2", "a", "claude", WINDOW_SECS + 60, Status::Idle);
        assert!(stale(&old, n));
        let recent = pane("%3", "a", "claude", 60, Status::Idle);
        assert!(!stale(&recent, n));
    }

    #[test]
    fn active_panes_breaks_ties_on_pane_id() {
        let a = pane("%9", "alpha", "claude", 60, Status::Idle);
        let b = pane("%2", "beta", "claude", 60, Status::Idle);
        let out = active_panes(&[a, b], now());
        let ids: Vec<&str> = out.iter().map(|p| p.pane_id.as_str()).collect();
        assert_eq!(ids, vec!["%2", "%9"]);
    }

    #[test]
    fn metrics_fit_cards_to_height_and_cap_width() {
        // 3 cards need 8 rows; + 4 rows of chrome = 12.
        let m = Metrics::for_area(Rect { x: 0, y: 0, width: 100, height: 12 });
        assert!(m.chrome);
        assert_eq!(m.visible, 3);
        // Width caps at MAX_CARD_WIDTH and centers.
        assert_eq!(m.width, MAX_CARD_WIDTH);
        assert_eq!(m.left, 10);

        // A narrow popup uses its full width, no inset.
        let narrow = Metrics::for_area(Rect { x: 0, y: 0, width: 50, height: 12 });
        assert_eq!(narrow.width, 50);
        assert_eq!(narrow.left, 0);
    }

    #[test]
    fn metrics_drop_chrome_when_short_and_always_keep_one_card() {
        let m = Metrics::for_area(Rect { x: 0, y: 0, width: 80, height: 4 });
        assert!(!m.chrome, "no room for header + footer");
        assert_eq!(m.visible, 1);

        let tiny = Metrics::for_area(Rect { x: 0, y: 0, width: 80, height: 1 });
        assert_eq!(tiny.visible, 1, "never zero cards");
    }

    fn render_to_string(sw: &Switcher, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render(f, sw)).unwrap();
        let buf = term.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..h {
            let mut line = String::new();
            for x in 0..w {
                line.push_str(buf.cell((x, y)).unwrap().symbol());
            }
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }

    fn switcher(cards: Vec<PaneState>) -> Switcher {
        Switcher { cards, cursor: 0 }
    }

    #[test]
    fn renders_one_card_per_pane_with_hotkey_agent_and_workspace() {
        let mut first = pane("%1", "team", "claude", 120, Status::Waiting);
        first.label = "Claude Code".into();
        let mut second = pane("%2", "video-editing", "codex", 3600, Status::Working);
        second.label = "general-purpose".into();
        let sw = switcher(vec![first, second]);
        let out = render_to_string(&sw, 80, 14);
        assert!(out.contains("active agents"), "header missing:\n{}", out);
        assert!(out.contains("1 "), "hotkey 1 missing:\n{}", out);
        assert!(out.contains("2 "), "hotkey 2 missing:\n{}", out);
        assert!(out.contains("team"), "workspace missing:\n{}", out);
        assert!(out.contains("video-editing"), "workspace missing:\n{}", out);
        assert!(out.contains("claude"), "agent missing:\n{}", out);
        assert!(out.contains("codex"), "agent missing:\n{}", out);
        assert!(out.contains("Claude Code"), "tmux label missing:\n{}", out);
        assert!(out.contains("2m"), "age missing:\n{}", out);
        assert!(out.contains("1h"), "age missing:\n{}", out);
        assert!(out.contains("1 waiting"), "counts missing:\n{}", out);
    }

    #[test]
    fn empty_state_explains_itself() {
        let out = render_to_string(&switcher(vec![]), 80, 14);
        assert!(out.contains("No agent sessions are running"), "{}", out);
        assert!(out.contains("aw dash"), "points at the full view:\n{}", out);
    }

    #[test]
    fn scroll_follows_the_cursor_and_footer_counts_the_rest() {
        let cards: Vec<PaneState> = (1..=6)
            .map(|i| pane(&format!("%{}", i), "ws", "claude", i * 60, Status::Idle))
            .collect();
        let mut sw = switcher(cards);
        // 12 rows of chrome+cards fits 3 cards; select the 5th.
        sw.cursor = 4;
        assert_eq!(sw.scroll_offset(3), 2, "window slides to keep cursor shown");
        let out = render_to_string(&sw, 80, 12);
        assert!(out.contains("+3 more"), "hidden count missing:\n{}", out);
    }

    #[test]
    fn reload_keeps_the_cursor_on_the_same_pane() {
        let mut sw = switcher(vec![
            pane("%1", "alpha", "claude", 60, Status::Idle),
            pane("%2", "beta", "claude", 120, Status::Idle),
        ]);
        sw.cursor = 1; // on %2
        // %2 becomes the most recent, so the list order flips.
        let snap = Snapshot {
            entries: vec![
                pane("%1", "alpha", "claude", 300, Status::Idle),
                pane("%2", "beta", "claude", 5, Status::Idle),
            ],
            dormant: vec![],
        };
        sw.reload(&snap);
        assert_eq!(
            sw.cards[sw.cursor].pane_id, "%2",
            "cursor must track the pane, not the index"
        );
    }

    #[test]
    fn number_key_jumps_to_that_card_and_q_quits() {
        let mut sw = switcher(vec![
            pane("%7", "alpha", "claude", 60, Status::Idle),
            pane("%8", "beta", "claude", 120, Status::Idle),
        ]);
        let press = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);

        match on_key(&mut sw, press('2')) {
            Some(Exit::Jump(pane)) => assert_eq!(pane, "%8"),
            _ => panic!("'2' should jump to the second card"),
        }
        // A digit with no card behind it does nothing.
        assert!(on_key(&mut sw, press('9')).is_none());
        assert!(matches!(on_key(&mut sw, press('q')), Some(Exit::Quit)));
    }

    #[test]
    fn j_k_move_the_cursor_and_stop_at_the_ends() {
        let mut sw = switcher(vec![
            pane("%1", "a", "claude", 60, Status::Idle),
            pane("%2", "b", "claude", 120, Status::Idle),
        ]);
        let press = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        on_key(&mut sw, press('k'));
        assert_eq!(sw.cursor, 0, "already at the top");
        on_key(&mut sw, press('j'));
        assert_eq!(sw.cursor, 1);
        on_key(&mut sw, press('j'));
        assert_eq!(sw.cursor, 1, "clamped at the bottom");
        match on_key(&mut sw, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Some(Exit::Jump(pane)) => assert_eq!(pane, "%2"),
            _ => panic!("enter should jump to the selection"),
        }
    }

    #[test]
    fn long_names_truncate_instead_of_spilling() {
        let mut p = pane("%1", "a-very-long-workspace-name-indeed", "claude", 60, Status::Idle);
        p.label = "an extremely long tmux window name that will not fit".into();
        let out = render_to_string(&switcher(vec![p]), 44, 9);
        for line in out.lines() {
            assert!(
                line.chars().count() <= 44,
                "line overflowed the 44-col popup: {:?}",
                line
            );
        }
        assert!(out.contains('…'), "expected an ellipsis:\n{}", out);
    }
}
