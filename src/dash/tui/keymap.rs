//! Key handling.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::dash::tui::app::{Action, App, CreateField, Mode};

pub fn on_key(app: &mut App, key: KeyEvent) -> Action {
    match app.mode {
        Mode::Filter => filter_mode(app, key),
        Mode::Normal => normal_mode(app, key),
        Mode::Create => create_mode(app, key),
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
        (KeyCode::Char('c'), _) => {
            app.enter_create();
            Action::Continue
        }
        _ => Action::Continue,
    }
}

fn create_mode(app: &mut App, key: KeyEvent) -> Action {
    let field = match app.create.as_ref().map(|f| f.field) {
        Some(f) => f,
        None => return Action::Continue,
    };
    match key.code {
        KeyCode::Esc => {
            app.exit_create();
            Action::Continue
        }
        KeyCode::Enter => app.submit_create(),
        KeyCode::Tab | KeyCode::BackTab => {
            app.with_create(|f| {
                f.field = match f.field {
                    CreateField::Name => CreateField::Base,
                    CreateField::Base => CreateField::Name,
                };
            });
            Action::Continue
        }
        KeyCode::Backspace if field == CreateField::Name => {
            app.with_create(|f| {
                f.name.pop();
            });
            Action::Continue
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.with_create(|f| {
                if f.field == CreateField::Name {
                    f.name.clear();
                }
            });
            Action::Continue
        }
        KeyCode::Up | KeyCode::Char('k') if field == CreateField::Base => {
            app.with_create(|f| {
                if !f.bases.is_empty() && f.base_idx > 0 {
                    f.base_idx -= 1;
                }
            });
            Action::Continue
        }
        KeyCode::Down | KeyCode::Char('j') if field == CreateField::Base => {
            app.with_create(|f| {
                if !f.bases.is_empty() && f.base_idx + 1 < f.bases.len() {
                    f.base_idx += 1;
                }
            });
            Action::Continue
        }
        KeyCode::Char(c)
            if field == CreateField::Name
                && !key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.with_create(|f| {
                f.name.push(c);
            });
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
            label: String::new(),
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

    // ---- Create-mode keymap ----

    use crate::dash::tui::app::{CreateField, CreateForm};

    fn shim_create_form(app: &mut App, bases: Vec<&str>) {
        // Bypass enter_create's disk reads — tests don't have a real
        // config or workspaces dir. Construct the form directly.
        app.create = Some(CreateForm {
            name: String::new(),
            bases: bases.into_iter().map(String::from).collect(),
            base_idx: 0,
            existing: std::collections::HashSet::new(),
            field: CreateField::Name,
            error: None,
        });
        app.mode = Mode::Create;
    }

    #[test]
    fn typing_in_name_field_appends_chars() {
        let mut app = App::new(Snapshot { entries: vec![], dormant: vec![] });
        shim_create_form(&mut app, vec!["default"]);
        for ch in "my-feature".chars() {
            let _ = on_key(&mut app, key(KeyCode::Char(ch)));
        }
        assert_eq!(app.create.as_ref().unwrap().name, "my-feature");
    }

    #[test]
    fn backspace_in_name_field_pops_last_char() {
        let mut app = App::new(Snapshot { entries: vec![], dormant: vec![] });
        shim_create_form(&mut app, vec!["default"]);
        for ch in "abc".chars() {
            let _ = on_key(&mut app, key(KeyCode::Char(ch)));
        }
        let _ = on_key(&mut app, key(KeyCode::Backspace));
        assert_eq!(app.create.as_ref().unwrap().name, "ab");
    }

    #[test]
    fn tab_switches_field_focus() {
        let mut app = App::new(Snapshot { entries: vec![], dormant: vec![] });
        shim_create_form(&mut app, vec!["default", "python"]);
        assert_eq!(app.create.as_ref().unwrap().field, CreateField::Name);
        let _ = on_key(&mut app, key(KeyCode::Tab));
        assert_eq!(app.create.as_ref().unwrap().field, CreateField::Base);
        let _ = on_key(&mut app, key(KeyCode::Tab));
        assert_eq!(app.create.as_ref().unwrap().field, CreateField::Name);
    }

    #[test]
    fn arrows_move_base_selection_when_focused() {
        let mut app = App::new(Snapshot { entries: vec![], dormant: vec![] });
        shim_create_form(&mut app, vec!["a", "b", "c"]);
        let _ = on_key(&mut app, key(KeyCode::Tab)); // focus Base
        let _ = on_key(&mut app, key(KeyCode::Down));
        assert_eq!(app.create.as_ref().unwrap().base_idx, 1);
        let _ = on_key(&mut app, key(KeyCode::Down));
        assert_eq!(app.create.as_ref().unwrap().base_idx, 2);
        // Clamped at the bottom.
        let _ = on_key(&mut app, key(KeyCode::Down));
        assert_eq!(app.create.as_ref().unwrap().base_idx, 2);
        // Up works too.
        let _ = on_key(&mut app, key(KeyCode::Up));
        assert_eq!(app.create.as_ref().unwrap().base_idx, 1);
    }

    #[test]
    fn esc_cancels_create_form() {
        let mut app = App::new(Snapshot { entries: vec![], dormant: vec![] });
        shim_create_form(&mut app, vec!["default"]);
        let _ = on_key(&mut app, key(KeyCode::Esc));
        assert!(app.create.is_none());
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn enter_submits_valid_form_to_create_action() {
        let mut app = App::new(Snapshot { entries: vec![], dormant: vec![] });
        shim_create_form(&mut app, vec!["default", "python"]);
        for ch in "my-task".chars() {
            let _ = on_key(&mut app, key(KeyCode::Char(ch)));
        }
        let _ = on_key(&mut app, key(KeyCode::Tab));
        let _ = on_key(&mut app, key(KeyCode::Down)); // select python
        let action = on_key(&mut app, key(KeyCode::Enter));
        match action {
            Action::CreateWorkspace { name, base } => {
                assert_eq!(name, "my-task");
                assert_eq!(base, "python");
            }
            other => panic!("expected CreateWorkspace, got {:?}", other),
        }
        // Form cleared on successful submit.
        assert!(app.create.is_none());
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn enter_on_empty_name_keeps_form_with_error() {
        let mut app = App::new(Snapshot { entries: vec![], dormant: vec![] });
        shim_create_form(&mut app, vec!["default"]);
        let action = on_key(&mut app, key(KeyCode::Enter));
        assert!(matches!(action, Action::Continue));
        let f = app.create.as_ref().expect("form retained on invalid submit");
        assert!(f.error.is_some());
        assert!(f.error.as_deref().unwrap().contains("name"));
    }

    #[test]
    fn enter_on_conflicting_name_shows_error() {
        let mut app = App::new(Snapshot { entries: vec![], dormant: vec![] });
        shim_create_form(&mut app, vec!["default"]);
        // Inject a conflict directly.
        app.create.as_mut().unwrap().existing.insert("taken".into());
        for ch in "taken".chars() {
            let _ = on_key(&mut app, key(KeyCode::Char(ch)));
        }
        let action = on_key(&mut app, key(KeyCode::Enter));
        assert!(matches!(action, Action::Continue));
        let err = app.create.as_ref().and_then(|f| f.error.clone()).unwrap_or_default();
        assert!(err.contains("already exists"), "got: {}", err);
    }

    #[test]
    fn enter_with_slash_in_name_shows_error() {
        let mut app = App::new(Snapshot { entries: vec![], dormant: vec![] });
        shim_create_form(&mut app, vec!["default"]);
        for ch in "bad/name".chars() {
            let _ = on_key(&mut app, key(KeyCode::Char(ch)));
        }
        let action = on_key(&mut app, key(KeyCode::Enter));
        assert!(matches!(action, Action::Continue));
        let err = app.create.as_ref().and_then(|f| f.error.clone()).unwrap_or_default();
        assert!(err.contains("'/'"), "got: {}", err);
    }

    #[test]
    fn enter_with_no_bases_configured_shows_error() {
        let mut app = App::new(Snapshot { entries: vec![], dormant: vec![] });
        shim_create_form(&mut app, vec![]); // empty bases
        for ch in "fine-name".chars() {
            let _ = on_key(&mut app, key(KeyCode::Char(ch)));
        }
        let action = on_key(&mut app, key(KeyCode::Enter));
        assert!(matches!(action, Action::Continue));
        let err = app.create.as_ref().and_then(|f| f.error.clone()).unwrap_or_default();
        assert!(err.contains("no bases"), "got: {}", err);
    }

    #[test]
    fn lowercase_c_in_normal_enters_create_mode() {
        let mut app = App::new(Snapshot { entries: vec![], dormant: vec![] });
        assert_eq!(app.mode, Mode::Normal);
        let _ = on_key(&mut app, key(KeyCode::Char('c')));
        assert_eq!(app.mode, Mode::Create);
        // The form is populated (bases may be empty in a test sandbox,
        // but the form itself must exist for the renderer to see it).
        assert!(app.create.is_some());
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
