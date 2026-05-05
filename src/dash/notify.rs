//! System notifications for state transitions. Uses `notify-rust` which
//! handles macOS, Linux (D-Bus), and Windows. Failures are silent — a missed
//! notification is far better than a broken hook.

use crate::dash::state::PaneState;

pub fn on_waiting(state: &PaneState) {
    if std::env::var("AW_DASH_NOTIFY").as_deref() == Ok("0") {
        return;
    }
    let title = "Agent waiting";
    let body = if state.workspace.is_empty() {
        format!("{} ({})", state.agent, state.pane_id)
    } else {
        format!("{} ({})", state.workspace, state.agent)
    };
    let _ = notify_rust::Notification::new()
        .summary(title)
        .body(&body)
        .show();
}
