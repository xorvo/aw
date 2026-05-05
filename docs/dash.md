# `aw dash` — agent dashboard

A tmux-aware control plane for the AI agents running across your `aw`
workspaces. `aw dash` shows their live state (working / waiting / idle) and
lets you jump straight into whichever pane needs you.

## Quick start

```bash
aw install all                 # one-shot: shell hook + agent hooks + tmux bindings
# (or piecewise: aw install hooks --agent claude, etc.)

# In a tmux session created by `aw start`, run a Claude / Codex / pi session
# as you normally would. Hooks fire automatically.

# In any tmux pane:
aw dash                        # full-screen popup TUI
# or bind it: prefix + a      (installed by `aw install tmux-bindings`)
```

## What's tracked

One row per tmux pane that has had at least one agent event. Each row
records:

| Field | Source |
|---|---|
| `pane_id` (`%42`) | `$TMUX_PANE` at hook fire time |
| `session` | `tmux display-message -p '#{session_name}'` |
| `workspace` | `$AGENT_WORKSPACE_NAME` (set by `aw start`) |
| `agent` | passed by hook (claude / codex / pi) |
| `status` | mapped from event (see table below) |
| `last_event` | last event name fired |
| `last_activity` | unix epoch of last hook fire |
| `last_prompt` | from hook stdin payload (preserved across events) |

## Event → status mapping

| Agent | Event | Status |
|---|---|---|
| claude | `UserPromptSubmit`, `PreToolUse` | working |
| claude | `Notification` | waiting |
| claude | `Stop` | idle |
| codex | `SessionStart` | idle |
| codex | `UserPromptSubmit`, `PreToolUse` | working |
| codex | `Stop` | idle |
| pi | `agent_start`, `input` | working |
| pi | `agent_end` | idle |

Unknown events are silent no-ops — a misconfigured hook can never break the
agent.

## State files

State lives at `~/.cache/aw/panes/<pane_id>.json` (overridable via
`$AW_STATE_DIR`). Writes are atomic (tempfile + rename). Parked sentinels
live alongside at `~/.cache/aw/parked/<pane_id>` — empty file = parked.

```json
{
  "schema_version": 1,
  "pane_id": "%42",
  "session": "aw-my-feature",
  "workspace": "my-feature",
  "cwd": "/Users/me/agent-workspaces/my-feature",
  "agent": "claude",
  "status": "working",
  "last_event": "UserPromptSubmit",
  "last_activity": 1736812345,
  "last_prompt": "fix the auth middleware"
}
```

## Key bindings (popup)

| Key | Action |
|---|---|
| `j` / `↓` | next pane |
| `k` / `↑` | prev pane |
| `Enter` | `tmux switch-client` to selected pane |
| `Tab` | toggle the pane preview (last 60 lines via `tmux capture-pane`) |
| `/` | fuzzy filter on workspace + agent + last prompt |
| `p` | toggle parked (parked panes don't count toward "needs attention") |
| `n` | jump to oldest waiting pane (or idle if none waiting) |
| `r` | refresh |
| `Space` | collapse / expand workspace under cursor |
| `q` / `Esc` | quit |

## Tmux bindings (installed by `aw install tmux-bindings`)

```tmux
bind-key a display-popup -E -w 80% -h 60% "aw dash"
bind-key N run-shell "aw dash next-ready"
bind-key C-p run-shell "aw dash park"
bind-key o run-shell "aw dash sidebar"
```

## Other dashboard subcommands

| Command | Purpose |
|---|---|
| `aw dash json` | dump state snapshot to stdout (for scripts / external UIs) |
| `aw dash gc` | prune state files for tmux panes that no longer exist |
| `aw dash status-line` | one-line summary for tmux's `status-right` |
| `aw dash next-ready` | `switch-client` to oldest waiting / idle pane |
| `aw dash park [--pane <id>]` | toggle parked sentinel (default: current pane) |
| `aw dash sidebar` | spawn a 42-col side pane that auto-refreshes |

## Status-line wiring

Add to your `.tmux.conf`:

```tmux
set -g status-right '#(aw dash status-line) | %H:%M'
```

Output format:

- All idle / empty: nothing (transparent).
- Mixed: `⚡ 2 working  ⏸ 1 waiting  ✓ 3 idle`.

## Notifications

When any agent flips to `waiting`, `aw hook` fires a system notification
(`notify-rust`: macOS Notification Center / Linux D-Bus). Disable with
`AW_DASH_NOTIFY=0`.

## Disable hook firing in some context

The hook silently no-ops outside a tmux pane (no `$TMUX_PANE`). To skip
firing inside tmux too, point `aw` at a different binary or unset the hook
in your agent config — there's no opt-in flag because no-op-when-not-tmux
is already the safe default.
