//! `aw install shell` — append `eval "$(aw shell-init <shell>)"` to the
//! user's rc file.

use std::path::PathBuf;

use anyhow::Result;

use crate::cli::ShellKind;
use crate::install::marker;

pub fn install(shell: ShellKind) -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    let (rc, name): (PathBuf, &str) = match shell {
        ShellKind::Zsh => (home.join(".zshrc"), "zsh"),
        ShellKind::Bash => (home.join(".bashrc"), "bash"),
        ShellKind::Fish => (home.join(".config/fish/config.fish"), "fish"),
    };
    let body = format!("eval \"$(aw shell-init {})\"", name);
    marker::apply(&rc, "shell-init", &body)?;
    println!("✅ Shell hook installed in {}", rc.display());
    println!("   Open a new shell or run: source {}", rc.display());
    Ok(())
}
