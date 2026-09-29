//! ratatui-based dashboard TUI.
//!
//! Two display modes share the same renderer:
//!
//! - **Popup** (`aw dash`): full-screen interactive TUI. Two-pane layout
//!   with the agent list on the left and a detail/preview pane on the
//!   right. Keymap: j/k navigate, Enter jumps via `tmux switch-client`,
//!   `/` filters, `p` parks, `n` next-ready, `r` refresh, Tab toggles
//!   preview, `Q` shows the phone-pairing QR overlay, q quits.
//! - **Sidebar** (`aw dash sidebar` / `_sidebar-loop`): the same `App` and
//!   keymap in a narrow single-column layout, pinned in a tmux pane. Rows
//!   are grouped by status rather than workspace (waiting first) because
//!   it answers "who needs me" rather than "what is in this workspace".
//!   Fully interactive, with one difference from the popup: jumping does
//!   not close it.

pub mod app;
pub mod keymap;
pub mod preview;
pub mod switch;
pub mod view;

use std::io::stdout;
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
use crate::cli::SidebarSide;
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
/// `@aw-sidebar = 1`) already exists, we just focus it — `side` only
/// applies when one is actually created. Otherwise we split a new
/// 42-column pane on `side` and tag it.
pub fn run_sidebar(side: SidebarSide) -> Result<()> {
    if std::env::var_os("TMUX").is_none() {
        anyhow::bail!("not inside a tmux session");
    }
    let session = tmux_capture(&["display-message", "-p", "#{session_name}"])
        .ok_or_else(|| anyhow::anyhow!("could not resolve current tmux session"))?;

    if let Some(existing) = find_existing_sidebar(&session) {
        let _ = crate::dash::tmux::tmux_command()
            .args(["select-pane", "-t", &existing])
            .status();
        return Ok(());
    }

    let aw_self = std::env::current_exe()?;
    let cmd = format!("{} _sidebar-loop", aw_self.display());
    // `-b` puts the new pane *before* the target — left, for a horizontal
    // split. Without it tmux splits to the right.
    let split = match side {
        SidebarSide::Left => "-hb",
        SidebarSide::Right => "-h",
    };
    let out = crate::dash::tmux::tmux_command()
        .args([
            "split-window",
            split,
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
        let _ = crate::dash::tmux::tmux_command()
            .args(["set-option", "-p", "-t", &new_pane, "@aw-sidebar", "1"])
            .status();
    }
    Ok(())
}

/// Returns the pane id of an existing sidebar in `session`, or None.
fn find_existing_sidebar(session: &str) -> Option<String> {
    let out = crate::dash::tmux::tmux_command()
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
    let out = crate::dash::tmux::tmux_command()
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
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, Hide)?;

    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let result = sidebar_loop(&mut terminal);

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen, Show).ok();
    terminal.show_cursor().ok();
    result
}

/// The sidebar's event loop.
///
/// Shares `App` and `keymap` with the popup, with one deliberate
/// difference: **the popup exits to act, the sidebar acts and stays**.
/// `aw dash` tears down its alternate screen and then jumps, because it
/// is a transient overlay. A pinned sidebar that closed itself every time
/// you jumped would be useless, so Jump/NextReady switch the tmux client
/// from inside the loop and keep running. Only `q` ends it.
fn sidebar_loop<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    let mut app = App::new_sidebar(Snapshot::load()?);
    let mut last_reload = Instant::now();

    loop {
        terminal.draw(|f| view::render_sidebar(f, &app))?;

        if poll(Duration::from_millis(250))? {
            if let Event::Key(key) = read()? {
                match keymap::on_key(&mut app, key) {
                    Action::Quit => return Ok(()),
                    Action::Jump(pane) => tmux::switch_to_pane(&pane),
                    Action::NextReady => {
                        if let Some(pane) = crate::dash::pick_next_ready_for(&app.snapshot) {
                            tmux::switch_to_pane(&pane);
                        }
                    }
                    // Park/Pin/Refresh mutate sentinels and reload in
                    // place; they never ask the caller to exit, so the
                    // returned action is discarded.
                    other @ (Action::Park(_)
                    | Action::Refresh
                    | Action::TogglePin(_)
                    | Action::OpenWorkspace(_)
                    | Action::CreateWorkspace { .. }) => {
                        if let Some(after) = app.apply(other) {
                            // `apply` only hands back Jump/OpenWorkspace
                            // follow-ups; honour them without exiting.
                            match after {
                                Action::Jump(pane) => tmux::switch_to_pane(&pane),
                                Action::OpenWorkspace(name) => {
                                    let _ = crate::workspace::start::open_or_attach_session(&name);
                                }
                                _ => {}
                            }
                        }
                    }
                    Action::Continue => {}
                }
            }
        }

        if last_reload.elapsed() >= Duration::from_millis(1000) {
            app.reload(Snapshot::load()?);
            last_reload = Instant::now();
        }
    }
}
