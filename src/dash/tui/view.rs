//! Render functions for both the popup and the sidebar.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap},
    Frame,
};

use crate::dash::render::{humanize_age, parked_glyph, status_glyph};
use crate::dash::state::{Snapshot, Status};
use crate::dash::tui::app::{App, Mode, Row};

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();
    // Horizontal margin only — pushes the inner panels away from the
    // tmux popup's left/right border without wasting vertical space.
    // Header and footer stay flush against the popup's top/bottom which
    // already have the popup's own border breathing room.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(1)
        .constraints([
            Constraint::Length(1),    // header (status counts)
            Constraint::Min(0),       // body
            Constraint::Length(1),    // footer (keys / filter)
        ])
        .split(area);

    render_header(f, chunks[0], app);
    render_body(f, chunks[1], app);
    render_footer(f, chunks[2], app);
}

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    let (working, waiting, idle) = app.snapshot.counts();
    let mut spans = vec![
        Span::styled(
            "aw dash",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::raw("    "),
    ];
    let g_work = status_glyph(Status::Working);
    let g_wait = status_glyph(Status::Waiting);
    let g_idle = status_glyph(Status::Idle);
    if working > 0 {
        spans.push(Span::styled(
            format!("{} {} working", g_work, working),
            Style::default().fg(Color::Yellow),
        ));
        spans.push(Span::raw("   "));
    }
    if waiting > 0 {
        spans.push(Span::styled(
            format!("{} {} waiting", g_wait, waiting),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw("   "));
    }
    if idle > 0 {
        spans.push(Span::styled(
            format!("{} {} idle", g_idle, idle),
            Style::default().fg(Color::Green),
        ));
    }
    if working == 0 && waiting == 0 && idle == 0 {
        spans.push(Span::styled(
            "no agents tracked",
            Style::default().fg(Color::DarkGray),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_body(f: &mut Frame, area: Rect, app: &App) {
    let chunks = if app.show_preview {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area)
    };

    render_list(f, chunks[0], app);
    render_detail(f, chunks[1], app);
}

fn render_list(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            " Agents ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.rows.is_empty() {
        let empty = Paragraph::new(Line::from(vec![
            Span::styled(
                "No agents tracked yet. ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(
                "Wire hooks via `aw install hooks` and start an agent inside a tmux pane.",
            ),
        ]))
        .wrap(Wrap { trim: true });
        f.render_widget(empty, inner);
        return;
    }

    // Inject blank line before each workspace header (except the first)
    // so groups breathe a little. Using a one-shot index map so the
    // selected-row math stays correct against `app.rows`.
    let mut lines: Vec<Line> = Vec::with_capacity(app.rows.len() + 4);
    let mut prior_was_pane = false;
    for (i, row) in app.rows.iter().enumerate() {
        if matches!(row, Row::Header { .. }) && prior_was_pane {
            lines.push(Line::raw(""));
        }
        lines.push(line_for_row(row, i == app.selected));
        prior_was_pane = matches!(row, Row::Pane(_));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn line_for_row(row: &Row, selected: bool) -> Line<'static> {
    // Selection cue: a left-edge accent bar (▌ in cyan) instead of the
    // ›/space carat. Scans more cleanly because the bar sits in its own
    // single-cell column flush against the block padding.
    let edge = if selected {
        Span::styled("▌", Style::default().fg(Color::Cyan))
    } else {
        Span::raw(" ")
    };
    match row {
        Row::Header { workspace, session_hint, collapsed } => {
            let arrow = if *collapsed { "▸" } else { "▾" };
            Line::from(vec![
                edge,
                Span::raw(" "),
                Span::styled(
                    format!("{} {}", arrow, workspace),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::raw("   "),
                Span::styled(
                    session_hint.clone(),
                    Style::default().fg(Color::DarkGray),
                ),
            ])
        }
        Row::Pane(p) => {
            let glyph = status_glyph(p.status);
            let glyph_style = match p.status {
                Status::Working => Style::default().fg(Color::Yellow),
                Status::Waiting => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                Status::Idle => Style::default().fg(Color::Green),
            };
            let parked = if p.parked {
                Some(Span::styled(
                    format!(" {} parked", parked_glyph()),
                    Style::default().fg(Color::DarkGray),
                ))
            } else {
                None
            };
            let prompt_short = if p.last_prompt.is_empty() {
                String::new()
            } else {
                truncate(&p.last_prompt, 40)
            };
            let mut spans = vec![
                edge,
                Span::raw("   "),
                Span::styled(glyph.to_string(), glyph_style),
                Span::raw("  "),
                Span::styled(
                    format!("{:<7}", p.agent),
                    Style::default().fg(Color::White),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("{:<5}", p.pane_id),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("{:<4}", humanize_age(p.last_activity)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw("  "),
                Span::raw(prompt_short),
            ];
            if let Some(p) = parked {
                spans.push(p);
            }
            let mut line = Line::from(spans);
            if selected {
                line = line.style(Style::default().bg(Color::DarkGray));
            }
            line
        }
    }
}

fn render_detail(f: &mut Frame, area: Rect, app: &App) {
    let title = if app.show_preview { " Preview " } else { " Details " };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            title,
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let pane = match app.selected_pane() {
        Some(p) => p,
        None => return,
    };

    if app.show_preview {
        let content = crate::dash::tui::preview::capture(&pane.pane_id, inner.height.max(20));
        f.render_widget(Paragraph::new(content), inner);
        return;
    }

    let lines = vec![
        kv("pane", &pane.pane_id),
        kv("agent", &pane.agent),
        kv("status", status_label(pane.status)),
        kv("workspace", if pane.workspace.is_empty() { "—" } else { &pane.workspace }),
        kv("session", if pane.session.is_empty() { "—" } else { &pane.session }),
        kv("cwd", if pane.cwd.is_empty() { "—" } else { &pane.cwd }),
        kv("last event", if pane.last_event.is_empty() { "—" } else { &pane.last_event }),
        kv("last activity", &humanize_age(pane.last_activity)),
        kv(
            "last prompt",
            if pane.last_prompt.is_empty() { "—" } else { &pane.last_prompt },
        ),
        kv("parked", if pane.parked { "yes" } else { "no" }),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn kv(k: &str, v: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{:<14}", k),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(v.to_string()),
    ])
}

fn status_label(s: Status) -> &'static str {
    match s {
        Status::Working => "working",
        Status::Waiting => "waiting",
        Status::Idle => "idle",
    }
}

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    let line = match app.mode {
        Mode::Filter => Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(app.filter.clone()),
            Span::raw("  "),
            Span::styled(
                "↵ confirm · esc clear",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Mode::Normal => {
            let key_style = Style::default().fg(Color::Cyan);
            let act_style = Style::default().fg(Color::DarkGray);
            let sep = Span::styled("  ·  ", act_style);
            // Pairs of (key, action). Rendered key↪action then joined by `·`.
            let pairs: &[(&str, &str)] = &[
                ("↵", "jump"),
                ("⇥", "preview"),
                ("/", "filter"),
                ("p", "park"),
                ("n", "next-ready"),
                ("r", "refresh"),
                ("␣", "(un)collapse"),
                ("q", "quit"),
            ];
            let mut spans: Vec<Span> = Vec::with_capacity(pairs.len() * 4);
            for (i, (k, a)) in pairs.iter().enumerate() {
                if i > 0 {
                    spans.push(sep.clone());
                }
                spans.push(Span::styled(k.to_string(), key_style));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(a.to_string(), act_style));
            }
            Line::from(spans)
        }
    };
    f.render_widget(Paragraph::new(line), area);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

// ---- sidebar (text-only, no ratatui) ----

/// Plain-text rendering for the sidebar pane. One workspace block per group,
/// pane lines indented. Width self-trims at ~38 cols.
pub fn render_sidebar_text(snap: &Snapshot) -> String {
    let mut out = String::new();
    let (working, waiting, idle) = snap.counts();
    // One space between glyph and count, two spaces between groups so the
    // columns line up regardless of whether the glyph is rendered as 1 or 2
    // cells wide.
    out.push_str(&format!(
        " {} {}  {} {}  {} {}\n",
        status_glyph(Status::Working), working,
        status_glyph(Status::Waiting), waiting,
        status_glyph(Status::Idle),    idle,
    ));
    out.push_str(" ───────────────────────────────\n");
    if snap.entries.is_empty() {
        out.push_str(" no agents tracked\n");
    } else {
        let mut by_ws: std::collections::BTreeMap<String, Vec<&crate::dash::state::PaneState>> =
            std::collections::BTreeMap::new();
        for e in &snap.entries {
            by_ws.entry(e.workspace.clone()).or_default().push(e);
        }
        for (ws, panes) in by_ws {
            let label = if ws.is_empty() { "(no workspace)".into() } else { ws };
            out.push_str(&format!(" ▾ {}\n", label));
            for p in panes {
                let parked = if p.parked {
                    format!(" {}", parked_glyph())
                } else {
                    String::new()
                };
                out.push_str(&format!(
                    "    {} {:<7} {:<4}{}\n",
                    status_glyph(p.status),
                    truncate(&p.agent, 7),
                    humanize_age(p.last_activity),
                    parked,
                ));
            }
        }
    }
    // Hint footer: this pane is read-only by design — point users at the
    // popup for navigation and at next-ready for one-keystroke triage.
    // Always emitted so the affordance is discoverable even on first run
    // when no agents have fired hooks yet.
    out.push('\n');
    out.push_str(" ───────────────────────────────\n");
    out.push_str(" prefix+a → popup (j/k jump)\n");
    out.push_str(" prefix+N → next waiting agent\n");
    out
}
