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
            // Dispatch on the selected row kind: pane → jump, dormant
            // workspace → open. Headers/dividers aren't selectable so this
            // can only be called on a Pane or Dormant row.
            if let Some(p) = app.selected_pane() {
                Action::Jump(p.pane_id.clone())
            } else if let Some(d) = app.selected_dormant() {
                Action::OpenWorkspace(d.name.clone())
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
        (KeyCode::Char('H'), _) => {
            app.toggle_dormant();
            Action::Continue
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dash::state::{DormantWorkspace, PaneState, Snapshot, Status};
    use crate::dash::tui::app::Row;
    use crossterm::event::KeyEventKind;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    fn pane(pane_id: &str, ws: &str) -> PaneState {
        PaneState {
            schema_version: 1,
            pane_id: pane_id.into(),
            session: format!("aw-{}", ws),
            workspace: ws.into(),
            cwd: format!("/tmp/{}", ws),
            agent: "claude".into(),
            status: Status::Idle,
            last_event: String::new(),
            last_activity: 0,
            last_prompt: String::new(),
            parked: false,
        }
    }

    fn dormant(name: &str) -> DormantWorkspace {
        DormantWorkspace {
            name: name.into(),
            base: "default".into(),
            created: "2026-03-01T10:00:00Z".into(),
        }
    }

    #[test]
    fn enter_on_pane_returns_jump_with_pane_id() {
        let mut app = App::new(Snapshot {
            entries: vec![pane("%42", "alpha")],
            dormant: vec![],
        });
        let action = on_key(&mut app, key(KeyCode::Enter));
        match action {
            Action::Jump(p) => assert_eq!(p, "%42"),
            other => panic!("expected Jump, got {:?}", other),
        }
    }

    #[test]
    fn enter_on_dormant_returns_open_workspace() {
        let mut app = App::new(Snapshot {
            entries: vec![],
            dormant: vec![dormant("backlog")],
        });
        // First selectable row is the dormant entry (no panes present).
        let action = on_key(&mut app, key(KeyCode::Enter));
        match action {
            Action::OpenWorkspace(name) => assert_eq!(name, "backlog"),
            other => panic!("expected OpenWorkspace, got {:?}", other),
        }
    }

    #[test]
    fn shift_h_toggles_dormant_section() {
        let mut app = App::new(Snapshot {
            entries: vec![pane("%1", "alpha")],
            dormant: vec![dormant("dorm")],
        });
        assert!(app.show_dormant);
        let _ = on_key(&mut app, key(KeyCode::Char('H')));
        assert!(!app.show_dormant);
        // Dormant rows are gone after toggle.
        assert!(!app.rows.iter().any(|r| matches!(r, Row::Dormant(_))));
        let _ = on_key(&mut app, key(KeyCode::Char('H')));
        assert!(app.show_dormant);
    }

    #[test]
    fn lowercase_h_does_not_toggle_dormant() {
        let mut app = App::new(Snapshot {
            entries: vec![],
            dormant: vec![dormant("dorm")],
        });
        let was = app.show_dormant;
        let _ = on_key(&mut app, key(KeyCode::Char('h')));
        assert_eq!(app.show_dormant, was, "lowercase h must not affect dormant");
    }

    #[test]
    fn park_on_dormant_row_is_a_noop_continue() {
        let mut app = App::new(Snapshot {
            entries: vec![],
            dormant: vec![dormant("dorm")],
        });
        let action = on_key(&mut app, key(KeyCode::Char('p')));
        // Park should produce Continue when no pane is selected — never
        // OpenWorkspace or anything destructive on a dormant row.
        assert!(matches!(action, Action::Continue));
    }
}
