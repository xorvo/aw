//! `aw install ...` — interactive setup helpers.
//!
//! Every step is idempotent:
//!  - JSON/TOML edits use serde to round-trip, only adding entries that
//!    aren't already present (matched by exact command string).
//!  - Text-file edits (~/.tmux.conf, shell rc files) bracket their content
//!    with `# >>> aw <name> >>>` / `# <<< aw <name> <<<` markers so a
//!    re-run replaces the block in place.

use anyhow::Result;

use crate::cli::{AgentKind, InstallCmd, ShellKind};

pub mod claude;
pub mod codex;
pub mod marker;
pub mod pi;
pub mod service;
pub mod shell_rc;
pub mod tmux;

pub fn run(cmd: InstallCmd) -> Result<()> {
    match cmd {
        InstallCmd::Shell { shell } => run_shell(shell),
        InstallCmd::Hooks { agent } => run_hooks(agent.unwrap_or(AgentKind::All)),
        InstallCmd::TmuxBindings { config } => tmux::install(config.as_deref()),
        InstallCmd::Service { uninstall, host, port } => {
            if uninstall {
                service::uninstall()
            } else {
                service::install(host.as_deref(), port)
            }
        }
        InstallCmd::All => run_all(),
    }
}

fn run_shell(shell: Option<ShellKind>) -> Result<()> {
    let shell = shell.or_else(detect_shell).unwrap_or(ShellKind::Zsh);
    shell_rc::install(shell)
}

fn run_hooks(agent: AgentKind) -> Result<()> {
    match agent {
        AgentKind::Claude => claude::install(),
        AgentKind::Codex => codex::install(),
        AgentKind::Pi => pi::install(),
        AgentKind::All => {
            // Best-effort: each step prints its own status; don't bail early.
            let _ = claude::install();
            let _ = codex::install();
            let _ = pi::install();
            Ok(())
        }
    }
}

fn run_all() -> Result<()> {
    println!("🛠️  aw install all");
    println!();
    println!("→ Shell integration");
    let _ = run_shell(None);
    println!();
    println!("→ Agent hooks");
    let _ = run_hooks(AgentKind::All);
    println!();
    println!("→ Tmux key bindings");
    let _ = tmux::install(None);
    println!();
    println!("→ Phone remote (aw serve at login)");
    let _ = service::install(None, None);
    println!();
    println!("✅ Done. You may need to restart your shell.");
    Ok(())
}

/// Best-effort shell detection from $SHELL.
fn detect_shell() -> Option<ShellKind> {
    let s = std::env::var("SHELL").ok()?;
    if s.contains("zsh") {
        Some(ShellKind::Zsh)
    } else if s.contains("fish") {
        Some(ShellKind::Fish)
    } else if s.contains("bash") {
        Some(ShellKind::Bash)
    } else {
        None
    }
}
