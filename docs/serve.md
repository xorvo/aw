# `aw serve` — phone remote control

Control your `aw` agent sessions from a phone on the same Wi-Fi. The daemon
is built into the `aw` binary; the mobile client is a PWA served from it
(TypeScript source in `src/serve/assets/`, compiled `app.js` committed and
embedded at build time).

See [`remote-sessions.md`](remote-sessions.md) for the full design and
roadmap.

## Run

```bash
aw serve
```

It prints a URL with an embedded token — and a QR code, so you don't have to
type it:

```
  Open on your phone (same Wi-Fi):
    http://192.168.50.138:7340/?t=WcoBx6YWSGkKjRWh6_9_4w
```

You can also press **`Q`** inside the `aw dash` popup at any time to re-show
the QR code and URL (the dashboard and daemon share the token at
`~/.cache/aw/remote-token`, so whichever runs first generates it).

Open it on your phone, then **Share → Add to Home Screen** for an app-like,
installable experience. Tap the bell button once to allow "agent waiting"
alerts.

## What you get

- **Live session list** — every agent, color-coded: green working, amber
  *needs you* (sorted to the top), grey idle. Streamed over SSE.
- **Tap a session** → its live terminal, in full ANSI color, pushed over SSE
  only when the screen changes (no polling). Rendered in a Nerd Font so
  powerline/status-line glyphs show correctly.
- **Type two ways** — tap the terminal to type straight into the pane (live),
  or open the full-screen **draft editor** (pencil icon) to compose locally:
  IME-friendly, per-session drafts saved in `localStorage`, sent as one
  bracketed paste (markdown/multi-line safe).
- **Quick keys** — one-tap `1`/`2`/`3` (permission menus), arrows, `⌫`, `⏎`,
  `esc`, `^C`.
- **Fit to screen** — opt-in; resizes the tmux window to your phone so Claude's
  TUI reflows. Only the open session, auto-restored when you leave or close.
  (Shares the window with your Mac — see Security/notes.)
- **Send a screenshot** — the camera button uploads an image and pastes its
  saved path into the session for Claude to read.
- **Alerts** — while the (installed) PWA is open/backgrounded, you get a
  notification the moment a session starts waiting for you.
- **Native feel** — installable (icon + manifest), back gesture / refresh wired
  to browser history, viewport tracks the keyboard, no focus-zoom.

## Config (flags + env vars)

| Flag / Var | Default | Purpose |
| ---------- | ------- | ------- |
| `--port` / `AW_REMOTE_PORT`  | `7340`  | listen port |
| `--host` / `AW_REMOTE_HOST`  | `0.0.0.0` | bind interface |

## Running a second, isolated instance

Useful for working on the web client without touching the instance you rely
on. Nothing special is needed — point the three env vars somewhere else and
pick another port:

```bash
AW_STATE_DIR=/tmp/aw-dev-state \
AW_REMOTE_TOKEN=devtoken \
  aw serve --port 7341
```

The two processes then share nothing but tmux itself, which they only read:

| | your instance | the scratch one |
|---|---|---|
| port | 7340 | 7341 |
| pane state + manifest | `~/.cache/aw` | `/tmp/aw-dev-state` |
| token | `~/.cache/aw/remote-token` | `$AW_REMOTE_TOKEN` |

A fresh `AW_STATE_DIR` starts with no pane state, so the list still shows every
live session (names come from tmux) but without hook-derived status or prompts.
Copy them in if you want realistic rendering — they are copies, so the scratch
instance's own garbage collection can never reach your originals:

```bash
mkdir -p /tmp/aw-dev-state/panes
cp ~/.cache/aw/panes/*.json /tmp/aw-dev-state/panes/
```

One caveat: the fit button and the key/paste endpoints act on **real** tmux
panes whichever instance serves them, because tmux is the shared thing. A
scratch instance is isolated for reading, not for writing.
| `AW_REMOTE_TOKEN` | generated → `~/.cache/aw/remote-token` | fixed auth token |
| `AW_FONT` | a Meslo/FiraCode Nerd Font found in the system font dirs | UI/terminal webfont served to the phone |

## Security (LAN)

- Bearer token required on every request (`401` without it) — header, `?t=`
  bootstrap, or cookie.
- `/api/keys` & `/api/screen*` only accept `pane_id`s present in the *current*
  dash snapshot — you can't target arbitrary tmux panes.
- Named keys go through an allowlist; literal text is sent with
  `send-keys -l`. All tmux calls are exec-with-arg-arrays. No shell, no
  injection.

**Do not port-forward this to the internet.** For remote access, put it behind
Tailscale/WireGuard (see the design doc, §4).

## Run at login (system service)

`aw install all` sets this up for you. To manage it directly:

```bash
aw install service              # run aw serve at login, keep it alive
aw install service --uninstall  # stop and remove it
aw install service --port 9000  # custom host/port (re-run to change)
```

One command, one backend per platform:

| | macOS | Linux |
| --- | --- | --- |
| Unit file | `~/Library/LaunchAgents/com.agent-workspaces.serve.plist` | `~/.config/systemd/user/aw-serve.service` |
| Loaded by | `launchctl bootstrap gui/<uid>` | `systemctl --user enable --now` |
| Restart on crash | `KeepAlive` | `Restart=always` |
| Inspect | `launchctl print gui/<uid>/com.agent-workspaces.serve` | `systemctl --user status aw-serve` |

Either way the daemon starts on login, restarts if it crashes, and logs to
`~/.cache/aw/serve.log`. The unit bakes in a PATH (Homebrew/Linuxbrew
prefixes, your binary's own dir, the system dirs) so `tmux` is found from
the minimal environment launchd and systemd hand their jobs.

`aw self update` automatically restarts the service onto the new binary, so
upgrades take effect without a manual reload.

**macOS:** run `aw install service` from a normal desktop session — loading
into the GUI launchd domain needs one.

**Linux:** a systemd user manager is normally torn down when your last
session ends, taking `aw serve` with it. To keep the phone remote reachable
while you're logged out:

```bash
sudo loginctl enable-linger "$USER"
```

If you'd rather not run a service at all, quick-and-dirty still works:
`nohup aw serve > ~/.cache/aw/serve.log 2>&1 &`.

## API

| Method | Path                  | Notes                                            |
| ------ | --------------------- | ------------------------------------------------ |
| GET    | `/api/state`          | sorted session list + `ageSec`                   |
| GET    | `/api/events`         | SSE, pushes state on change                      |
| GET    | `/api/screen`         | `?pane=%1&lines=80` — one-shot capture           |
| GET    | `/api/screen-stream`  | `?pane=%1&lines=80` — SSE, pushes on change      |
| POST   | `/api/keys`           | `{pane, text?, key?, submit?, paste?}`           |
| POST   | `/api/resize`         | `{pane, cols, rows}` — fit window to phone       |
| POST   | `/api/unfit`          | `{pane}` — restore original window size          |
| POST   | `/api/upload`         | image body → saved to `~/.cache/aw/uploads/`     |

## Rebuilding the frontend

`src/serve/assets/app.js` is generated from `app.ts` and committed, so plain
`cargo build` needs no Node toolchain. After editing `app.ts` (or
`index.html`'s script expectations), run:

```bash
scripts/build-frontend.sh   # npx tsc, writes app.js
cargo build                 # re-embeds the assets
```
