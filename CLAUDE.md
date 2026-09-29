# CLAUDE.md

Guidance for Claude Code (claude.ai/code) working in this repository.

## Project Overview

Agent Workspace (`aw`) is a **Rust CLI** that manages isolated repo checkouts
for AI coding agents (Claude Code, Codex, pi), plus a tmux-based dashboard that
shows every agent's live state. Multiple agents work in parallel without
colliding, and `aw dash` surfaces who's working / waiting / idle.

## Architecture

The CLI is a Rust binary (`src/`). Key modules:

- **src/main.rs / cli.rs** — clap dispatch + subcommand surface
- **src/workspace/** — `init` / `create` / `list` / `start` / `delete` / `sync` / `reset` / `edit-*`
- **src/dash/** — the popup TUI, sidebar, hook state (`~/.cache/aw/panes/*.json`), tmux merge
- **src/dash/tui/switch.rs** — `aw switch`, the pane-centric quick switcher
- **src/shell/** — `shell-init`, completions, workspace detection
- **src/install/** — `aw install …` (shell rc, agent hooks, tmux bindings)
- **src/install/hammerspoon.rs** — optional macOS menu selector (docs/hammerspoon.md)
- **src/install/service.rs** — `aw install service` (launchd on macOS,
  systemd user unit on Linux)
- **src/hook.rs** — `aw hook` (agent state writer, called from agent hooks)
- **src/config.rs** — `config.yaml` parser (serde_yaml; no `yq` at runtime)
- **src/paths.rs**, **src/git.rs**, **src/self_update.rs**

Full layout and conventions live in [CONTRIBUTING.md](CONTRIBUTING.md);
dashboard state schema + hook contract in [docs/dash.md](docs/dash.md).

## Build & test

```bash
cargo build                       # debug build
cargo build --release             # optimized (what releases ship)
cargo test --tests                # full suite
cargo test --test create          # a single test file
INSTA_UPDATE=always cargo test --tests && cargo insta review  # update snapshots
./install.sh                      # build + place binary + bootstrap config
```

Tests sandbox `$HOME`, the state dir, and the tmux socket per test
(`tests/common/`). Some spawn a real tmux/zsh, so those tools must be installed
locally (CI installs `tmux`, `zsh`).

## Common runtime commands

```bash
aw init            # materialize the 'default' base workspace
aw create my-task  # isolated workspace from a base
aw start my-task   # enter it (env + tmux)
aw list            # list workspaces
aw dash            # live agent state across all workspaces
aw switch          # quick-jump between agents active in the last 24h
aw serve           # phone remote control over the LAN (docs/serve.md)
aw resurrect       # rebuild sessions after a tmux-server death (docs/resurrect.md)
aw snapshot        # save the live session layout before a planned shutdown
aw delete my-task
```

## Dependencies

- **git** — repo cloning
- **tmux** (optional) — workspace sessions + the dashboard
- **cargo / Rust** — to build from source

Supported platforms: macOS and Linux. Keep new code portable — reach for
`cfg!(target_os = ...)` only where the OS genuinely differs (init system,
font/app locations), and give the other platform a working path rather than
a bail.

## Release

A `chore: bump to vX.Y.Z` commit (Cargo.toml + Cargo.lock) followed by a `vX.Y.Z`
tag push triggers `.github/workflows/release.yml`, which builds macOS (signed)
and Linux binaries for arm64 + x86_64, publishes a GitHub Release, and bumps the
Homebrew tap (macOS only). The release matrix and
`src/self_update.rs::target_triple` must list the same triples. Always bump
`Cargo.toml` **before** tagging. Details: [CONTRIBUTING.md](CONTRIBUTING.md).

## Conventions

- No `unwrap()` outside tests — use `?` + `anyhow::Context`.
- Don't guard against scenarios that can't happen; trust internal invariants.
- One choke point per concept (e.g. status icons → `dash::render::shown_glyph`,
  pane queries → `dash::tmux::list_panes_with_metadata`). Don't sprinkle
  equivalents.
- **Deleting a user's pane state is the one irreversible thing `aw` does.** It
  goes through `dash::state::should_drop` and nothing else. A process may only
  collect state stamped with the pid of the tmux server it is itself talking to;
  state owned by another *live* server, or with no owner recorded, is never
  touched. Absence from one `list-panes` reply is a suspicion, not a verdict.
  Every decision is logged to `<state>/gc.log`. This is not defensive
  over-engineering: a test harness that sandboxed tmux but not `AW_STATE_DIR`
  once deleted a developer's live agents' state, and the symptom (agents that
  look like they have never run) is invisible until someone notices.
- **Tests must never touch the real `~/.cache/aw`.** Use `TestEnv`, and when
  starting a tmux server pass `common::sandbox_env` — a tmux server hands the
  environment it was started with to every pane it creates later, so a server
  started without `AW_STATE_DIR` will run `aw` against the developer's real
  cache. `tests/dash.rs` has guards that fail if collection ever stops
  respecting ownership.

## Key directories & env vars

| Path / Var | Purpose |
|------------|---------|
| `~/.agent-workspaces/` (`AW_INSTALL_DIR`) | config.yaml + base workspaces |
| `~/agent-workspaces/` (`AW_WORKSPACES_DIR`) | created workspaces |
| `~/.cache/aw/panes/*.json` (`AW_STATE_DIR`) | per-pane agent state |
| `~/.cache/aw/sessions.json` | durable session manifest for `aw resurrect` (docs/resurrect.md) |
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
- **src/install/service.rs** — `aw install service`, which runs `aw serve` at
  login (folded into `aw install all`): a launchd LaunchAgent on macOS, a
  systemd **user** unit on Linux. `aw self update` calls
  `service::refresh_after_upgrade()` to bounce the daemon onto the new binary.
  Both unit renderers are pure + unit-tested, and
  `AW_SERVICE_SKIP_ACTIVATION=1` writes the unit file without touching
  launchd/systemd.
