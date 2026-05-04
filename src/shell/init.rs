//! `aw shell-init <zsh|bash|fish>` — print the shell hook for `eval`.
//!
//! The hook does three things:
//!
//! 1. Defines an `aw()` wrapper that intercepts `aw start|enter|open` and
//!    `eval`s the output of `aw _shell-start ...` so the workspace env
//!    applies in the user's shell process.
//! 2. Adds an auto-activation hook on cwd change: when the user `cd`s into
//!    a workspace, exports `AGENT_WORKSPACE` / `AGENT_WORKSPACE_NAME` and
//!    sources global + per-workspace hooks.
//! 3. (Optional, off by default) Adds a workspace-name segment to the right
//!    prompt — left to native shell-prompt frameworks since one-size-fits-all
//!    PROMPT munging tends to fight users' existing setups.

use anyhow::Result;

use crate::cli::ShellKind;

pub fn run(shell: ShellKind) -> Result<()> {
    let text = match shell {
        ShellKind::Zsh => zsh(),
        ShellKind::Bash => bash(),
        ShellKind::Fish => fish(),
    };
    print!("{}", text);
    Ok(())
}

fn zsh() -> String {
    String::from(r#"# >>> aw shell-init (zsh) >>>
# Wrapper that intercepts subcommands needing in-shell side effects.
aw() {
  case "${1-}" in
    start|enter|open)
      shift
      local __aw_script
      __aw_script=$(command aw _shell-start "$@") || return $?
      eval "$__aw_script"
      ;;
    *)
      command aw "$@"
      ;;
  esac
}

# Auto-activation: when cwd changes into (or out of) a workspace, export
# AGENT_WORKSPACE* and source hooks. Idempotent — safe to re-source.
__aw_chpwd() {
  local ws
  ws=$(command aw _detect-workspace "$PWD" 2>/dev/null) || return 0
  if [[ -z $ws ]]; then
    unset AGENT_WORKSPACE AGENT_WORKSPACE_NAME
    return 0
  fi
  export AGENT_WORKSPACE="$ws"
  export AGENT_WORKSPACE_NAME="${ws:t}"
  local h
  for h in "$HOME/.agent-workspaces/hooks.d"/*.sh(N) "$ws/.agent-workspace/hooks.d"/*.sh(N); do
    [[ -r $h ]] && source "$h"
  done
}
typeset -ag chpwd_functions
chpwd_functions+=(__aw_chpwd)
__aw_chpwd

# ---------- Tab completion ----------
# Hand-rolled because clap_complete's static output can't reach into your
# config / workspaces directory. We delegate to internal `aw _list-*`
# subcommands so workspace names update as you create / delete them.
__aw_workspaces() {
  local -a ws
  ws=("${(@f)$(command aw _list-workspaces 2>/dev/null)}")
  [[ ${#ws} -gt 0 ]] && _describe -t workspaces 'workspace' ws
}
__aw_bases() {
  local -a bs
  bs=("${(@f)$(command aw _list-bases 2>/dev/null)}")
  [[ ${#bs} -gt 0 ]] && _describe -t bases 'base' bs
}
_aw() {
  local -a top
  top=(
    'init:Initialize base workspace'
    'create:Create a new workspace from a base'
    'list:List all workspaces' 'ls:List all workspaces'
    'start:Enter workspace' 'enter:Enter workspace' 'open:Enter workspace'
    'delete:Delete a workspace' 'rm:Delete a workspace'
    'config:Show configuration file location'
    'edit-config:Open config in editor'
    'edit-base:Open a base workspace in editor'
    'sync:Sync repos in current workspace'
    'open-home:Open install dir'
    'dash:Tmux-based agent dashboard'
    'hook:Agent state writer (called by hooks)'
    'shell-init:Print shell hook for eval'
    'completions:Print shell completions'
    'install:Interactive setup helpers'
  )
  if (( CURRENT == 2 )); then
    _describe 'subcommand' top
    return
  fi
  case ${words[2]} in
    (start|enter|open|delete|rm)
      __aw_workspaces ;;
    (edit-base)
      __aw_bases ;;
    (init)
      if (( CURRENT == 3 )); then __aw_bases; fi ;;
    (create)
      _arguments '--base[base name]:base:__aw_bases' '*::name:' ;;
    (dash)
      if (( CURRENT == 3 )); then
        local -a sub=(sidebar status-line next-ready park json gc)
        _describe 'dash subcommand' sub
      fi ;;
    (install)
      if (( CURRENT == 3 )); then
        local -a sub=(shell hooks tmux-bindings all)
        _describe 'install subcommand' sub
      elif [[ ${words[3]} == hooks ]]; then
        _arguments '--agent[which agent to wire]:agent:(claude codex pi all)'
      elif [[ ${words[3]} == shell ]]; then
        _arguments '--shell[which shell rc to write]:shell:(zsh bash fish)'
      fi ;;
    (shell-init|completions)
      if (( CURRENT == 3 )); then
        local -a shells=(zsh bash fish)
        _describe 'shell' shells
      fi ;;
    (hook)
      _arguments \
        '--agent[which agent fired]:agent:(claude codex pi)' \
        '--event[event name]:event:' \
        '--prompt[prompt text]:prompt:' ;;
  esac
}
compdef _aw aw
# <<< aw shell-init (zsh) <<<
"#)
}

fn bash() -> String {
    String::from(r#"# >>> aw shell-init (bash) >>>
aw() {
  case "${1-}" in
    start|enter|open)
      shift
      local __aw_script
      __aw_script=$(command aw _shell-start "$@") || return $?
      eval "$__aw_script"
      ;;
    *)
      command aw "$@"
      ;;
  esac
}

__aw_chpwd() {
  local ws
  ws=$(command aw _detect-workspace "$PWD" 2>/dev/null) || return 0
  if [[ -z $ws ]]; then
    unset AGENT_WORKSPACE AGENT_WORKSPACE_NAME
    return 0
  fi
  export AGENT_WORKSPACE="$ws"
  export AGENT_WORKSPACE_NAME="$(basename "$ws")"
  local h
  for h in "$HOME/.agent-workspaces/hooks.d"/*.sh "$ws/.agent-workspace/hooks.d"/*.sh; do
    [[ -r $h ]] && source "$h"
  done
}

# Bash has no chpwd — wrap PROMPT_COMMAND. Save the old PWD between calls
# so we only fire when it actually changes.
__aw_last_pwd="$PWD"
__aw_check_chpwd() {
  if [[ $PWD != $__aw_last_pwd ]]; then
    __aw_last_pwd="$PWD"
    __aw_chpwd
  fi
}
case "$PROMPT_COMMAND" in
  *__aw_check_chpwd*) ;;
  *) PROMPT_COMMAND="__aw_check_chpwd${PROMPT_COMMAND:+;$PROMPT_COMMAND}" ;;
esac
__aw_chpwd

# ---------- Tab completion ----------
# Hand-rolled to support dynamic workspace / base name completion. Mirrors
# the zsh override above; same `aw _list-*` calls.
__aw_complete() {
  local cur prev cmd
  COMPREPLY=()
  cur="${COMP_WORDS[COMP_CWORD]}"
  prev="${COMP_WORDS[COMP_CWORD-1]}"
  cmd="${COMP_WORDS[1]}"

  local subs="init create list ls start enter open delete rm config edit-config edit-base sync open-home dash hook shell-init completions install"
  if [ "$COMP_CWORD" -eq 1 ]; then
    COMPREPLY=( $(compgen -W "$subs" -- "$cur") ); return 0
  fi

  case "$cmd" in
    start|enter|open|delete|rm)
      COMPREPLY=( $(compgen -W "$(command aw _list-workspaces 2>/dev/null)" -- "$cur") ) ;;
    edit-base|init)
      COMPREPLY=( $(compgen -W "$(command aw _list-bases 2>/dev/null)" -- "$cur") ) ;;
    create)
      if [ "$prev" = "--base" ]; then
        COMPREPLY=( $(compgen -W "$(command aw _list-bases 2>/dev/null)" -- "$cur") )
      else
        COMPREPLY=( $(compgen -W "--base" -- "$cur") )
      fi ;;
    dash)
      [ "$COMP_CWORD" -eq 2 ] && COMPREPLY=( $(compgen -W "sidebar status-line next-ready park json gc" -- "$cur") ) ;;
    install)
      if [ "$COMP_CWORD" -eq 2 ]; then
        COMPREPLY=( $(compgen -W "shell hooks tmux-bindings all" -- "$cur") )
      elif [ "$prev" = "--agent" ]; then
        COMPREPLY=( $(compgen -W "claude codex pi all" -- "$cur") )
      elif [ "$prev" = "--shell" ]; then
        COMPREPLY=( $(compgen -W "zsh bash fish" -- "$cur") )
      fi ;;
    shell-init|completions)
      [ "$COMP_CWORD" -eq 2 ] && COMPREPLY=( $(compgen -W "zsh bash fish" -- "$cur") ) ;;
    hook)
      case "$prev" in
        --agent) COMPREPLY=( $(compgen -W "claude codex pi" -- "$cur") ) ;;
        *) COMPREPLY=( $(compgen -W "--agent --event --prompt" -- "$cur") ) ;;
      esac ;;
  esac
  return 0
}
complete -F __aw_complete aw
# <<< aw shell-init (bash) <<<
"#)
}

fn fish() -> String {
    String::from(r#"# >>> aw shell-init (fish) >>>
function aw
  switch $argv[1]
    case start enter open
      set -l rest $argv[2..-1]
      set -l script (command aw _shell-start $rest)
      or return $status
      eval $script
    case '*'
      command aw $argv
  end
end

function __aw_chpwd --on-variable PWD
  set -l ws (command aw _detect-workspace $PWD 2>/dev/null)
  if test -z "$ws"
    set -e AGENT_WORKSPACE
    set -e AGENT_WORKSPACE_NAME
    return 0
  end
  set -gx AGENT_WORKSPACE $ws
  set -gx AGENT_WORKSPACE_NAME (basename $ws)
  for h in $HOME/.agent-workspaces/hooks.d/*.sh $ws/.agent-workspace/hooks.d/*.sh
    if test -r $h
      bass source $h ^/dev/null; or source $h
    end
  end
end
__aw_chpwd

# ---------- Tab completion ----------
complete -c aw -f
# Subcommands.
complete -c aw -n '__fish_use_subcommand' -a 'init create list ls start enter open delete rm config edit-config edit-base sync open-home dash hook shell-init completions install'
# Dynamic args.
complete -c aw -n '__fish_seen_subcommand_from start enter open delete rm' -a '(command aw _list-workspaces 2>/dev/null)'
complete -c aw -n '__fish_seen_subcommand_from edit-base init'             -a '(command aw _list-bases 2>/dev/null)'
complete -c aw -n '__fish_seen_subcommand_from create' -l base -d 'base name' -xa '(command aw _list-bases 2>/dev/null)'
# Dash / install / hook value-enums.
complete -c aw -n '__fish_seen_subcommand_from dash' -a 'sidebar status-line next-ready park json gc'
complete -c aw -n '__fish_seen_subcommand_from install' -a 'shell hooks tmux-bindings all'
complete -c aw -n '__fish_seen_subcommand_from install' -l agent -xa 'claude codex pi all'
complete -c aw -n '__fish_seen_subcommand_from install' -l shell -xa 'zsh bash fish'
complete -c aw -n '__fish_seen_subcommand_from shell-init completions' -a 'zsh bash fish'
complete -c aw -n '__fish_seen_subcommand_from hook' -l agent -xa 'claude codex pi'
# <<< aw shell-init (fish) <<<
"#)
}
