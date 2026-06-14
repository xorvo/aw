# Remote Sessions — controlling `aw` from your phone

Watch and steer every `aw` agent session from a phone on the same network:
see which agents are working / waiting / idle, get pinged when one needs you,
read its terminal, and type back or approve permission prompts.

LAN-first, with a clear path to WAN later. **This is shipped as
[`aw serve`](serve.md)** (`src/serve/` — Rust daemon, TypeScript PWA client);
this doc is the design record and the roadmap for what's next. It began as a
standalone Node prototype (`prototype/remote/`, retired once the port landed).

---

## 1. What "control a session" actually means

`aw` already has every primitive we need; remote control is a thin transport
over them. Grounding the design in the real implementation:

| Need on the phone            | Where `aw` already provides it                                            |
| ---------------------------- | ------------------------------------------------------------------------- |
| List sessions + live status  | `aw dash json` → array of `PaneState` (`working`/`waiting`/`idle`)         |
| Know *when* one needs you    | Hooks write state on every agent event; `waiting` = "wants input"         |
| Read what the agent is doing | `tmux capture-pane -t <pane> -p` (the pane is the source of truth)         |
| Type / approve / reject      | `tmux send-keys -t <pane> -l -- <text>` and named keys (`Enter`, `1`, …)   |
| Workspace lifecycle          | `aw create`, `aw list`, `aw delete`, `aw start`                           |
| Triage helpers               | `aw dash next-ready`, `park`, `pin`                                        |

Key facts from the codebase that shape the design:

- **State model** — `src/dash/state.rs`: one JSON file per pane at
  `~/.cache/aw/panes/<pane_id>.json` (override via `$AW_STATE_DIR`), written
  atomically. `Status` is `Working | Waiting | Idle`. `aw dash json` aggregates
  them and resolves live labels from tmux.
- **State transitions** — `src/hook.rs`, wired in `~/.claude/settings.json`:
  Claude/Codex/pi call `aw hook --agent <a> --event <e>` on
  `UserPromptSubmit` / `PreToolUse` / `Notification` / `Stop`. `Notification`
  is the signal that the agent is **waiting for a human** — this is our push
  trigger.
- **Sessions are tmux** — one session `aw-<workspace>` per workspace. Every
  control action is ultimately a tmux command against a `pane_id` (`%0`, `%1`…).
- **Notifications today** are local-only (`src/dash/notify.rs` →
  `notify-rust`, macOS Notification Center). Remote push is the gap we fill.

**Design consequence:** the Mac is the single source of truth. We add one
component — a local **agent daemon** that exposes `aw dash json` + tmux over an
authenticated HTTP/SSE API. Everything else is a client.

---

## 2. Architecture

```
                 LAN (same Wi-Fi)                          Mac mini (host)
  ┌──────────────────┐   HTTPS/WSS    ┌───────────────────────────────────────┐
  │  Phone client     │ ─────────────▶│  aw serve  (daemon)                    │
  │  • PWA or iOS app  │◀──── SSE ─────│   ├─ GET  /api/state    aw dash json   │
  │  • list + detail   │   live state  │   ├─ GET  /api/screen   capture-pane   │
  │  • terminal view   │               │   ├─ POST /api/keys     send-keys      │
  │  • quick actions   │               │   ├─ GET  /api/events   SSE (state)    │
  └──────────────────┘                │   └─ POST /api/workspaces  create/…    │
                                       │                                        │
                                       │  Hooks ──▶ ~/.cache/aw/panes/*.json    │
                                       │  Agent panes ◀── tmux ──┘              │
                                       └───────────────────────────────────────┘
```

- **Daemon** (`aw serve`): the only new server-side piece. Reads state, runs
  tmux, holds the auth boundary. Stateless beyond the token.
- **Push path** (phase 2): the existing `Notification` hook also POSTs to the
  daemon, which fans out a Web Push / APNs alert so the phone buzzes **even
  when the app is closed**.
- **Clients**: start with one responsive **PWA** served by the daemon
  ("Add to Home Screen" → app-like, installable, notification-capable on iOS
  16.4+). A native iOS app is a later option for true background APNs and Live
  Activities — but it's an optimization, not a requirement.

### Why a daemon, not "SSH from a terminal app"

You *could* run `tmux attach` from Blink/Termius over SSH. That gives raw
access but a poor phone UX: no session list, no status colors, no
"agent is waiting" badge, no one-tap approve, no push. The daemon turns `aw`'s
structured state into a glanceable, tappable surface — and SSH remains the
power-user escape hatch.

---

## 3. API (the contract `aw serve` implements)

| Method | Path                  | Body / query                          | Action                                                       |
| ------ | --------------------- | ------------------------------------- | ------------------------------------------------------------ |
| GET    | `/api/state`          | —                                     | `aw dash json`, sorted waiting-first, `+ageSec`              |
| GET    | `/api/events`         | —                                     | SSE; pushes the state array whenever it changes              |
| GET    | `/api/screen`         | `?pane=%1&lines=80`                    | one-shot `tmux capture-pane -e` of that pane                 |
| GET    | `/api/screen-stream`  | `?pane=%1&lines=80`                    | SSE; tails the pane, pushes a frame only when it changes     |
| POST   | `/api/keys`           | `{pane, text?, key?, submit?, paste?}` | type text / allowlisted key / Enter / bracketed-paste a blob |
| POST   | `/api/resize`         | `{pane, cols, rows}`                   | fit the window to the phone (`window-size manual`)           |
| POST   | `/api/unfit`          | `{pane}`                              | restore the window's original `window-size`                 |
| POST   | `/api/upload`         | image body                            | save a screenshot to `~/.cache/aw/uploads/`, return its path |
| GET    | `/font.ttf`, `/icon-*.png`, `/manifest.webmanifest` | — | public assets (Nerd Font, app icon, PWA manifest) |
| POST   | `/api/workspaces`†    | `{name, base}`                        | `aw create` (phase 1)                                       |
| DELETE | `/api/workspaces/:n`† | —                                     | `aw delete` (phase 1)                                       |

† workspace lifecycle not implemented yet. Everything above it is implemented and tested.

**Beyond the table, the client also includes:** an ANSI→HTML renderer
(16/256/truecolor, strips non-SGR/OSC sequences); live tap-to-type into the
pane and a full-screen **draft editor** with per-session `localStorage` drafts
(IME-friendly, sends via bracketed paste); **fit-to-screen** that resizes the
tmux window to the phone (opt-in, auto-restored on leave/close); one-tap
**screenshot upload** that pastes the saved path into the session; a
**Nerd-Font icon set** (served from the host) with the terminal rendered in the
same font so powerline glyphs display; and browser-history integration so the
back gesture and refresh behave natively.

**Status → UI mapping:** `waiting` = amber "needs you" (sorted to top, fires a
notification), `working` = green, `idle` = grey.

**Permission prompts:** Claude renders these as a numbered menu in the pane.
The client exposes one-tap `1` / `2` / `3`, arrow keys, `⏎`, `esc`, `^C`, plus a
free-text box that types and submits — so "approve this edit" is a single tap.

---

## 4. Security (the part that matters once it leaves your laptop)

Threat model: anything that can reach the port can drive your agents — read
code, run tools, type commands. So even on LAN this is gated.

**Phase 1 — LAN (implemented):**
- **Bearer token**, 128-bit, generated once and cached at
  `~/.cache/aw/remote-token` (mode 0600). Required on every request (header,
  `?t=` bootstrap, or cookie). No token → `401`.
- **Pane allowlisting** — `/api/keys` and `/api/screen` reject any `pane_id`
  not present in the *current* `aw dash json`. You can only touch live agent
  panes, never arbitrary tmux targets.
- **No shell, ever** — all subprocess calls use `execFile` with argument
  arrays and `send-keys -l` (literal). User text can't escape into a shell.
- **Named-key allowlist** — only a fixed set (`Enter`, `Escape`, arrows,
  `1`–`4`, `C-c`, …) is accepted as a "key"; everything else must go through
  literal text.
- Bind to the LAN interface; the token in the URL never leaves your network.

**Phase 1.5 — hardening before WAN:**
- TLS (self-signed cert + trust on device, or `mkcert`) so the token and
  terminal contents aren't plaintext on the wire.
- Short-lived session tokens after a one-time pairing (QR code on the Mac →
  scan once), instead of a long-lived secret in a bookmark.
- Audit log of every `/api/keys` action.

**Phase 2 — WAN, pick one (in order of preference):**
1. **Tailscale / WireGuard** — *recommended.* The daemon stays bound to a
   private interface; your phone joins the tailnet and reaches it as if on LAN.
   No ports exposed to the internet, MagicDNS, ACLs. Near-zero code, best
   security. WAN becomes "it just works from anywhere."
2. **Cloudflare Tunnel** (`cloudflared`) — public hostname with Cloudflare
   Access (email/SSO) in front. Good if you want a URL without a VPN client.
3. **Relay + APNs** — only if you build a native app and want true background
   push: a tiny cloud relay holds the WebSocket and forwards APNs. Most work;
   defer until the native app exists.

Never just port-forward the daemon's port (default 7340) to the internet.

---

## 5. Notifications (so being AFK actually works)

The whole point is to walk away and get pulled back when needed.

- **Trigger:** the `Notification` hook event (agent waiting) — already fires
  `aw hook`. We extend the hook (or the daemon polling state) to also enqueue a
  push.
- **Phase 1:** the PWA, while open/backgrounded, raises a local notification
  the moment a session flips to `waiting` (Web Notifications API; works on an
  installed iOS PWA, 16.4+). Good enough to validate the loop.
- **Phase 2:** real background push — **Web Push (VAPID)** for the PWA, or
  **APNs** for a native app — delivered even when the app is fully closed.
  Pragmatic shortcut worth shipping early: POST to **[ntfy.sh](https://ntfy.sh)**
  (or a self-hosted ntfy) from the hook; subscribe in the ntfy app. Background
  push from your Mac to your phone in ~10 lines, no app to build.

---

## 6. Recommendation & roadmap

**Go PWA-first, served by an `aw serve` daemon, reached over Tailscale for WAN.**
Rationale: one codebase, installable on the home screen, no App Store loop, and
the daemon is the reusable core whether the client is later a native app or not.
A native iOS app earns its keep only for background APNs / Live Activities /
widgets — revisit after the PWA proves the workflow.

| Phase | Deliverable                                                            | Status |
| ----- | --------------------------------------------------------------------- | ------ |
| **0** | Node prototype daemon + PWA: live SSE state & screen, ANSI color, tap-to-type, draft editor, fit-to-screen, screenshot upload, Nerd-Font UI, token auth, LAN | ✅ done — `prototype/remote/` |
| **0.5** | QR pairing in the dashboard: `Q` in `aw dash` shows the pairing URL as a terminal QR code (shared token via `~/.cache/aw/remote-token`) | ✅ done — `src/dash/remote_link.rs` |
| **1** | Foreground/installed-PWA "waiting" notifications; workspace create/delete | next |
| **1.5** | Port the daemon into `aw` as `aw serve` (Rust daemon, TS client, embedded assets) | ✅ done — `src/serve/` |
| **1.6** | TLS (self-signed / mkcert) so the token never crosses the LAN in clear | |
| **2** | Web Push (VAPID) or ntfy background alerts; Tailscale for WAN          | |
| **3** | Optional native iOS app (APNs, Live Activities, widgets)              | later |

### Folding into `aw` (phase 1.5 — done)

`aw serve` lives at `src/serve/` (threaded `tiny_http`, no async runtime —
matching the repo's lean ethos). It reuses `dash::state` for snapshots and
`dash::remote_link` for the shared pairing token; tmux control (capture /
send-keys / fit) lives in `serve::term`. The PWA client is TypeScript at
`src/serve/assets/app.ts`; its compiled `app.js` plus `index.html` are
embedded with `include_str!` → single binary, no Node at build or run time
(`scripts/build-frontend.sh` regenerates `app.js` after editing the TS).

The HTTP contract above was the seam: the client didn't change when the
backend moved from Node to Rust.

---

## 7. Open questions (decide later, not blocking)

- Multi-host: control agents across several Macs from one phone view? (daemon
  per host + a client-side host switcher, or one aggregating daemon.)
- Scrollback depth & ANSI color in the terminal view (the client strips color
  for legibility; `capture-pane -e` preserves it).
- Rate-limiting `/api/keys` and an explicit "dangerous action" confirm for
  `^C` / destructive replies.
