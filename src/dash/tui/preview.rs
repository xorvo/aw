//! Pane preview via `tmux capture-pane -p -t <pane>`.

/// Last `lines` lines of the pane's visible buffer. Empty if tmux/pane is gone.
pub fn capture(pane_id: &str, lines: u16) -> String {
    let out = crate::dash::tmux::tmux_command()
        .args([
            "capture-pane",
            "-p",
            "-t",
            pane_id,
            "-S",
            &format!("-{}", lines),
        ])
        .stderr(std::process::Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => String::new(),
    }
}
