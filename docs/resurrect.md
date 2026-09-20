# Resurrect — surviving tmux-server death

`aw`'s workspaces live on disk, but the tmux sessions that host them live in
the tmux server's memory. When that server dies — an agent runs
`tmux kill-server`, a power cut, a reboot — every `aw-*` session and running
agent goes with it. `aw resurrect` brings them back.

```bash
aw resurrect --dry-run   # show what would be restored
aw resurrect             # recreate sessions + resume agents
aw snapshot              # save the live layout now (before a planned shutdown)
```

## What it restores

For each lost session it recreates `aw-<workspace>` rooted in the workspace
directory, then types the agent's **resume command** into the fresh pane.
The agent CLIs persist conversations on disk, so the agent picks its
conversation back up. Agent hooks report their conversation id
(`session_id` in the hook payload), and when one was recorded the *exact*
conversation is reopened — precise even with several agent sessions in the
same directory. Without an id, the `--continue`-style fallback reopens the
directory's most recent conversation:

| Agent    | With recorded id            | Fallback               |
|----------|-----------------------------|------------------------|
| claude   | `claude --resume <id>`      | `claude --continue`    |
| codex    | `codex resume <id>`         | `codex resume --last`  |
| opencode | `opencode --session <id>`   | `opencode --continue`  |
| kimi     | `kimi --session <id>`       | `kimi --continue`      |

Interactive-picker forms (bare `claude --resume`) are deliberately not the
default: resurrect runs with nobody at the keyboard. Override or disable
per agent in `config.yaml` — a `{session_id}` placeholder is substituted
(shell-quoted) when an id is available:

```yaml
agent_config:
  resume_commands:
    claude: "claude --resume {session_id} --verbose"
    codex: ""                      # plain shell, don't relaunch codex
```

Each restored session gets one window per recorded (agent, directory,
conversation) triple; only id-less duplicates in the same directory
collapse, since the fallback can only reopen that directory's latest
conversation anyway.

## `aw snapshot` — planned shutdowns

The hook-driven manifest only knows about panes where agent hooks fired.
`aw snapshot` captures the **live tmux truth** on demand: every window in
every `aw-*` session, including agents that never fired a hook (recognized
by their foreground command, or by what the manifest already recorded for
that pane under the same server — a resurrected agent nobody has typed into
yet). Run it before rebooting; after boot, `aw resurrect` rebuilds the lot.

Panes with nothing to resume — plain shells, or an agent `aw` cannot
identify (e.g. a freshly launched Claude that never fired a hook; its
native binary reports its version string as the foreground command) — are
**not** restored, and a session consisting only of such panes is dropped
rather than recreated as an empty shell.

The snapshot is authoritative for the server it sees: records it proves
were closed on purpose are dropped, while crash-survivor records from an
older server are left untouched.

## How it knows what existed

Every `aw hook` event and every session creation (`aw start`, dash open)
shadows the session into a durable manifest at
`~/.cache/aw/sessions.json` (`$AW_STATE_DIR/sessions.json`), keyed by
**session name** — unlike pane ids, which tmux reassigns on every server
restart. No extra daemon, no periodic snapshotting: the hooks that already
fire on every agent event keep it fresh.

## Deliberate kill vs. crash

Restoring sessions you closed on purpose would be worse than useless, so
each record carries the pid of the tmux server it was last seen under:

- Session missing, live server has the **same pid** → the server never died,
  so the session was closed on purpose → pruned from the manifest.
- Session missing, server gone or running under a **different pid** → crash
  survivor → restored.

This is also why opening a plain tmux session after a reboot doesn't erase
your recovery state: the new server's pid doesn't match, so the records are
left alone until you resurrect.

## What the dashboard shows afterwards

Pane state files (`~/.cache/aw/panes/*.json`) are keyed by tmux pane id, so
they die with the server and the dash garbage-collects them on its next
tick. To stop a restored agent from showing up with no history at all,
resurrect seeds a state file for each pane it creates, carrying the agent,
conversation id, and the **original pane's last-activity time** over from
the manifest. A session you last touched three days ago still reads `3d`
after a restore, not "just now".

Status resets to `idle`, and last event / last prompt come back empty — the
manifest never recorded those. The agent's first hook after the restore
fills them in.

## What is *not* restored

Window layouts, scrollback, shell history, and processes other than the
recorded agents. The workspace files were never at risk — they live in
`~/agent-workspaces/<name>/` and survive any crash.
