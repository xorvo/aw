#!/bin/bash
# Shell hook for agent-workspace
# Auto-detects and activates workspaces when you cd into them

# set variables with default values
AW_INSTALL_DIR="${AW_INSTALL_DIR:-$HOME/.agent-workspaces}"
AW_WORKSPACES_DIR="${AW_WORKSPACES_DIR:-$HOME/agent-workspaces}"
AW_CONFIG_FILE="${AW_CONFIG_FILE:-$AW_INSTALL_DIR/config.yaml}"

# Function to detect and activate workspace
_aw_detect_workspace() {
    local dir="$PWD"
    local workspace_root=""
    local workspace_name=""

    # Search up the directory tree for a workspace marker
    while [ "$dir" != "/" ]; do
        if [ -f "$dir/.agent-workspace/name" ]; then
            workspace_root="$dir"
            workspace_name=$(cat "$dir/.agent-workspace/name" 2>/dev/null)
            break
        fi
        dir=$(dirname "$dir")
    done

    # Check if we've moved to a different workspace or left one
    if [ "$workspace_root" != "$AGENT_WORKSPACE" ]; then
        if [ -n "$workspace_root" ]; then
            # Entering or switching workspace
            export AGENT_WORKSPACE="$workspace_root"
            export AGENT_WORKSPACE_NAME="$workspace_name"

            # Source workspace hooks (global then per-workspace)
            local global_hooks_dir="$AW_INSTALL_DIR/hooks.d"
            local ws_hooks_dir="$workspace_root/.agent-workspace/hooks.d"
            if [ -d "$global_hooks_dir" ]; then
                for hook in "$global_hooks_dir"/*.sh; do
                    [ -f "$hook" ] && source "$hook"
                done
            fi
            if [ -d "$ws_hooks_dir" ]; then
                for hook in "$ws_hooks_dir"/*.sh; do
                    [ -f "$hook" ] && source "$hook"
                done
            fi

            # Show activation message
            echo "🎯 Activated workspace: $workspace_name"
        elif [ -n "$AGENT_WORKSPACE" ]; then
            # Leaving a workspace
            echo "👋 Deactivated workspace: $AGENT_WORKSPACE_NAME"
            unset AGENT_WORKSPACE
            unset AGENT_WORKSPACE_NAME
        fi
    fi
}

# Hook into shell's directory change
# For Zsh
if [ -n "$ZSH_VERSION" ]; then
    # Add to precmd hooks (runs before each prompt)
    autoload -U add-zsh-hook
    add-zsh-hook chpwd _aw_detect_workspace
    # Also run on shell startup
    _aw_detect_workspace
# For Bash
elif [ -n "$BASH_VERSION" ]; then
    # Override cd to detect workspace changes
    cd() {
        builtin cd "$@"
        _aw_detect_workspace
    }
    # Also run on shell startup
    _aw_detect_workspace
fi

# Source prompt integration if available
if [ -f "$AW_INSTALL_DIR/bin/aw-prompt-integration.sh" ]; then
    source "$AW_INSTALL_DIR/bin/aw-prompt-integration.sh"
fi

# Source tmux integration if available and in tmux
if [ -n "$TMUX" ] && [ -f "$AW_INSTALL_DIR/bin/aw-tmux-integration.sh" ]; then
    source "$AW_INSTALL_DIR/bin/aw-tmux-integration.sh"
fi
