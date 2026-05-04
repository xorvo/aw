# Quick Reference

## Basic Commands

```bash
# Initialize base workspace
aw init                 # Initialize default base workspace
aw init test           # Initialize 'test' base workspace

# Create workspaces
aw create new-task     # Create workspace from default base
aw create task2 --base test  # Create from 'test' base

# Manage workspaces
aw list                # List all workspaces
aw start my-workspace  # Enter workspace (type 'exit' to leave)
aw start task --tmux   # Start in tmux session
aw delete workspace    # Delete a workspace

# Configuration and Setup
aw config              # Show configuration file location
aw edit-config         # Open config file in default editor (Cursor/VS Code/nvim)
aw edit-base default   # Open default base workspace directory
aw open-home           # Open ~/.agent-workspaces directory
```

## Environment Variables

- `AW_INSTALL_DIR`: Installation directory (default: `~/.agent-workspaces`)
- `AW_WORKSPACES_DIR`: Where workspaces are created (default: `~/agent-workspaces`)
- `AW_CONFIG_FILE`: Config file path (default: `$AW_INSTALL_DIR/config.yaml`)
- `AW_BIN_DIR`: Where aw command is installed (default: `~/.local/bin`)

## Quick Setup

1. Add to your shell config (e.g., `~/.zshrc` or `~/.bashrc`):

   ```bash
   export PATH="$PATH:$HOME/.local/bin"
   ```

2. Initialize and create your first workspace:
   ```bash
   aw init
   aw create my-first-workspace
   aw start my-first-workspace
   ```

## File Locations

- **Config**: `~/.agent-workspaces/config.yaml`
- **Base Workspaces**: `~/.agent-workspaces/base/`
- **Created Workspaces**: `~/agent-workspaces/` (or `$AGENT_WORKSPACE_ROOT`)
