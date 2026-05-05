mod cli;
mod config;
mod dash;
mod git;
mod hook;
mod install;
mod paths;
mod self_update;
mod shell;
mod workspace;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Cmd, DashCmd, SelfCmd};

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Init { base } => workspace::init::run(&base),
        Cmd::Create { name, base } => workspace::create::run(&name, &base),
        Cmd::List => workspace::list::run(),
        Cmd::Start { name, no_tmux } => workspace::start::run(&name, no_tmux),
        Cmd::Delete { name } => workspace::delete::run(&name),
        Cmd::Config => workspace::config_show::run(),
        Cmd::EditConfig => workspace::edit::edit_config(),
        Cmd::EditBase { base } => workspace::edit::edit_base(&base),
        Cmd::Sync => workspace::sync::run(),
        Cmd::OpenHome => workspace::edit::open_home(),

        Cmd::Dash { command } => match command {
            None => dash::tui::run_popup(),
            Some(DashCmd::Sidebar) => dash::tui::run_sidebar(),
            Some(DashCmd::StatusLine) => dash::cmd_status_line(),
            Some(DashCmd::NextReady) => dash::cmd_next_ready(),
            Some(DashCmd::Park { pane }) => dash::cmd_park(pane.as_deref()),
            Some(DashCmd::Json) => dash::cmd_json(),
            Some(DashCmd::Gc) => dash::cmd_gc(),
        },

        Cmd::Hook { agent, event, prompt } => hook::run(agent, &event, prompt),

        Cmd::ShellInit { shell } => shell::init::run(shell),
        Cmd::Completions { shell } => shell::completions::run(shell),

        Cmd::Install { command } => install::run(command),

        Cmd::SelfMgmt { command } => match command {
            SelfCmd::Check => self_update::check(),
            SelfCmd::Update { yes } => self_update::update(yes),
        },

        Cmd::ShellStart { name, no_tmux } => workspace::start::shell_start(&name, no_tmux),
        Cmd::DetectWorkspace { cwd } => shell::detect::run(&cwd),
        Cmd::SidebarLoop => dash::tui::run_sidebar_loop(),
        Cmd::ListWorkspaces => workspace::listing::list_workspaces(),
        Cmd::ListBases => workspace::listing::list_bases(),
    }
}

/// Unused now that every subcommand is implemented. Retained as a lever in
/// case we add a new subcommand variant before it has a real handler — the
/// `unreachable!` makes that fail loudly in tests rather than printing a
/// generic clap error.
#[allow(dead_code)]
fn stub(name: &str, args: &[(&str, &str)]) -> Result<()> {
    let argstr = args
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join(" ");
    eprintln!(
        "aw: subcommand '{}' is not yet implemented{}",
        name,
        if argstr.is_empty() { String::new() } else { format!(" (args: {})", argstr) }
    );
    std::process::exit(101)
}
