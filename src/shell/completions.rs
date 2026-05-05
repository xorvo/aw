//! `aw completions <shell>` — emit a clap-generated completion script.

use anyhow::Result;
use clap::CommandFactory;

use crate::cli::{Cli, ShellKind};

pub fn run(shell: ShellKind) -> Result<()> {
    let mut cmd = Cli::command();
    let bin_name = "aw";
    let target = match shell {
        ShellKind::Zsh => clap_complete::Shell::Zsh,
        ShellKind::Bash => clap_complete::Shell::Bash,
        ShellKind::Fish => clap_complete::Shell::Fish,
    };
    clap_complete::generate(target, &mut cmd, bin_name, &mut std::io::stdout());
    Ok(())
}
