# Agent Workspace (`aw`)

Run multiple AI coding agents in parallel — each in its own isolated repo
checkout — and watch all of them from a single live dashboard.

<!--
TODO: drop a screenshot or GIF of `aw dash` here. The popup is the
headline feature; one image does more than the next 100 lines of prose.
e.g.  ![aw dash](docs/img/dash.png)
-->

## Why

- **Parallel agents, no collisions.** Each workspace is a fresh checkout
  of your repos with its own working tree. Three Claudes can refactor the
  same codebase at the same time without stepping on each other.
- **One place to see them all.** `aw dash` shows every agent across every
  workspace in a tmux popup — what they're working on, who's waiting for
  input, who's idle. Press `enter` to jump to any of them.
- **Built for the agents that actually ship.** Hooks for [Claude
  Code](https://www.anthropic.com/claude-code), Codex, and pi out of the
  box.

## Install (macOS)

```bash
brew tap xorvo/tap
brew install aw
aw install all          # shell hook + agent hooks + tmux bindings
```

Or grab the binary directly:

```bash
TAG=$(curl -fsSL https://api.github.com/repos/xorvo/aw/releases/latest | jq -r .tag_name)
TRIPLE=$(uname -m | sed 's/arm64/aarch64/')-apple-darwin
curl -fsSL "https://github.com/xorvo/aw/releases/download/${TAG}/aw-${TAG}-${TRIPLE}.tar.gz" \
  | tar -xz -C ~/.local/bin
xattr -d com.apple.quarantine ~/.local/bin/aw 2>/dev/null
aw install all
```

Upgrade later with `brew upgrade aw` or `aw self update`.

## 60-second quickstart

```bash
aw edit-config          # tell aw which repos to clone (one-time)
aw init                 # materialize the 'default' base workspace
aw create my-task       # spin up an isolated workspace from the base
aw start my-task        # cd in (auto-activates env, opens tmux)
aw dash                 # see live agent state across all workspaces
```

That's it. Run `claude` (or `codex`, `pi`) inside the workspace and the
dashboard picks it up automatically.

## The dashboard

`aw dash` is a tmux popup TUI showing every active agent, grouped by
workspace.

```
↵ jump to pane           p  park (mute)
/ filter                 n  next-ready (oldest waiting)
H toggle dormant         q  quit
```

Other entry points:

```bash
aw dash sidebar          # narrow always-on side pane
aw dash status-line      # one-liner for tmux's status-right
aw dash next-ready       # one-shot: jump to oldest waiting agent
aw dash --filter         # open straight into search mode
```

The default tmux bindings (after `aw install tmux-bindings`) are
`prefix + a` for the popup and `prefix + /` for filter mode. Full
keymap, hook contract, and state schema in
[`docs/dash.md`](docs/dash.md).

## The phone remote

`aw serve` puts the same live view on your phone (same Wi-Fi): see who's
working / waiting, read any agent's terminal in color, type back, approve
permission prompts, even paste a screenshot. It prints a QR code to pair —
or press `Q` inside `aw dash` anytime. Details in
[`docs/serve.md`](docs/serve.md).

## Configure

`~/.agent-workspaces/config.yaml` (or `aw edit-config`):

```yaml
default:
  repos:
    - git@github.com:your-org/repo1.git
    - git@github.com:your-org/repo2.git
  local_files:
    - ~/projects/local-project
    - "~/projects/infrastructure -> infra"  # copy with renamed destination

development:
  repos:
    - git@github.com:your-org/dev-repo.git
```

Each top-level key is a "base" — a template you can create workspaces
from with `aw create <name> --base <key>`. See
[`docs/base-workspaces.md`](docs/base-workspaces.md).

## Custom hooks

Drop `.sh` files in `~/.agent-workspaces/hooks.d/` (global) or
`<workspace>/.agent-workspace/hooks.d/` (per-workspace). They're sourced
when you enter a workspace, so you can wrap commands, set env vars, or
guard against dangerous operations (e.g. blocking `kubectl --context=production`).

## Going further

- [Architecture & directory layout](docs/architecture.md)
- [Shell integration](docs/shell-integration.md) — auto-activation, prompt frameworks
- [Prompt customization](docs/prompt-customization.md) — p10k, starship, oh-my-zsh
- [Quick command reference](docs/quick-reference.md)
- [Performance notes](docs/performance-considerations.md) — large-repo handling
- [Migrating from the bash CLI](docs/migration.md)
- [Contributing](CONTRIBUTING.md) — building from source, the release process
