#!/bin/bash
# Prompt integration for agent-workspace
# Shows workspace status in shell prompts

# Function to get workspace prompt segment
_aw_prompt_segment() {
    if [ -n "$AGENT_WORKSPACE_NAME" ]; then
        # Use color codes for zsh, plain text for bash
        if [ -n "$ZSH_VERSION" ]; then
            echo "%F{242}[◉ $AGENT_WORKSPACE_NAME]%f"
        else
            echo "[◉ $AGENT_WORKSPACE_NAME]"
        fi
    fi
}

# Zsh integration
if [ -n "$ZSH_VERSION" ]; then
    # Skip all prompt integration if Starship is active
    if [ -n "$STARSHIP_SHELL" ]; then
        # Starship is handling the prompt, don't add native integration
        return 0 2>/dev/null || true
    fi

    # Always define the p10k segment function (it won't hurt if p10k isn't used)
    function prompt_agent_workspace() {
        if [ -n "$AGENT_WORKSPACE_NAME" ]; then
            # Check if p10k segment function exists
            if typeset -f p10k &>/dev/null; then
                p10k segment -f 242 -i '◉' -t "$AGENT_WORKSPACE_NAME"
            else
                # Fallback display
                echo -n "%F{242}[◉ - $AGENT_WORKSPACE_NAME]%f"
            fi
        fi
    }

    # Check if using oh-my-zsh (non-p10k)
    if [ -n "$ZSH_THEME" ] && [ -z "$AW_DISABLE_OMZ_PROMPT" ]; then
        # For oh-my-zsh themes, add to RPS1 (right prompt) to avoid breaking themes
        # Set AW_DISABLE_OMZ_PROMPT=1 to disable this integration
        _aw_setup_omz_prompt() {
            local workspace_segment=$(_aw_prompt_segment)
            if [ -n "$workspace_segment" ]; then
                # Save original RPS1 if not already saved
                if [ -z "$_AW_ORIGINAL_OMZ_RPS1" ]; then
                    _AW_ORIGINAL_OMZ_RPS1="${RPS1:-$RPROMPT}"
                fi
                # Set workspace in right prompt
                RPS1="${workspace_segment}${_AW_ORIGINAL_OMZ_RPS1:+ $_AW_ORIGINAL_OMZ_RPS1}"
                RPROMPT="$RPS1"
            elif [ -n "$_AW_ORIGINAL_OMZ_RPS1" ]; then
                # Restore original right prompt when leaving workspace
                RPS1="${_AW_ORIGINAL_OMZ_RPS1}"
                RPROMPT="$RPS1"
            fi
        }

        # Hook into precmd
        autoload -U add-zsh-hook
        add-zsh-hook precmd _aw_setup_omz_prompt
    else
        # Pure Zsh without oh-my-zsh or p10k
        # Save original prompt if not already saved
        if [ -z "$_AW_ORIGINAL_PROMPT" ]; then
            _AW_ORIGINAL_PROMPT="$PROMPT"
            _AW_ORIGINAL_RPROMPT="$RPROMPT"
        fi

        # Function to update prompt with workspace info
        _aw_update_prompt() {
            local workspace_segment=$(_aw_prompt_segment)
            if [ -n "$workspace_segment" ]; then
                # Add workspace to the beginning of the prompt
                PROMPT="${workspace_segment} ${_AW_ORIGINAL_PROMPT}"
            else
                PROMPT="$_AW_ORIGINAL_PROMPT"
            fi
        }

        # Hook into precmd to update prompt
        autoload -U add-zsh-hook
        add-zsh-hook precmd _aw_update_prompt
    fi

# Bash prompt integration
elif [ -n "$BASH_VERSION" ]; then
    # Save original prompt if not already saved
    if [ -z "$_AW_ORIGINAL_PS1" ]; then
        _AW_ORIGINAL_PS1="$PS1"
    fi

    # Update PS1 to include workspace info
    _aw_update_bash_prompt() {
        local workspace_segment=$(_aw_prompt_segment)
        if [ -n "$workspace_segment" ]; then
            PS1="${workspace_segment} ${_AW_ORIGINAL_PS1}"
        else
            PS1="$_AW_ORIGINAL_PS1"
        fi
    }

    # Set up PROMPT_COMMAND to update prompt
    if [ -z "$PROMPT_COMMAND" ]; then
        PROMPT_COMMAND="_aw_update_bash_prompt"
    else
        PROMPT_COMMAND="_aw_update_bash_prompt; $PROMPT_COMMAND"
    fi
fi

# Environment variable for custom integration
# Users can use $AGENT_WORKSPACE_PROMPT in their custom prompts
export AGENT_WORKSPACE_PROMPT='$(_aw_prompt_segment)'
