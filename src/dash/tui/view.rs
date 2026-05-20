//! Render functions for both the popup and the sidebar.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap},
    Frame,
};

use crate::dash::render::{dormant_glyph, humanize_age, parked_glyph, status_glyph};
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
    // Create mode owns the entire body — replacing the agents/details
    // split with a focused form keeps the user's attention on the task
    // and avoids a layout that's stretched thin across three sections.
    if app.mode == Mode::Create {
        render_create(f, area, app);
        return;
    }

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

fn render_create(f: &mut Frame, area: Rect, app: &App) {
    use crate::dash::tui::app::CreateField;

    let form = match app.create.as_ref() {
        Some(f) => f,
        None => return,
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .padding(Padding::horizontal(2))
        .title(Span::styled(
            " New workspace ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Build the form content as a list of Lines. We lay out vertically:
    //   <blank>
    //   Name:  <input>
    //   <blank>
    //   Base:  ▾ <selected>
    //          • option
    //          • option
    //   <blank>
    //   <error if any>
    let label_style = Style::default().fg(Color::DarkGray);
    let active = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let inactive = Style::default().fg(Color::White);

    let name_style = if form.field == CreateField::Name { active } else { inactive };
    let base_style = if form.field == CreateField::Base { active } else { inactive };

    let cursor = if form.field == CreateField::Name { "│" } else { " " };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("Name:  ", label_style),
        Span::styled(form.name.clone(), name_style),
        Span::styled(cursor.to_string(), Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::raw(""));

    if form.bases.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Base:  ", label_style),
            Span::styled(
                "(no bases configured — run `aw init` first)",
                Style::default().fg(Color::Red),
            ),
        ]));
    } else {
        let chosen = form.bases.get(form.base_idx).map(String::as_str).unwrap_or("");
        lines.push(Line::from(vec![
            Span::styled("Base:  ", label_style),
            Span::styled(format!("▾ {}", chosen), base_style),
        ]));
        // Show all options when the Base field has focus — otherwise just
        // the chosen one. Keeps the form compact when focus is elsewhere.
        if form.field == CreateField::Base {
            for (i, b) in form.bases.iter().enumerate() {
                let marker = if i == form.base_idx { "▌ " } else { "  " };
                let marker_style = if i == form.base_idx {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                };
                let opt_style = if i == form.base_idx {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                lines.push(Line::from(vec![
                    Span::raw("       "),
                    Span::styled(marker, marker_style),
                    Span::styled(b.clone(), opt_style),
                ]));
            }
        }
    }

    if let Some(err) = form.error.as_deref() {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![Span::styled(
            format!("❌ {}", err),
            Style::default().fg(Color::Red),
        )]));
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
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
        // Differentiate "no panes, no workspaces on disk" (true empty)
        // from "agents not wired up but workspaces exist but H toggled
        // them off" (less likely; show generic). The dormant list is the
        // tell — when populated and hidden, we'd still have rows, so
        // app.rows being empty really does mean nothing to show.
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
    // so groups breathe a little. The same rule lives in
    // `app::injects_blank_before` so the scroll-line math stays in sync.
    let mut lines: Vec<Line> = Vec::with_capacity(app.rows.len() + 4);
    let mut prior_was_selectable = false;
    for (i, row) in app.rows.iter().enumerate() {
        if crate::dash::tui::app::injects_blank_before(prior_was_selectable, row) {
            lines.push(Line::raw(""));
        }
        lines.push(line_for_row(row, i == app.selected));
        prior_was_selectable = matches!(row, Row::Pane(_) | Row::Dormant(_));
    }

    // Scroll math: keep the selected row in view. The `Paragraph::scroll`
    // tuple is (y, x) — we only ever scroll vertically. We track the
    // offset on `app` (interior-mutable) so it persists across redraws,
    // which keeps the cursor "stable" in the middle of the viewport
    // instead of jumping to the edge on each navigation step.
    let viewport_h = inner.height;
    let total_lines = crate::dash::tui::app::total_displayed_lines(&app.rows);
    let selected_line =
        crate::dash::tui::app::displayed_line_index(&app.rows, app.selected);

    let mut offset = app.scroll_offset.get();
    // Defensive clamp — row list may have shrunk (H toggle, filter
    // pruned rows) since the last render.
    if total_lines > viewport_h {
        offset = offset.min(total_lines - viewport_h);
    } else {
        offset = 0;
    }
    // Bring the selected line into the viewport with the minimum scroll.
    if selected_line < offset {
        offset = selected_line;
    } else if viewport_h > 0 && selected_line >= offset + viewport_h {
        offset = selected_line + 1 - viewport_h;
    }
    app.scroll_offset.set(offset);

    f.render_widget(Paragraph::new(lines).scroll((offset, 0)), inner);
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
        Row::Header { workspace, session_hint, collapsed, pinned } => {
            let arrow = if *collapsed { "▸" } else { "▾" };
            let mut spans = vec![
                edge,
                Span::raw(" "),
                Span::styled(
                    format!("{} {}", arrow, workspace),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ];
            if *pinned {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    crate::dash::render::pinned_glyph().to_string(),
                    Style::default().fg(Color::Yellow),
                ));
            }
            spans.push(Span::raw("   "));
            spans.push(Span::styled(
                session_hint.clone(),
                Style::default().fg(Color::DarkGray),
            ));
            Line::from(spans)
        }
        Row::DormantDivider => Line::from(vec![
            edge,
            Span::raw(" "),
            Span::styled(
                "─ Dormant ───────────────────",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Row::Dormant(d) => {
            let mut spans = vec![
                edge,
                Span::raw("   "),
                Span::styled(
                    dormant_glyph().to_string(),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("{:<20}", truncate(&d.name, 20)),
                    Style::default().fg(Color::Gray),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("{:<10}", truncate(&d.base, 10)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw("  "),
                Span::styled(
                    humanize_created(&d.created),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            if d.pinned {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    crate::dash::render::pinned_glyph().to_string(),
                    Style::default().fg(Color::Yellow),
                ));
            }
            // Selected row highlight matches Pane styling so the cursor
            // reads consistently as you scroll between sections.
            let mut line = Line::from(std::mem::take(&mut spans));
            if selected {
                line = line.style(Style::default().bg(Color::DarkGray));
            }
            line
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
            // The "label" column shows whatever's most identifying for the
            // pane: a `/rename`'d Claude session name (which Claude writes
            // back to tmux's window_name + pane_title), the auto-renamed
            // command, etc. `label` is refreshed from tmux on every load;
            // fall back to `agent` when tmux is unreachable (file-only
            // fallback) so the column is never blank. 18 cols fits common
            // session names without spilling into the prompt column.
            let label_src = if p.label.is_empty() { p.agent.as_str() } else { p.label.as_str() };
            let label_col = format!("{:<18}", truncate(label_src, 18));
            let mut spans = vec![
                edge,
                Span::raw("   "),
                Span::styled(glyph.to_string(), glyph_style),
                Span::raw("  "),
                Span::styled(label_col, Style::default().fg(Color::White)),
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
    let dormant = app.selected_dormant();
    let title = if dormant.is_some() {
        " Workspace "
    } else if app.show_preview {
        " Preview "
    } else {
        " Details "
    };
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

    if let Some(d) = dormant {
        // Resolve the workspace dir for display; fall back gracefully if
        // env-resolution fails (shouldn't, since we only got here because
        // enumerate_workspaces succeeded earlier).
        let cwd_display = match crate::paths::Paths::from_env() {
            Ok(paths) => paths.workspace_dir(&d.name).display().to_string(),
            Err(_) => "—".into(),
        };
        let lines = vec![
            kv("name", &d.name),
            kv("base", &d.base),
            kv("created", if d.created.is_empty() { "—" } else { &d.created }),
            kv("cwd", &cwd_display),
            kv("session", &format!("aw-{} (will be created)", d.name)),
            Line::raw(""),
            Line::from(vec![
                Span::styled(
                    "↵ ",
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "open this workspace in a new tmux session",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
        ];
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
        return;
    }

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
        Mode::Create => {
            let key_style = Style::default().fg(Color::Cyan);
            let act_style = Style::default().fg(Color::DarkGray);
            let sep = Span::styled("  ·  ", act_style);
            let pairs: &[(&str, &str)] = &[
                ("↵", "create"),
                ("⇥", "switch field"),
                ("esc", "cancel"),
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
        Mode::Normal => {
            let key_style = Style::default().fg(Color::Cyan);
            let act_style = Style::default().fg(Color::DarkGray);
            let sep = Span::styled("  ·  ", act_style);
            // Adapt the Enter label based on whether the cursor is on a
            // dormant row — "open" reads better than "jump" for that
            // action (it spawns a new session rather than switching to
            // an existing pane).
            let enter_action = if app.selected_dormant().is_some() {
                "open"
            } else {
                "jump"
            };
            let pairs: &[(&str, &str)] = &[
                ("↵", enter_action),
                ("⇥", "preview"),
                ("/", "filter"),
                ("c", "new"),
                ("p", "park"),
                ("P", "pin"),
                ("n", "next-ready"),
                ("r", "refresh"),
                ("H", "dormant"),
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

/// Best-effort relative-age formatter for `WorkspaceMeta::created`, which is
/// free-form `date` output. We try a tiny set of common formats; on miss we
/// truncate the raw string. Never lies — falls back to the original.
fn humanize_created(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() || raw == "unknown" {
        return "—".into();
    }
    // Try ISO 8601 / RFC 3339-ish ("2024-03-05T14:23:01Z" or with offset).
    // No chrono in the deps, so do a hand-roll: pull the leading
    // YYYY-MM-DD HH:MM:SS chunk and treat as UTC if it looks structured.
    if let Some(epoch) = parse_iso_like(raw) {
        return crate::dash::render::humanize_age(epoch);
    }
    // Fallback: just show the raw string truncated. `date` output is too
    // locale-dependent to parse reliably without a real datetime library.
    truncate(raw, 16)
}

fn parse_iso_like(s: &str) -> Option<u64> {
    // Accept `YYYY-MM-DDTHH:MM:SS` or `YYYY-MM-DD HH:MM:SS` (UTC). Anything
    // after the seconds (timezone, fractional, etc.) is ignored — good
    // enough for "5d ago" precision.
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let year: i64 = std::str::from_utf8(&bytes[0..4]).ok()?.parse().ok()?;
    if bytes[4] != b'-' { return None; }
    let month: u32 = std::str::from_utf8(&bytes[5..7]).ok()?.parse().ok()?;
    if bytes[7] != b'-' { return None; }
    let day: u32 = std::str::from_utf8(&bytes[8..10]).ok()?.parse().ok()?;
    if !matches!(bytes[10], b'T' | b' ') { return None; }
    let hour: u32 = std::str::from_utf8(&bytes[11..13]).ok()?.parse().ok()?;
    if bytes[13] != b':' { return None; }
    let minute: u32 = std::str::from_utf8(&bytes[14..16]).ok()?.parse().ok()?;
    if bytes[16] != b':' { return None; }
    let second: u32 = std::str::from_utf8(&bytes[17..19]).ok()?.parse().ok()?;
    days_from_civil(year, month, day).and_then(|days| {
        let secs = days
            .checked_mul(86_400)?
            .checked_add(hour as i64 * 3600 + minute as i64 * 60 + second as i64)?;
        if secs < 0 { None } else { Some(secs as u64) }
    })
}

/// Howard Hinnant's days-from-civil. Returns days since the Unix epoch.
fn days_from_civil(y: i64, m: u32, d: u32) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe as i64 - 719_468)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dash::state::{DormantWorkspace, PaneState, Snapshot, Status};
    use crate::dash::tui::app::App;
    use ratatui::{backend::TestBackend, Terminal};

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

    /// Render `app` to an in-memory backend and return the visible text as
    /// a single string with newlines between rows (trailing whitespace per
    /// row stripped — content matters, padding doesn't).
    fn render_to_string(app: &App, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| render(f, app)).unwrap();
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

    #[test]
    fn popup_renders_dormant_section_after_active() {
        let app = App::new(Snapshot {
            entries: vec![pane("%1", "alpha", "claude")],
            dormant: vec![dormant("scratch", "default"), dormant("backlog", "python")],
        });
        // 130 cols so the full footer (which grew with `c new`) fits
        // without truncation; the assertions below match on key fragments.
        let out = render_to_string(&app, 130, 24);
        // Active workspace header + pane row
        assert!(out.contains("alpha"), "active workspace header missing:\n{}", out);
        // Dormant divider header
        assert!(out.contains("Dormant"), "dormant divider missing:\n{}", out);
        // Both dormant workspaces, both bases
        assert!(out.contains("backlog"), "backlog dormant row missing:\n{}", out);
        assert!(out.contains("scratch"), "scratch dormant row missing:\n{}", out);
        assert!(out.contains("python"), "base column for backlog missing:\n{}", out);
        // Footer key hint
        assert!(out.contains(" dormant"), "footer hint missing 'dormant':\n{}", out);
        // Default Enter label is "jump" (cursor is on the pane row by default).
        assert!(out.contains("jump"), "default footer should read 'jump':\n{}", out);
    }

    #[test]
    fn popup_footer_label_switches_to_open_when_dormant_selected() {
        let mut app = App::new(Snapshot {
            entries: vec![],
            dormant: vec![dormant("only", "default")],
        });
        // No panes, so the only selectable row is the dormant one.
        assert!(app.selected_dormant().is_some());
        let out = render_to_string(&app, 100, 24);
        assert!(out.contains("open"), "footer should read 'open' when dormant selected:\n{}", out);

        // Toggle dormant off — selection clears, footer reverts to 'jump'.
        app.toggle_dormant();
        let out2 = render_to_string(&app, 100, 24);
        assert!(out2.contains("jump"), "footer should revert to 'jump' when no dormant selected:\n{}", out2);
    }

    #[test]
    fn popup_hides_dormant_when_toggled_off() {
        let mut app = App::new(Snapshot {
            entries: vec![pane("%1", "alpha", "claude")],
            dormant: vec![dormant("hidden-ws", "default")],
        });
        let out_on = render_to_string(&app, 100, 24);
        assert!(out_on.contains("hidden-ws"), "dormant should be visible:\n{}", out_on);

        app.toggle_dormant();
        let out_off = render_to_string(&app, 100, 24);
        assert!(!out_off.contains("hidden-ws"),
            "dormant must be hidden after toggle:\n{}", out_off);
        assert!(!out_off.contains("Dormant"),
            "divider must be hidden after toggle:\n{}", out_off);
    }

    #[test]
    fn popup_detail_pane_shows_workspace_info_for_dormant() {
        let app = App::new(Snapshot {
            entries: vec![],
            dormant: vec![dormant("my-spike", "rust-base")],
        });
        let out = render_to_string(&app, 120, 24);
        // Detail pane title
        assert!(out.contains("Workspace"), "detail title 'Workspace' missing:\n{}", out);
        // Detail rows
        assert!(out.contains("my-spike"), "name field missing:\n{}", out);
        assert!(out.contains("rust-base"), "base field missing:\n{}", out);
        assert!(out.contains("aw-my-spike"), "session preview missing:\n{}", out);
        assert!(out.contains("will be created"), "session-status hint missing:\n{}", out);
    }

    #[test]
    fn list_scrolls_to_keep_selection_visible() {
        // 30 panes in a single workspace — far more than the viewport
        // can show. Move the selection to the last pane and assert that
        // its label is in the rendered output (i.e. the viewport
        // scrolled to follow it).
        let panes: Vec<_> = (0..30)
            .map(|i| pane(&format!("%{}", i), "alpha", &format!("agent-{:02}", i)))
            .collect();
        let mut app = App::new(Snapshot { entries: panes, dormant: vec![] });
        // Default selection is pane 0; agent-29 should NOT be visible at
        // a 24-row terminal (block borders + header eat several rows,
        // leaving ~20 list rows max).
        let initial = render_to_string(&app, 80, 24);
        assert!(initial.contains("agent-00"), "first pane visible initially:\n{}", initial);
        assert!(!initial.contains("agent-29"), "last pane should be off-screen initially:\n{}", initial);

        // Move down 29 times — selection lands on pane 29.
        for _ in 0..29 {
            app.move_down();
        }
        let scrolled = render_to_string(&app, 80, 24);
        assert!(scrolled.contains("agent-29"), "selected pane must be visible after scrolling:\n{}", scrolled);
        assert!(!scrolled.contains("agent-00"), "first pane should be scrolled out:\n{}", scrolled);

        // Move back to top — the viewport should follow up.
        for _ in 0..29 {
            app.move_up();
        }
        let back = render_to_string(&app, 80, 24);
        assert!(back.contains("agent-00"), "first pane visible again after scrolling up:\n{}", back);
    }

    #[test]
    fn create_form_renders_name_and_base_options() {
        use crate::dash::tui::app::{CreateField, CreateForm};
        let mut app = App::new(Snapshot { entries: vec![], dormant: vec![] });
        app.create = Some(CreateForm {
            name: "my-task".into(),
            bases: vec!["default".into(), "python".into(), "web".into()],
            base_idx: 1,
            existing: std::collections::HashSet::new(),
            field: CreateField::Base, // focus Base so all options render
            error: Some("name cannot contain '/'".into()),
        });
        app.mode = crate::dash::tui::app::Mode::Create;

        let out = render_to_string(&app, 100, 24);
        assert!(out.contains("New workspace"), "title missing:\n{}", out);
        assert!(out.contains("Name:"), "name label missing:\n{}", out);
        assert!(out.contains("my-task"), "name value missing:\n{}", out);
        assert!(out.contains("Base:"), "base label missing:\n{}", out);
        assert!(out.contains("▾ python"), "selected base missing:\n{}", out);
        assert!(out.contains("default"), "first option missing:\n{}", out);
        assert!(out.contains("web"), "third option missing:\n{}", out);
        assert!(out.contains("name cannot contain"), "error missing:\n{}", out);
        // Footer should be the Create-mode hint
        assert!(out.contains("↵ create"), "create-mode footer missing:\n{}", out);
        assert!(out.contains("⇥ switch field"), "switch-field hint missing:\n{}", out);
    }

    #[test]
    fn create_form_with_empty_bases_shows_init_hint() {
        use crate::dash::tui::app::{CreateField, CreateForm};
        let mut app = App::new(Snapshot { entries: vec![], dormant: vec![] });
        app.create = Some(CreateForm {
            name: String::new(),
            bases: vec![],
            base_idx: 0,
            existing: std::collections::HashSet::new(),
            field: CreateField::Name,
            error: None,
        });
        app.mode = crate::dash::tui::app::Mode::Create;

        let out = render_to_string(&app, 100, 24);
        assert!(out.contains("no bases configured"), "init hint missing:\n{}", out);
    }

    #[test]
    fn list_does_not_scroll_when_everything_fits() {
        let app = App::new(Snapshot {
            entries: vec![
                pane("%1", "alpha", "agent-a"),
                pane("%2", "alpha", "agent-b"),
            ],
            dormant: vec![],
        });
        // Render large enough that nothing gets clipped, then assert
        // scroll_offset stayed at 0.
        let _ = render_to_string(&app, 100, 40);
        assert_eq!(app.scroll_offset.get(), 0);
    }

    #[test]
    fn parse_iso_like_accepts_t_and_space_separators() {
        let with_t = parse_iso_like("2026-03-15T14:23:45Z").expect("T separator");
        let with_space = parse_iso_like("2026-03-15 14:23:45").expect("space separator");
        assert_eq!(with_t, with_space, "T and space variants should agree");

        // Cross-check against UNIX_EPOCH math: 2026-03-15T14:23:45 UTC vs
        // 2026-03-15T13:23:45 UTC should differ by exactly 3600 seconds.
        let later = parse_iso_like("2026-03-15T14:23:45Z").unwrap();
        let earlier = parse_iso_like("2026-03-15T13:23:45Z").unwrap();
        assert_eq!(later - earlier, 3600);

        // And 24h apart.
        let next_day = parse_iso_like("2026-03-16T14:23:45Z").unwrap();
        assert_eq!(next_day - later, 86_400);
    }

    #[test]
    fn parse_iso_like_rejects_garbage() {
        assert!(parse_iso_like("not a date").is_none());
        assert!(parse_iso_like("Tue Mar 15 14:23:45 PDT 2026").is_none());
        assert!(parse_iso_like("").is_none());
        assert!(parse_iso_like("2026-13-01T00:00:00Z").is_none(), "month=13 invalid");
    }

    #[test]
    fn humanize_created_falls_back_to_truncated_raw() {
        // Locale-dependent date format we can't parse — should be returned
        // truncated, never panic, never fabricate an age.
        let raw = "Tue Mar 15 14:23:45 PDT 2026";
        let out = humanize_created(raw);
        assert!(out.starts_with("Tue Mar"), "should preserve start of raw: got {:?}", out);
        assert!(out.chars().count() <= 16, "should truncate to <=16 chars: got {:?}", out);
    }

    #[test]
    fn humanize_created_parses_iso_to_age() {
        // 1970-01-01: very old, must produce "Nd" form.
        let out = humanize_created("1970-01-02T00:00:00Z");
        assert!(out.ends_with('d'), "ancient ISO date should produce day-form age: {:?}", out);
    }

    #[test]
    fn humanize_created_handles_empty_and_unknown() {
        assert_eq!(humanize_created(""), "—");
        assert_eq!(humanize_created("unknown"), "—");
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
