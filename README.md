# Agent Workspace (aw)

A helper to setup sub-workspaces in your local environment which will be optimized for working with AI Agents like [Claude Code](https://www.anthropic.com/claude-code).

## Installation

```bash
./install.sh
```

This will:

1. Install the `aw` command to `~/.local/bin` (or `$AW_BIN_DIR`)
2. Create installation directory at `~/.agent-workspaces` (or `$AW_INSTALL_DIR`)
3. Create workspaces directory at `~/agent-workspaces` (or `$AW_WORKSPACES_DIR`)
4. Copy configuration template and scripts
5. Offer to set up shell integration (auto-activation, completion, prompt indicator)

## Configuration

The configuration file is located at `$AW_INSTALL_DIR/config.yaml` (default: `~/.agent-workspaces/config.yaml`).

```yaml
# Example configuration
default:
  repos:
    - git@github.com:your-org/repo1.git
    - git@github.com:your-org/repo2.git
  local_files:
    - ~/projects/local-project
    - "~/projects/infrastructure -> infra"  # Copy with custom destination name

development:
  repos:
    - git@github.com:your-org/dev-repo.git
```

## Usage

### 1. Initialize a Base Workspace

Base workspaces are templates stored in `$AW_INSTALL_DIR/base/`:

```bash
aw init                  # Initialize 'default' base
aw init development     # Initialize 'development' base
```

This clones repos and copies files according to your config into `~/.agent-workspaces/base/[name]/`.

### 2. Create a Workspace

Create actual workspaces in `$AW_WORKSPACES_DIR`:

```bash
aw create my-feature              # Create from 'default' base
aw create my-feature --base dev   # Create from 'dev' base
```

This creates a new workspace at `~/agent-workspaces/my-feature/`.

### 3. Start Working

```bash
aw start my-feature              # Enter workspace (auto-activates)
aw start my-feature --tmux      # Start in tmux session
```

When you enter a workspace:

- Sets `AGENT_WORKSPACE` to the workspace path
- Sets `AGENT_WORKSPACE_NAME` to the workspace name
- Sources custom hooks from `hooks.d/` (command wrappers, env vars, etc.)
- Shows workspace in your shell prompt (if configured)

### 4. List and Manage

```bash
aw list                  # List all workspaces
aw delete my-feature     # Delete a workspace
```

### 5. Quick Access Commands

```bash
aw edit-config           # Open config file in editor
aw edit-base default     # Browse/edit base workspace
aw open-home            # Browse installation directory
```

## Shell Integration Features

### Auto-Activation

When you `cd` into a workspace, it automatically:

- Sets environment variables
- Sources workspace hooks
- Updates your shell prompt

### Prompt Integration

Shows `[◉ workspace-name]` in your prompt:

- **Powerlevel10k**: Add `agent_workspace` to `~/.p10k.zsh` prompt elements
- **Oh-My-Zsh**: Automatic right prompt integration
- **Starship**: Run `$AW_INSTALL_DIR/bin/setup-starship.sh`
- **Tmux**: Run `$AW_INSTALL_DIR/bin/setup-tmux-status.sh`
- **Native Zsh/Bash**: Automatic integration

### Tab Completion

Complete workspace names and command options:

```bash
aw create <TAB>          # Complete base names
aw start <TAB>           # Complete workspace names
aw delete <TAB>          # Complete workspace names
```

## Custom Hooks

Extend workspace behavior with shell scripts in `hooks.d/` directories. Hooks are sourced on workspace activation.

```bash
# Global hooks (apply to all workspaces)
mkdir -p ~/.agent-workspaces/hooks.d

# Example: wrap kubectl to block production context
cat > ~/.agent-workspaces/hooks.d/kubectl-guard.sh << 'HOOK'
kubectl() {
    if [[ "$*" =~ --context=production ]]; then
        echo "❌ Production context blocked in agent workspace" >&2
        return 1
    fi
    command kubectl "$@"
}
HOOK

# Per-workspace hooks
mkdir -p ~/agent-workspaces/my-workspace/.agent-workspace/hooks.d
```

Global hooks run first, then per-workspace hooks (which can override).

## Environment Variables

Customize the directory structure:

```bash
# In your shell config (.zshrc, .bashrc)
export AW_INSTALL_DIR="$HOME/.config/agent-workspace"  # Change installation directory
export AW_WORKSPACES_DIR="$HOME/work/ai-workspaces" # Change workspaces location
export AW_BIN_DIR="/usr/local/bin"                  # Change CLI install location
```

## Directory Structure

In general, `aw` will only be touching the following 3 directories.

| Directory                  | Environment Variable | Default Location      | Purpose                                                |
| -------------------------- | -------------------- | --------------------- | ------------------------------------------------------ |
| **Installation Directory** | `AW_INSTALL_DIR`     | `~/.agent-workspaces` | Configuration, base workspaces, scripts, and templates |
| **Workspaces**             | `AW_WORKSPACES_DIR`  | `~/agent-workspaces`  | Your actual working workspaces                         |
| **CLI Binary**             | `AW_BIN_DIR`         | `~/.local/bin`        | Where the `aw` command is installed                    |

## File Structure Overview

```
~/.agent-workspaces/              # Installation Directory (AW_INSTALL_DIR)
├── config.yaml                   # Your configuration
├── hooks.d/                      # Global hooks (sourced for all workspaces)
│   └── my-guard.sh
├── base/                         # Base workspace templates
│   ├── default/                  # Default base workspace
│   │   ├── .agent-workspace/    # Template files & repo cache
│   │   ├── CLAUDE.md            # Agent instructions
│   │   └── AGENTS.md
│   └── development/             # Another base workspace
└── bin/                         # Shell integration scripts
    ├── aw-shell-hook.sh
    ├── aw-completion.sh
    └── setup-*.sh

~/agent-workspaces/              # Workspaces Directory (AW_WORKSPACES_DIR)
├── my-feature/                  # A workspace
│   ├── repo1/
│   ├── repo2/
│   ├── CLAUDE.md                # Agent instructions (symlinked from base)
│   ├── AGENTS.md
│   └── .agent-workspace/
│       ├── name                 # Workspace name
│       ├── base                 # Base it was created from
│       ├── created              # Creation timestamp
│       └── hooks.d/             # Per-workspace hooks
└── bugfix-123/                  # Another workspace

~/.local/bin/                    # CLI Binary Directory (AW_BIN_DIR)
└── aw                           # The main command
```

## Tips

1. **Base Workspaces**: Think of these as templates. Set them up once, create many workspaces from them.

2. **Workspace Lifecycle**: Workspaces are meant to be ephemeral. Create, use, delete per task.

3. **Multiple Bases**: Create different bases for different project types or teams.

4. **Custom Locations**: Set environment variables to organize directories your way.

## Troubleshooting

### Command Not Found

If `aw` command is not found, ensure `$AW_BIN_DIR` is in your PATH:

```bash
export PATH="$PATH:$HOME/.local/bin"
```

### Workspace Not Auto-Activating

Ensure shell integration is sourced:

```bash
source $AW_INSTALL_DIR/bin/aw-shell-hook.sh
```

### Configuration Issues

Check your config file:

```bash
aw config                # Show config location
aw edit-config          # Edit configuration
```
