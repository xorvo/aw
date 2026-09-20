# Hammerspoon menu selector (optional)

A system-wide picker for your `aw` agents: hit a hotkey anywhere in macOS, get
a fuzzy-searchable list of agents active in the last 24 hours, pick one, land
in its tmux pane. If a Ghostty window is already attached it is reused and
focused; if none is, a new one opens.

It lists **only `aw`-managed agents** — the panes inside `aw-*` tmux sessions.
An agent you started directly in a terminal window isn't tracked by `aw` and
won't appear.

Strictly optional, and needs two apps `aw` does not otherwise depend on:

- [Hammerspoon](https://www.hammerspoon.org) — the macOS automation host
- [Ghostty](https://ghostty.org) — the terminal

## Install

```bash
aw install hammerspoon            # installs regardless of what's present
aw install hammerspoon --uninstall
```

`aw install all` runs this step **only when both apps are found** in
`/Applications`, `/System/Applications`, or `~/Applications`, so it never nags
anyone who doesn't use them. Asking for it by name skips that check, which is
what you want when setting up before the apps land, or on a machine you're
provisioning.

Two files, both idempotent:

| Path | Role |
|---|---|
| `~/.hammerspoon/aw.lua` | generated; overwritten on every re-install |
| `~/.hammerspoon/init.lua` | gets a `--`-commented `aw` marker block that requires it |

The hotkey lives in `init.lua`, not in the generated file, and the block is
written **once** — a re-install refreshes `aw.lua` but never rewrites an
existing block, so a hotkey you rebound stays rebound:

```lua
-- >>> aw hammerspoon >>>
local aw = require("aw")
aw.bind({ "cmd", "alt" }, "a") -- change the hotkey here; re-installing won't touch it
-- <<< aw hammerspoon <<<
```

After installing, reload Hammerspoon from its menubar icon. Hammerspoon does
not watch its config directory unless you tell it to, so the hotkey won't
exist until that reload.

Hammerspoon also needs macOS Accessibility permission to focus windows. If
picking an agent switches the tmux pane but doesn't bring the window forward,
grant it under System Settings → Privacy & Security → Accessibility.

## The picker

Each row carries a native macOS status dot — red wants your attention, yellow
is busy, green is done — then the agent's own session title, with
`status · workspace · agent · age` underneath.

Typing filters on both lines, so `video wait` narrows by workspace and status
at once. (Searching the subtitle is off by default in `hs.chooser`; the
generated Lua turns it on.)

Two agents can share a headline when they picked the same session title, which
is what the workspace in the subtitle is for. The headline is the agent's title
rather than the workspace because a single workspace often holds several panes.

## Recognising an already-open window

This is the one piece that needs configuration on your side. The switcher
identifies a Ghostty window by its **title**, so tmux has to publish the
session name as the terminal title:

```tmux
set -g set-titles on
set -g set-titles-string "#S"
```

`aw install tmux-bindings` does not set these — they change how your terminal
titles look everywhere, which isn't `aw`'s call to make. Without them the
picker still works and still opens a new window, but it can't tell that one is
already open, so you'll accumulate windows.

The match is exact rather than heuristic: the titles being searched for come
from `tmux list-clients`, and a non-tmux window titles itself after its
directory or running program, so the two can't collide.

## What happens when you pick something

1. `tmux list-clients` says which ttys are attached to which sessions.
2. If a client is on the target session, that window is focused and only the
   pane changes. Otherwise any Ghostty window that is a tmux client gets
   steered to the target with `switch-client -c <tty> -t <pane>`.
3. With nothing attached anywhere, the session is pointed at the right window
   and pane first (both work with no client attached), then a new Ghostty
   window opens onto it.

### Why a second Ghostty process appears

macOS only forwards `-e <command>` to Ghostty through `open -na`, and `-n`
always starts a new instance — `open -a` without it silently drops the
arguments. So opening a window this way yields a second Ghostty process. It
exits by itself when its last window closes, and the picker enumerates windows
across every running instance, so nothing gets lost in the meantime.

## The data contract

The Lua shells out to exactly one `aw` command:

```bash
aw switch --json
```

which prints the same list, filter, and order as the `aw switch` TUI:

```json
[
  {
    "pane_id": "%0",
    "session": "aw-team",
    "workspace": "team",
    "agent": "claude",
    "name": "Claude Code",
    "status": "waiting",
    "last_activity": 1789882344,
    "age_secs": 120,
    "age": "2m"
  }
]
```

`session` is what a GUI caller matches against a window title. `name` is the
pane name the cards show, which `aw dash json` cannot give you — `PaneState`
marks that field `#[serde(skip)]` because it is recomputed from tmux on every
load.

Use it for any other launcher too (Raycast, Alfred, skhd); the Lua is a
reference implementation, not the only possible client.

## Notes

- Binary paths are substituted into `aw.lua` at install time. Hammerspoon runs
  with a bare `PATH`, and going through a login shell to fix that would pick up
  shell aliases — oh-my-zsh aliases `tmux` to a wrapper function that doesn't
  exist non-interactively.
- Those paths are resolved from `PATH`, not from the running binary. Under
  Homebrew the running binary lives at a version-pinned Cellar path that the
  next `brew upgrade` deletes, while `/opt/homebrew/bin/aw` is a stable
  symlink. Re-run `aw install hammerspoon` if you move `aw` somewhere new.
- `aw` resolves the `tmux` binary itself rather than trusting `PATH`. A
  GUI-launched process gets a minimal `PATH` with no Homebrew on it, and tmux
  then looks *absent* rather than broken, so the dashboard silently falls back
  to reading state files only: no live pane names, no refreshed status. That
  is why an earlier version of this menu showed every row as "claude".
- Cosmetic chooser settings are applied inside a `pcall`, so an API difference
  in some Hammerspoon version can leave the picker plainer but never stop it
  from opening.
