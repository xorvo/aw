# aw-remote (prototype)

Control your `aw` agent sessions from a phone on the same Wi-Fi. Zero
dependencies — just Node (stdlib only) wrapping `aw dash json` + tmux.

See [`docs/remote-sessions.md`](../../docs/remote-sessions.md) for the full
design and roadmap.

## Run

```bash
node prototype/remote/aw-remote.mjs
```

It prints a URL with an embedded token:

```
  Open on your phone (same Wi-Fi):
    http://192.168.50.138:8787/?t=WcoBx6YWSGkKjRWh6_9_4w
```

Open that on your phone, then **Share → Add to Home Screen** for an app-like,
installable experience. Tap 🔔 once to allow "agent waiting" alerts.

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

## Config (env vars)

| Var               | Default                          | Purpose                       |
| ----------------- | -------------------------------- | ----------------------------- |
| `AW_REMOTE_PORT`  | `8787`                           | listen port                   |
| `AW_REMOTE_HOST`  | `0.0.0.0`                        | bind interface                |
| `AW_BIN`          | `~/.local/bin/aw` or `aw`        | path to the aw binary         |
| `AW_REMOTE_TOKEN` | generated → `~/.cache/aw/remote-token` | fixed auth token        |
| `AW_FONT`         | a Meslo/FiraCode Nerd Font in `~/Library/Fonts` | UI/terminal webfont served to the phone |

## Security (LAN prototype)

- Bearer token required on every request (`401` without it).
- `/api/keys` & `/api/screen` only accept `pane_id`s present in the *current*
  `aw dash json` — you can't target arbitrary tmux panes.
- All subprocess calls use `execFile` with arg arrays and `send-keys -l`
  (literal). No shell, no injection.

**Do not port-forward this to the internet.** For remote access, put it behind
Tailscale/WireGuard (see the design doc, §4).

## Run it as a background service (optional)

```bash
# keep it alive across logins with launchd, or quick-and-dirty:
nohup node prototype/remote/aw-remote.mjs > ~/.cache/aw/remote.log 2>&1 &
```

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
