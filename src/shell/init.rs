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

# Tab completion. Cheap (~one binary launch per shell startup) and
# self-contained — clap's emitted script registers via `compdef` so this
# works even if oh-my-zsh / prezto already ran compinit.
if (( $+functions[compdef] )) || command -v compinit >/dev/null 2>&1; then
  source <(command aw completions zsh)
fi
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

# Tab completion via the bash-completion plumbing built into bash 4+.
# Clap emits `complete -F _aw -o ... aw` at the end of its output, so
# sourcing inline is enough.
source <(command aw completions bash)
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

# Tab completion.
command aw completions fish | source
# <<< aw shell-init (fish) <<<
"#)
}
