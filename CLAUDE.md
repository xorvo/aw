# CLAUDE.md

Guidance for Claude Code (claude.ai/code) working in this repository.

## Project Overview

Agent Workspace (`aw`) is a **Rust CLI** that manages isolated repo checkouts
for AI coding agents (Claude Code, Codex, pi), plus a tmux-based dashboard that
shows every agent's live state. Multiple agents work in parallel without
colliding, and `aw dash` surfaces who's working / waiting / idle.

> Historical note: `aw` began as a bash script embedded in `install.sh`. It has
> since been ported to Rust. That bash version is now **frozen** at
> `tests/fixtures/aw-bash` and used only as the parity reference for tests —
> don't edit it (see below). `install.sh` today just runs `cargo build` and
> places the binary.

## Architecture

The CLI is a Rust binary (`src/`). Key modules:

- **src/main.rs / cli.rs** — clap dispatch + subcommand surface
- **src/workspace/** — `init` / `create` / `list` / `start` / `delete` / `sync` / `reset` / `edit-*`
- **src/dash/** — the popup TUI, sidebar, hook state (`~/.cache/aw/panes/*.json`), tmux merge
- **src/shell/** — `shell-init`, completions, workspace detection
- **src/install/** — `aw install …` (shell rc, agent hooks, tmux bindings)
- **src/hook.rs** — `aw hook` (agent state writer, called from agent hooks)
- **src/config.rs** — `config.yaml` parser (serde_yaml; no `yq` at runtime)
- **src/paths.rs**, **src/git.rs**, **src/self_update.rs**

Full layout and conventions live in [CONTRIBUTING.md](CONTRIBUTING.md);
dashboard state schema + hook contract in [docs/dash.md](docs/dash.md).

## Build & test

```bash
cargo build                       # debug build
cargo build --release             # optimized (what releases ship)
cargo test --tests                # full suite (parity + rust-only)
cargo test --test parity_create   # a single test file
INSTA_UPDATE=always cargo test --tests && cargo insta review  # update snapshots
./install.sh                      # build + place binary + bootstrap config
```

Tests sandbox `$HOME`, the state dir, and the tmux socket per test
(`tests/common/`). Some spawn a real tmux/zsh, so those tools must be installed
locally (CI installs `tmux`, `zsh`, `jq`, `yq`).

**Frozen bash baseline:** `tests/fixtures/aw-bash` is the parity reference,
frozen at commit `3ba2893`. Don't edit it. Any intentional divergence from bash
behavior gets a parity-snapshot update in the same commit plus a one-line note.

## Common runtime commands

```bash
aw init            # materialize the 'default' base workspace
aw create my-task  # isolated workspace from a base
aw start my-task   # enter it (env + tmux)
aw list            # list workspaces
aw dash            # live agent state across all workspaces
aw serve           # phone remote control over the LAN (docs/serve.md)
aw delete my-task
```

## Dependencies

- **git** — repo cloning
- **tmux** (optional) — workspace sessions + the dashboard
- **cargo / Rust** — to build from source

## Release

A `chore: bump to vX.Y.Z` commit (Cargo.toml + Cargo.lock) followed by a `vX.Y.Z`
tag push triggers `.github/workflows/release.yml`, which builds + signs the
macOS binaries, publishes a GitHub Release, and bumps the Homebrew tap. Always
bump `Cargo.toml` **before** tagging. Details: [CONTRIBUTING.md](CONTRIBUTING.md).

## Conventions

- No `unwrap()` outside tests — use `?` + `anyhow::Context`.
- Don't guard against scenarios that can't happen; trust internal invariants.
- One choke point per concept (e.g. status icons → `dash::render::status_glyph`,
  pane queries → `dash::tmux::list_panes_with_metadata`). Don't sprinkle
  equivalents.

## Key directories & env vars

| Path / Var | Purpose |
|------------|---------|
| `~/.agent-workspaces/` (`AW_INSTALL_DIR`) | config.yaml + base workspaces |
| `~/agent-workspaces/` (`AW_WORKSPACES_DIR`) | created workspaces |
| `~/.cache/aw/panes/*.json` (`AW_STATE_DIR`) | per-pane agent state |
| `AGENT_WORKSPACE` / `AGENT_WORKSPACE_NAME` | current workspace dir / name |
| `AW_CONFIG_FILE` | config file path |

## Phone remote control

- **src/serve/** — `aw serve`, a LAN daemon + mobile PWA for driving `aw`
  sessions from a phone. The client is TypeScript (`src/serve/assets/app.ts`);
  its compiled `app.js` is committed and embedded via `include_str!`, so
  `cargo build` needs no Node toolchain — rebuild it with
  `scripts/build-frontend.sh` after editing `app.ts`. Usage in
  [docs/serve.md](docs/serve.md); design + roadmap in
  [docs/remote-sessions.md](docs/remote-sessions.md).
- **src/install/service.rs** — `aw install service`, a launchd LaunchAgent
  that runs `aw serve` at login (folded into `aw install all`). `aw self
  update` calls `service::refresh_after_upgrade()` to bounce the daemon onto
  the new binary. macOS-only; plist rendering is pure + unit-tested, and
  `AW_SERVICE_SKIP_LAUNCHCTL=1` writes the plist without touching launchd.
