# Shell Integration

## Overview

Shell integration provides **automatic workspace activation** - just `cd` into a workspace and it activates automatically! No more manual environment setup.

## Features

### Auto-Activation

When you `cd` into a workspace directory:

- Environment variables are set automatically
- Workspace hooks are sourced (global and per-workspace)
- Shows "Activated workspace: name" message

When you `cd` out of a workspace:

- Environment is cleaned up
- Shows "Deactivated workspace: name" message
- Returns to normal shell environment

### AI Agent Support

New shells spawned by AI agents automatically:

- Detect if they're in a workspace
- Set up the correct environment
- Source any configured hooks

## The Problem It Solves

Without shell integration:

```bash
cd ~/agent-workspaces/my-workspace
aw open my-workspace       # Manual activation needed
```

With shell integration:

```bash
cd ~/agent-workspaces/my-workspace
# 🎯 Activated workspace: my-workspace  (automatic!)
```

## Installation

### Automatic Setup

Run the setup script:

```bash
~/.agent-workspaces/bin/setup-shell-integration.sh
```

This will add the integration to:

- `~/.zshrc` (for Zsh)
- `~/.bashrc` (for Bash)
- `~/.bash_profile` (for Bash on macOS)

### Manual Setup

Add this line to your shell configuration file:

```bash
[ -f "$HOME/.agent-workspaces/bin/aw-shell-hook.sh" ] && source "$HOME/.agent-workspaces/bin/aw-shell-hook.sh"
```

## How It Works

1. **Detection**: The hook monitors directory changes and checks for `.agent-workspace/name` markers
2. **Activation**: Sets `AGENT_WORKSPACE` and `AGENT_WORKSPACE_NAME` environment variables
3. **Hooks**: Sources scripts from `~/.agent-workspaces/hooks.d/` (global) and `<workspace>/.agent-workspace/hooks.d/` (per-workspace)
4. **Deactivation**: Cleans up environment variables when you leave a workspace

## What Gets Set Up

When you're in a workspace with shell integration:

```bash
# These are automatically available:
echo $AGENT_WORKSPACE           # /path/to/current/workspace
echo $AGENT_WORKSPACE_NAME      # workspace name
```

## Custom Hooks

You can add custom shell scripts to be sourced on workspace activation:

```bash
# Global hooks (apply to all workspaces)
mkdir -p ~/.agent-workspaces/hooks.d
cat > ~/.agent-workspaces/hooks.d/kubectl-wrapper.sh << 'EOF'
kubectl() {
    if [[ "$*" =~ --context=production ]]; then
        echo "❌ Production context blocked in agent workspace" >&2
        return 1
    fi
    command kubectl "$@"
}
EOF

# Per-workspace hooks
mkdir -p ~/agent-workspaces/my-workspace/.agent-workspace/hooks.d
```

## Testing the Integration

1. Create and start a workspace:

```bash
aw create test-integration
aw open test-integration
```

2. Open a new shell/terminal and navigate to the workspace:

```bash
cd ~/agent-workspaces/test-integration
```

3. Verify activation:

```bash
echo $AGENT_WORKSPACE_NAME  # Should show "test-integration"
```

## Compatibility

The shell hook is compatible with:

- **Zsh** (macOS default)
- **Bash**
- **POSIX-compliant shells**

It works with:

- Terminal.app
- iTerm2
- VS Code integrated terminal
- Cursor integrated terminal
- tmux/screen sessions

## Security Considerations

1. **Workspace Validation**: Only activates when a valid `.agent-workspace/name` marker is found
2. **Hook Order**: Global hooks run first, then per-workspace hooks (per-workspace can override)
3. **Minimal Overhead**: Quick directory check that doesn't slow shell startup

## Troubleshooting

### Workspace not auto-activating

Check if the hook is installed:

```bash
grep "aw-shell-hook" ~/.zshrc
```

Verify environment variables are set:

```bash
echo $AGENT_WORKSPACE
```

### Performance issues

The hook is designed to be fast, but if you experience slow shell startup:

1. Check if `AGENT_WORKSPACE` points to a valid directory
2. Ensure the workspace isn't on a slow network drive

### Removing the Integration

To disable shell integration, remove the line containing `aw-shell-hook.sh` from:

- `~/.zshrc`
- `~/.bashrc`
- `~/.bash_profile`
