//! Marker-block helpers for editing line-based config files (`~/.tmux.conf`,
//! shell rc files) idempotently.
//!
//! Block format:
//!
//!   # >>> aw <label> >>>
//!   <body lines>
//!   # <<< aw <label> <<<
//!
//! `apply` replaces an existing block (matched by the exact label) in place
//! and otherwise appends one at the end of the file.

use std::path::Path;

use anyhow::{Context, Result};

pub fn open_marker(label: &str) -> String {
    format!("# >>> aw {} >>>", label)
}

pub fn close_marker(label: &str) -> String {
    format!("# <<< aw {} <<<", label)
}

pub fn apply(path: &Path, label: &str, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let new = render(&existing, label, body);
    std::fs::write(path, new).with_context(|| format!("write {}", path.display()))
}

fn render(existing: &str, label: &str, body: &str) -> String {
    let open = open_marker(label);
    let close = close_marker(label);
    let block = format!("{}\n{}\n{}\n", open, body.trim_end_matches('\n'), close);

    // Try to replace an existing block in place.
    if let Some((before, rest)) = existing.split_once(&open) {
        if let Some((_old_body, after)) = rest.split_once(&close) {
            let after = after.strip_prefix('\n').unwrap_or(after);
            let before_clean = before.trim_end_matches('\n');
            let mut out = String::new();
            out.push_str(before_clean);
            if !before_clean.is_empty() {
                out.push('\n');
            }
            out.push_str(&block);
            out.push_str(after);
            return out;
        }
    }
    // Append.
    let mut out = String::with_capacity(existing.len() + block.len() + 2);
    out.push_str(existing);
    if !existing.is_empty() && !existing.ends_with('\n') {
        out.push('\n');
    }
    if !existing.is_empty() {
        out.push('\n');
    }
    out.push_str(&block);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_when_absent() {
        let out = render("# rc start\nalias ll='ls -la'\n", "shell", "eval foo");
        assert!(out.ends_with("# >>> aw shell >>>\neval foo\n# <<< aw shell <<<\n"));
        assert!(out.starts_with("# rc start\nalias ll"));
    }

    #[test]
    fn replaces_in_place() {
        let pre = "# top\n# >>> aw shell >>>\nold body\n# <<< aw shell <<<\n# tail\n";
        let out = render(pre, "shell", "new body");
        assert!(out.contains("new body"));
        assert!(!out.contains("old body"));
        assert!(out.contains("# top"));
        assert!(out.contains("# tail"));
    }

    #[test]
    fn appends_to_empty_file() {
        let out = render("", "shell", "x");
        assert_eq!(out, "# >>> aw shell >>>\nx\n# <<< aw shell <<<\n");
    }
}
