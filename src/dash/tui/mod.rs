//! ratatui-based dashboard TUI.
//!
//! Two display modes share the same renderer:
//!
//! - **Popup** (`aw dash`): full-screen interactive TUI. Two-pane layout
//!   with the agent list on the left and a detail/preview pane on the
//!   right. Keymap: j/k navigate, Enter jumps via `tmux switch-client`,
//!   `/` filters, `p` parks, `n` next-ready, `r` refresh, Tab toggles
//!   preview, `Q` shows the phone-pairing QR overlay, q quits.
//! - **Sidebar** (`aw dash sidebar` / `_sidebar-loop`): same renderer in a
//!   narrow single-column layout, redrawn on a 2 s timer. No interactive
//!   keys — kill the pane to dismiss.

pub mod app;
pub mod keymap;
pub mod preview;
pub mod view;

use std::io::{stdout, Write};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    cursor::{Hide, Show},
    event::{poll, read, Event},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::dash::state::Snapshot;
use crate::dash::tmux;
use crate::dash::tui::app::{Action, App};

/// `aw dash` — interactive popup. Returns once the user quits or jumps.
///
/// `start_in_filter` opens the popup with the cursor already in `/`-filter
/// mode — for tmux bindings that go straight to search.
pub fn run_popup(start_in_filter: bool) -> Result<()> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, Hide)?;

    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let result = popup_loop(&mut terminal, start_in_filter);

    // Always restore the terminal even if the inner loop returned an error.
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen, Show).ok();
    terminal.show_cursor().ok();

    let action = result?;
    handle_exit_action(action);
    Ok(())
}

fn popup_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    start_in_filter: bool,
) -> Result<Action> {
    let mut app = App::new(Snapshot::load()?);
    if start_in_filter {
        app.enter_filter();
    }
    let mut last_reload = Instant::now();

    loop {
        terminal.draw(|f| view::render(f, &app))?;

        // Poll up to 250 ms so we redraw periodically (humanized "5s ago"
        // labels stay current, and a fresh state file shows up promptly).
        if poll(Duration::from_millis(250))? {
            if let Event::Key(key) = read()? {
                let action = keymap::on_key(&mut app, key);
                match action {
                    Action::Continue => {}
                    Action::Quit => return Ok(Action::Quit),
                    Action::Jump(_)
                    | Action::Park(_)
                    | Action::Refresh
                    | Action::NextReady
                    | Action::OpenWorkspace(_)
                    | Action::CreateWorkspace { .. }
                    | Action::TogglePin(_) => {
                        if let Some(after) = app.apply(action) {
                            return Ok(after);
                        }
                    }
                }
            }
        }

        if last_reload.elapsed() >= Duration::from_millis(500) {
            app.reload(Snapshot::load()?);
            last_reload = Instant::now();
        }
    }
}

fn handle_exit_action(a: Action) {
    match a {
        Action::Jump(pane) => tmux::switch_to_pane(&pane),
        Action::NextReady => {
            // Resolve next-ready and jump.
            if let Ok(snap) = Snapshot::load() {
                if let Some(pane) = crate::dash::pick_next_ready_for(&snap) {
                    tmux::switch_to_pane(&pane);
                }
            }
        }
        Action::OpenWorkspace(name) => {
            // Print to stderr so a failure surfaces after the alt-screen
            // is restored. Exec-replaces our process when outside tmux.
            if let Err(e) = crate::workspace::start::open_or_attach_session(&name) {
                eprintln!("aw: could not open workspace '{}': {}", name, e);
            }
        }
        Action::CreateWorkspace { name, base } => {
            // The alt-screen has been torn down by run_popup, so emoji
            // progress from create::run prints to the popup's tty
            // normally. Once create succeeds we transition into the new
            // session.
            if let Err(e) = crate::workspace::create::run(&name, &base) {
                eprintln!("aw: could not create workspace '{}': {}", name, e);
                return;
            }
            if let Err(e) = crate::workspace::start::open_or_attach_session(&name) {
                eprintln!("aw: created '{}' but could not open: {}", name, e);
            }
        }
        _ => {}
    }
}

/// `aw dash sidebar` — open the agent sidebar.
///
/// Idempotent within a tmux session: if a sidebar pane (tagged
/// `@aw-sidebar = 1`) already exists, we just focus it. Otherwise we split
/// a new 42-column pane to the right and tag it.
pub fn run_sidebar() -> Result<()> {
    if std::env::var_os("TMUX").is_none() {
        anyhow::bail!("not inside a tmux session");
    }
    let session = tmux_capture(&["display-message", "-p", "#{session_name}"])
        .ok_or_else(|| anyhow::anyhow!("could not resolve current tmux session"))?;

    if let Some(existing) = find_existing_sidebar(&session) {
        let _ = std::process::Command::new("tmux")
            .args(["select-pane", "-t", &existing])
            .status();
        return Ok(());
    }

    let aw_self = std::env::current_exe()?;
    let cmd = format!("{} _sidebar-loop", aw_self.display());
    let out = std::process::Command::new("tmux")
        .args([
            "split-window",
            "-h",
            "-l", "42",
            "-t", &session,
            "-P",                       // print the new pane id...
            "-F", "#{pane_id}",         // ...in this format
            &cmd,
        ])
        .output()?;
    if !out.status.success() {
        anyhow::bail!(
            "tmux split-window failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let new_pane = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !new_pane.is_empty() {
        let _ = std::process::Command::new("tmux")
            .args(["set-option", "-p", "-t", &new_pane, "@aw-sidebar", "1"])
            .status();
    }
    Ok(())
}

/// Returns the pane id of an existing sidebar in `session`, or None.
fn find_existing_sidebar(session: &str) -> Option<String> {
    let out = std::process::Command::new("tmux")
        .args([
            "list-panes",
            "-s",                        // all windows in the session
            "-t", session,
            "-F", "#{pane_id}\t#{@aw-sidebar}",
        ])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).lines().find_map(|line| {
        let mut it = line.splitn(2, '\t');
        let pane = it.next()?;
        let mark = it.next().unwrap_or("");
        if mark == "1" { Some(pane.to_string()) } else { None }
    })
}

fn tmux_capture(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("tmux")
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `aw _sidebar-loop` — long-running redraw loop. Hand-renders to stdout
/// (no alt-screen), so it lives nicely inside a regular tmux pane.
pub fn run_sidebar_loop() -> Result<()> {
    let mut last_state = None::<String>;
    loop {
        let snap = Snapshot::load().unwrap_or(Snapshot { entries: vec![], dormant: vec![] });
        let rendered = view::render_sidebar_text(&snap);
        // Only repaint when content changed — avoids flicker.
        if last_state.as_deref() != Some(rendered.as_str()) {
            // Clear screen + move home.
            print!("\x1b[2J\x1b[H{}", rendered);
            stdout().flush().ok();
            last_state = Some(rendered);
        }
        std::thread::sleep(Duration::from_millis(2000));
    }
}
