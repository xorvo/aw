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

## When state files are removed

A pane's state file is deleted once its pane is gone, which keeps the cache from
growing forever. The check is deliberately paranoid: absence from the bulk
`list-panes` reply is only a suspicion, and tmux is asked again about that pane
specifically before anything is unlinked.

The reason is that the deletion is silent and unrecoverable. Nothing rebuilds a
removed file until the pane's agent happens to fire another hook, so a wrong
delete makes a busy agent look like it has never done anything — it drops out of
`aw switch`, and its age reads `—` in `aw dash` and on the phone. An earlier
version only refused to act on a completely *empty* listing, which left a
partial or short reply able to destroy state for live panes.

Every decision is appended to `<state>/gc.log` (self-capping at 64 KB):

```
1790065878 pid=17497 drop %9500 (listing=2 panes, tmux agrees)
1790065878 pid=17497 KEEP %17 — absent from a 2-pane listing but tmux says it is alive
```

A `KEEP` line means the bulk listing and tmux disagreed about the same pane.
That should never happen, and it is exactly the anomaly that used to destroy
state silently, so if files still go missing this log names the process and the
moment.

## Pane options (for tmux-side tooling)

`aw` stamps two pane-local tmux options on every agent pane it knows about:

| Option | Meaning |
|---|---|
| `@aw_agent` | `claude`, `codex`, … |
| `@aw_session_id` | the agent's conversation id, when one is known |

```bash
tmux show-options -p -t "$pane" -v @aw_session_id
```

They exist so a key binding or status format can ask "what is in this pane?"
without knowing anything about the cache layout below. They are written by
`aw hook` on every event that carries a conversation id, and by `aw resurrect`
when it recreates a pane — which is the case that matters, because a resumed
agent fires no hook until somebody types in it, leaving the pane anonymous to
hook-driven tooling until then.

Deliberately namespaced. `aw` does not write `@claude_session_id`: resuming a
conversation makes Claude mint a fresh id, so Claude's own `SessionStart` hook
is the authority on the current one and ours would be stale. If you read both,
prefer that one and fall back to `@aw_session_id`.

Empty values are never written — a pane with no conversation id yet gets
`@aw_agent` only.

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

## `aw switch` — the quick switcher

`aw dash` answers "what is every agent doing?". `aw switch` answers the
narrower question "which agent do I want to be looking at right now?", so it
is a separate, deliberately smaller view: no workspace grouping, no preview,
no control keys.

```
active agents  last 24h   2 waiting   1 working   2 idle

▌1 🔔  Claude Code                                              2m
       claude · team

 2 🔔  Creator portal telemetry gaps                            1h
       claude · automation-qa

 3 ⚡  general-purpose                                          5h
       claude · video-editing

1-9 jump · j/k select · enter jump · q quit
```

One card per **pane**, newest activity first. Cards have no borders — the
hierarchy is typographic: bold pane name, dim agent, accent-coloured
workspace, status glyph in the usual working/waiting/idle colours.

`aw switch --json` prints the same list as data, for external selectors —
see [hammerspoon.md](hammerspoon.md) for an optional system-wide picker built
on it.

| Key | Does |
|---|---|
| `1`–`9` | jump straight to that card |
| `j` / `k`, `↓` / `↑` | move the selection |
| `Enter` | jump to the selection |
| `q` / `Esc` | close |

What it shows, and what it deliberately doesn't:

- **Every live agent pane**, with the most recently active first. A pane we
  know has been quiet for more than 24 hours drops off; a pane with no recorded
  activity at all does not, because unknown is not the same as old. That second
  case is the norm straight after `aw resurrect`, and hiding those was a bug —
  they are exactly the sessions you want to get back to.
- **Agent panes only.** A pane counts when we know its agent: from hook state,
  from the `@aw_agent` stamp, or from the manifest's record for it under this
  server. The tmux label is not enough to go on — a shell would look like an
  agent called "zsh", and an un-hooked Claude pane like one called "2.1.278".
- **Parked panes never appear** — parking is "set this aside", the opposite of
  a jump target.

The layout adapts to the popup: cards cap at 80 columns and centre in a wider
one, the visible count follows the height (with a `+N more` hint in the
footer), and under 8 rows the header and footer drop so cards keep the space.

## Tmux bindings (installed by `aw install tmux-bindings`)

```tmux
bind-key a display-popup -E -w 80% -h 60% "aw dash"
bind-key Space display-popup -E -w 70% -h 60% "aw switch"
bind-key N run-shell "aw dash next-ready"
bind-key C-p run-shell "aw dash park"
bind-key o run-shell "aw dash sidebar"
```

`prefix + Space` overrides tmux's default `next-layout`, the least contested
of the obvious keys — `s` is the session chooser and `Tab` is commonly
rebound to `last-window`. Rebind it after the `aw` marker block if you want
`next-layout` back.

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
