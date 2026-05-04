#!/bin/bash
# Tab completion for aw command

AW_INSTALL_DIR="${AW_INSTALL_DIR:-$HOME/.agent-workspaces}"
AW_WORKSPACES_DIR="${AW_WORKSPACES_DIR:-$HOME/agent-workspaces}"
AW_CONFIG_FILE="${AW_CONFIG_FILE:-$AW_INSTALL_DIR/config.yaml}"

_aw_complete() {
    local cur prev opts
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"  # Fixed: COMP_CWORD not COMP_CUR
    prev="${COMP_WORDS[COMP_CWORD-1]}"  # Fixed: Calculate prev correctly

    # Main commands (added 'enter' if you use it)
    local commands="init create list start enter open delete config edit-config edit-base open-home help"

    # Get base names from config
    local bases=""
    if [ -f "$AW_CONFIG_FILE" ] && command -v yq &>/dev/null; then
        bases=$(yq -r 'keys | .[]' "$AW_CONFIG_FILE" 2>/dev/null | grep -v "^agent_config$" | grep -v "^workspace_defaults$" | tr '\n' ' ')
    fi

    # Get workspace names
    local workspaces=""
    if [ -d "$AW_WORKSPACES_DIR" ]; then
        for dir in "$AW_WORKSPACES_DIR"/*; do
            if [ -d "$dir" ] && [ -f "$dir/.agent-workspace/name" ]; then
                workspaces="$workspaces $(basename "$dir")"
            fi
        done
    fi

    case "${prev}" in
        aw)
            COMPREPLY=($(compgen -W "${commands}" -- ${cur}))
            return 0
            ;;
        init|edit-base)
            COMPREPLY=($(compgen -W "${bases}" -- ${cur}))
            return 0
            ;;
        create)
            # For --base option completion
            if [[ ${cur} == -* ]]; then
                COMPREPLY=($(compgen -W "--base -b" -- ${cur}))
            fi
            return 0
            ;;
        --base|-b)
            COMPREPLY=($(compgen -W "${bases}" -- ${cur}))
            return 0
            ;;
        start|enter|open|delete|rm)
            COMPREPLY=($(compgen -W "${workspaces}" -- ${cur}))
            return 0
            ;;
        *)
            # Check if we're completing the first argument
            if [ $COMP_CWORD -eq 1 ]; then
                COMPREPLY=($(compgen -W "${commands}" -- ${cur}))
            # Check for options based on the command
            elif [[ ${cur} == -* ]]; then
                case "${COMP_WORDS[1]}" in
                    create)
                        COMPREPLY=($(compgen -W "--base -b" -- ${cur}))
                        ;;
                    start|enter)
                        COMPREPLY=($(compgen -W "--tmux" -- ${cur}))
                        ;;
                esac
            fi
            ;;
    esac
}

# Register completion for aw command
complete -F _aw_complete aw
