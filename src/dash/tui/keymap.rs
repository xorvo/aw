//! Key handling.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::dash::tui::app::{Action, App, Mode};

pub fn on_key(app: &mut App, key: KeyEvent) -> Action {
    match app.mode {
        Mode::Filter => filter_mode(app, key),
        Mode::Normal => normal_mode(app, key),
    }
}

fn filter_mode(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => {
            app.exit_filter();
            Action::Continue
        }
        KeyCode::Enter => {
            app.exit_filter();
            Action::Continue
        }
        KeyCode::Backspace => {
            app.filter_pop();
            Action::Continue
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.filter_clear();
            Action::Continue
        }
        KeyCode::Char(c) => {
            app.filter_push(c);
            Action::Continue
        }
        _ => Action::Continue,
    }
}

fn normal_mode(app: &mut App, key: KeyEvent) -> Action {
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => Action::Quit,
        (KeyCode::Char('j'), _) | (KeyCode::Down, _) => {
            app.move_down();
            Action::Continue
        }
        (KeyCode::Char('k'), _) | (KeyCode::Up, _) => {
            app.move_up();
            Action::Continue
        }
        (KeyCode::Enter, _) => {
            if let Some(p) = app.selected_pane() {
                Action::Jump(p.pane_id.clone())
            } else {
                Action::Continue
            }
        }
        (KeyCode::Char('p'), _) => {
            if let Some(p) = app.selected_pane() {
                Action::Park(p.pane_id.clone())
            } else {
                Action::Continue
            }
        }
        (KeyCode::Char('n'), _) => Action::NextReady,
        (KeyCode::Char('r'), _) => Action::Refresh,
        (KeyCode::Tab, _) => {
            app.toggle_preview();
            Action::Continue
        }
        (KeyCode::Char(' '), _) => {
            app.toggle_collapse();
            Action::Continue
        }
        (KeyCode::Char('/'), _) => {
            app.enter_filter();
            Action::Continue
        }
        _ => Action::Continue,
    }
}
