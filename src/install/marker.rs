//! Marker-block helpers for editing line-based config files (`~/.tmux.conf`,
//! shell rc files) idempotently.
//!
//! Block format (`prefix` is the host file's comment syntax — `#` for shell
//! and tmux configs, `--` for Lua):
//!
//!   # >>> aw <label> >>>
//!   <body lines>
//!   # <<< aw <label> <<<
//!
//! `apply` replaces an existing block (matched by the exact label) in place
//! and otherwise appends one at the end of the file.

use std::path::Path;

use anyhow::{Context, Result};

pub fn open_marker(prefix: &str, label: &str) -> String {
    format!("{} >>> aw {} >>>", prefix, label)
}

pub fn close_marker(prefix: &str, label: &str) -> String {
    format!("{} <<< aw {} <<<", prefix, label)
}

pub fn apply(path: &Path, prefix: &str, label: &str, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let new = render(&existing, prefix, label, body);
    std::fs::write(path, new).with_context(|| format!("write {}", path.display()))
}

/// Strip our marker block from `path` if present. Returns Ok(true) when a
/// block was found and removed, Ok(false) when no block was present.
pub fn remove(path: &Path, prefix: &str, label: &str) -> Result<bool> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };
    let (new, found) = strip(&existing, prefix, label);
    if !found {
        return Ok(false);
    }
    std::fs::write(path, new).with_context(|| format!("write {}", path.display()))?;
    Ok(true)
}

fn strip(existing: &str, prefix: &str, label: &str) -> (String, bool) {
    let open = open_marker(prefix, label);
    let close = close_marker(prefix, label);
    let open_idx = match existing.find(&open) {
        Some(i) => i,
        None => return (existing.to_string(), false),
    };
    let close_idx = match existing[open_idx..].find(&close) {
        Some(i) => open_idx + i,
        None => return (existing.to_string(), false),
    };
    // Expand the cut to whole lines: from the start of the line containing
    // `open` to one past the newline that ends the line containing `close`.
    // This way we never collapse blank lines that surround the block.
    let line_start = existing[..open_idx].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let close_end = close_idx + close.len();
    let line_end = existing[close_end..]
        .find('\n')
        .map(|i| close_end + i + 1)
        .unwrap_or(existing.len());
    let mut out = String::with_capacity(existing.len());
    out.push_str(&existing[..line_start]);
    out.push_str(&existing[line_end..]);
    (out, true)
}

fn render(existing: &str, prefix: &str, label: &str, body: &str) -> String {
    let open = open_marker(prefix, label);
    let close = close_marker(prefix, label);
    let block = format!("{}\n{}\n{}\n", open, body.trim_end_matches('\n'), close);

    // Try to replace an existing block in place.
    if let Some((before, rest)) = existing.split_once(&open) {
        if let Some((_old_body, after)) = rest.split_once(&close) {
            let after = after.strip_prefix('\n').unwrap_or(after);
            // Keep whatever preceded the block byte for byte. Normalising it
            // here would fight the append path below (which inserts a blank
            // separator), so a second `aw install` would rewrite the file
            // and show up as a spurious diff.
            let mut out = String::new();
            out.push_str(before);
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

    /// Applying twice must produce byte-identical output, or every re-install
    /// shows up as a diff in configs people keep in git.
    #[test]
    fn apply_is_byte_idempotent() {
        for existing in ["", "rc line\n", "rc line", "a\n\nb\n"] {
            let once = render(existing, "#", "lbl", "body");
            let twice = render(&once, "#", "lbl", "body");
            assert_eq!(once, twice, "not idempotent for {:?}", existing);
            // And a third pass, in case the second converged by accident.
            assert_eq!(twice, render(&twice, "#", "lbl", "body"));
        }
    }

    /// A changed body replaces the old one in place without moving the block.
    #[test]
    fn apply_replaces_body_without_shifting_the_file() {
        let first = render("rc line\n", "#", "lbl", "old body");
        let second = render(&first, "#", "lbl", "new body");
        assert!(second.contains("new body"));
        assert!(!second.contains("old body"));
        assert_eq!(
            first.replace("old body", "new body"),
            second,
            "only the body should differ"
        );
    }

    #[test]
    fn appends_when_absent() {
        let out = render("# rc start\nalias ll='ls -la'\n", "#", "shell", "eval foo");
        assert!(out.ends_with("# >>> aw shell >>>\neval foo\n# <<< aw shell <<<\n"));
        assert!(out.starts_with("# rc start\nalias ll"));
    }

    #[test]
    fn replaces_in_place() {
        let pre = "# top\n# >>> aw shell >>>\nold body\n# <<< aw shell <<<\n# tail\n";
        let out = render(pre, "#", "shell", "new body");
        assert!(out.contains("new body"));
        assert!(!out.contains("old body"));
        assert!(out.contains("# top"));
        assert!(out.contains("# tail"));
    }

    #[test]
    fn appends_to_empty_file() {
        let out = render("", "#", "shell", "x");
        assert_eq!(out, "# >>> aw shell >>>\nx\n# <<< aw shell <<<\n");
    }

    #[test]
    fn strip_removes_block_and_keeps_surrounding() {
        let pre = "# top\n\n# >>> aw foo >>>\nbody\n# <<< aw foo <<<\n# tail\n";
        let (out, found) = strip(pre, "#", "foo");
        assert!(found);
        assert_eq!(out, "# top\n\n# tail\n");
    }

    #[test]
    fn strip_returns_false_when_block_absent() {
        let pre = "# unrelated\n";
        let (out, found) = strip(pre, "#", "foo");
        assert!(!found);
        assert_eq!(out, pre);
    }

    #[test]
    fn strip_handles_block_at_end() {
        let pre = "# top\n# >>> aw foo >>>\nbody\n# <<< aw foo <<<\n";
        let (out, found) = strip(pre, "#", "foo");
        assert!(found);
        assert_eq!(out, "# top\n");
    }
}
