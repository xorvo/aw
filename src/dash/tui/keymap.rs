//! Key handling.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::dash::tui::app::{Action, App, CreateField, Mode, Row};

pub fn on_key(app: &mut App, key: KeyEvent) -> Action {
    match app.mode {
        Mode::Filter => filter_mode(app, key),
        Mode::Normal => normal_mode(app, key),
        Mode::Create => create_mode(app, key),
        Mode::Qr => qr_mode(app, key),
    }
}

/// The QR overlay is read-only — the usual dismiss keys close it and
/// everything else is swallowed so a stray keystroke can't jump panes
/// while the overlay hides the list.
fn qr_mode(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.exit_qr();
        }
        _ => {}
    }
    Action::Continue
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
        (KeyCode::Char('P'), _) => match workspace_under_cursor(app) {
            Some(ws) => Action::TogglePin(ws),
            None => Action::Continue,
        },
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
        (KeyCode::Char('Q'), _) => {
            app.enter_qr();
            Action::Continue
        }
        _ => Action::Continue,
    }
}

/// Resolve the workspace name addressed by the row under the cursor.
/// Headers and dormant rows are workspace-level; pane rows derive from
/// their `workspace` field. The dormant divider returns None.
fn workspace_under_cursor(app: &App) -> Option<String> {
    match app.rows.get(app.selected) {
        Some(Row::Pane(p)) => Some(p.workspace.clone()),
        Some(Row::Header { workspace, .. }) => Some(workspace.clone()),
        Some(Row::Dormant(d)) => Some(d.name.clone()),
        _ => None,
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
            pinned: false,
        }
    }

    fn dormant(name: &str) -> DormantWorkspace {
        DormantWorkspace {
            name: name.into(),
            base: "default".into(),
            created: "2026-03-01T10:00:00Z".into(),
            pinned: false,
            mtime: 0,
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

    // ---- Q (phone-pairing QR overlay) ----

    use crate::dash::tui::app::QrOverlay;

    fn shim_qr_overlay(app: &mut App) {
        // Bypass enter_qr's token/network resolution — construct the
        // overlay directly, mirroring shim_create_form.
        app.qr = Some(QrOverlay {
            url: Some("http://192.168.1.10:8787/?t=tok".into()),
            lines: vec!["██████".into()],
            error: None,
        });
        app.mode = Mode::Qr;
    }

    #[test]
    #[serial_test::serial]
    fn capital_q_opens_qr_overlay_with_pairing_url() {
        // Pin the token via env so the keypress never touches the real
        // token cache on the dev machine.
        std::env::set_var("AW_REMOTE_TOKEN", "test-token");
        let mut app = App::new(Snapshot {
            entries: vec![pane("%1", "alpha")],
            dormant: vec![],
        });
        let action = on_key(&mut app, key(KeyCode::Char('Q')));
        std::env::remove_var("AW_REMOTE_TOKEN");
        assert!(matches!(action, Action::Continue));
        assert_eq!(app.mode, Mode::Qr);
        let overlay = app.qr.as_ref().expect("overlay populated");
        let url = overlay.url.as_deref().expect("url resolved");
        assert!(url.ends_with("/?t=test-token"), "got {}", url);
        assert!(!overlay.lines.is_empty(), "QR lines rendered");
    }

    #[test]
    fn esc_closes_qr_overlay() {
        let mut app = App::new(Snapshot {
            entries: vec![pane("%1", "alpha")],
            dormant: vec![],
        });
        shim_qr_overlay(&mut app);
        let action = on_key(&mut app, key(KeyCode::Esc));
        assert!(matches!(action, Action::Continue));
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.qr.is_none());
    }

    #[test]
    fn other_keys_are_swallowed_while_qr_overlay_open() {
        let mut app = App::new(Snapshot {
            entries: vec![pane("%1", "alpha")],
            dormant: vec![],
        });
        shim_qr_overlay(&mut app);
        // 'p' would Park in Normal mode; the overlay must swallow it.
        let action = on_key(&mut app, key(KeyCode::Char('p')));
        assert!(matches!(action, Action::Continue));
        assert_eq!(app.mode, Mode::Qr, "overlay stays open on unrelated keys");
        // 'q' closes the overlay instead of quitting the whole popup.
        let action = on_key(&mut app, key(KeyCode::Char('q')));
        assert!(matches!(action, Action::Continue), "q must not Quit from the overlay");
        assert_eq!(app.mode, Mode::Normal);
    }

    // ---- P (pin) ----

    #[test]
    fn capital_p_on_pane_returns_toggle_pin_for_workspace() {
        let mut app = App::new(Snapshot {
            entries: vec![pane("%1", "alpha")],
            dormant: vec![],
        });
        let action = on_key(&mut app, key(KeyCode::Char('P')));
        match action {
            Action::TogglePin(ws) => assert_eq!(ws, "alpha"),
            other => panic!("expected TogglePin(alpha), got {:?}", other),
        }
    }

    #[test]
    fn capital_p_on_dormant_returns_toggle_pin_for_workspace() {
        let mut app = App::new(Snapshot {
            entries: vec![],
            dormant: vec![dormant("backlog")],
        });
        let action = on_key(&mut app, key(KeyCode::Char('P')));
        match action {
            Action::TogglePin(ws) => assert_eq!(ws, "backlog"),
            other => panic!("expected TogglePin(backlog), got {:?}", other),
        }
    }

    #[test]
    fn lowercase_p_still_parks_does_not_pin() {
        let mut app = App::new(Snapshot {
            entries: vec![pane("%5", "alpha")],
            dormant: vec![],
        });
        let action = on_key(&mut app, key(KeyCode::Char('p')));
        match action {
            Action::Park(p) => assert_eq!(p, "%5"),
            other => panic!("expected Park, got {:?}", other),
        }
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
