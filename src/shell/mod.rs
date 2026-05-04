//! Shell integration: `aw shell-init <shell>` (the eval'able hook),
//! `aw completions <shell>` (clap-generated completions), and
//! `aw _detect-workspace <cwd>` (used by the auto-cd hook).

pub mod completions;
pub mod detect;
pub mod init;
