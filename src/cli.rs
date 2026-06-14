use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "aw",
    version,
    about = "Manage isolated workspaces for AI agents",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Initialize base workspace (default: 'default')
    Init {
        #[arg(default_value = "default")]
        base: String,
    },
    /// Create a new workspace from a base
    Create {
        name: String,
        #[arg(long, default_value = "default")]
        base: String,
    },
    /// List all workspaces
    #[command(alias = "ls")]
    List,
    /// Enter workspace (tmux session if available)
    #[command(alias = "enter", alias = "open")]
    Start {
        name: String,
        #[arg(long)]
        no_tmux: bool,
    },
    /// Delete a workspace
    #[command(alias = "rm")]
    Delete { name: String },
    /// Show configuration file location
    Config,
    /// Open config file in default editor
    EditConfig,
    /// Open base workspace directory in editor
    EditBase {
        #[arg(default_value = "default")]
        base: String,
    },
    /// Sync all repos in current workspace with remote
    Sync,
    /// Reset all repos in current workspace to the remote default branch
    Reset {
        /// Force reset, discarding uncommitted changes and divergent commits
        #[arg(long)]
        hard: bool,
    },
    /// Open tool home directory in editor
    OpenHome,

    /// Tmux-based dashboard for live agent state
    Dash {
        /// Open the popup directly in filter (search) mode.
        /// Has no effect when a subcommand is given.
        #[arg(long)]
        filter: bool,
        #[command(subcommand)]
        command: Option<DashCmd>,
    },

    /// Serve the phone remote-control web app over the LAN
    Serve {
        /// Listen host (default: $AW_REMOTE_HOST or 0.0.0.0)
        #[arg(long)]
        host: Option<String>,
        /// Listen port (default: $AW_REMOTE_PORT or 7340)
        #[arg(long)]
        port: Option<u16>,
    },

    /// Write an agent state transition (called by Claude/Codex/pi hooks)
    Hook {
        #[arg(long)]
        agent: AgentKind,
        #[arg(long)]
        event: String,
        #[arg(long)]
        prompt: Option<String>,
    },

    /// Print shell hook for eval (e.g. `eval "$(aw shell-init zsh)"`)
    ShellInit { shell: ShellKind },

    /// Print shell completions
    Completions { shell: ShellKind },

    /// Interactive setup helpers
    Install {
        #[command(subcommand)]
        command: InstallCmd,
    },

    /// Self-management: check for updates, upgrade in place
    #[command(name = "self")]
    SelfMgmt {
        #[command(subcommand)]
        command: SelfCmd,
    },

    // ---- internal subcommands (hidden) ----
    /// Internal: emit shell snippet for `aw start` (called by the shell wrapper)
    #[command(name = "_shell-start", hide = true)]
    ShellStart {
        name: String,
        #[arg(long)]
        no_tmux: bool,
    },
    /// Internal: detect the workspace path containing `cwd` (for auto-activation)
    #[command(name = "_detect-workspace", hide = true)]
    DetectWorkspace { cwd: String },
    /// Internal: sidebar redraw loop (called by `aw dash sidebar`)
    #[command(name = "_sidebar-loop", hide = true)]
    SidebarLoop,
    /// Internal: list workspace names (used by tab completion)
    #[command(name = "_list-workspaces", hide = true)]
    ListWorkspaces,
    /// Internal: list base names (used by tab completion)
    #[command(name = "_list-bases", hide = true)]
    ListBases,
}

#[derive(Subcommand, Debug)]
pub enum DashCmd {
    /// Spawn a sidebar pane in the current tmux session
    Sidebar,
    /// Print one-line summary for tmux status-right
    StatusLine,
    /// Switch-client to oldest waiting (or idle) pane
    NextReady,
    /// Toggle parked sentinel for a pane (default: current)
    Park {
        #[arg(long)]
        pane: Option<String>,
    },
    /// Toggle pinned sentinel for a workspace (floats it to the top in `aw dash`)
    Pin {
        workspace: String,
    },
    /// Dump full state snapshot as JSON
    Json,
    /// Prune state files for dead panes
    Gc,
}

#[derive(Subcommand, Debug)]
pub enum SelfCmd {
    /// Check whether a newer release is available on GitHub
    Check,
    /// Download the latest release and replace this binary in place
    Update {
        /// Skip the confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum InstallCmd {
    /// Append `eval "$(aw shell-init <shell>)"` to the user's rc file
    Shell {
        #[arg(long)]
        shell: Option<ShellKind>,
    },
    /// Wire agent hooks into Claude/Codex/pi configs
    Hooks {
        #[arg(long)]
        agent: Option<AgentKind>,
    },
    /// Append tmux key bindings to your tmux config file. We auto-detect
    /// `~/.config/tmux/tmux.conf` (preferred) and `~/.tmux.conf` (legacy);
    /// pass `--config <path>` to override.
    TmuxBindings {
        #[arg(long)]
        config: Option<std::path::PathBuf>,
    },
    /// Install a launchd LaunchAgent so `aw serve` (phone remote) runs at
    /// login and is kept alive (macOS). Re-run to update it.
    Service {
        /// Remove the service instead of installing it
        #[arg(long)]
        uninstall: bool,
        /// Bind host for the daemon (default: 0.0.0.0)
        #[arg(long)]
        host: Option<String>,
        /// Listen port for the daemon (default: 7340)
        #[arg(long)]
        port: Option<u16>,
    },
    /// Run shell + hooks + tmux-bindings + serve-at-login interactively
    All,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
#[value(rename_all = "lower")]
pub enum AgentKind {
    Claude,
    Codex,
    Pi,
    All,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
#[value(rename_all = "lower")]
pub enum ShellKind {
    Zsh,
    Bash,
    Fish,
}
