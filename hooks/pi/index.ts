// Pi extension: forward agent lifecycle events to `aw hook` so they show up
// in `aw dash`. Drop-in equivalent of the Claude Code / Codex hook configs
// since pi doesn't take command-string hooks via config.toml.
//
// Events mapped (subject to pi's published event surface):
//   agent_start  → working
//   agent_end    → idle
//   input        → working   (user submitted a prompt)
//
// We spawn `aw hook` detached so it can't slow pi down; failure is silent
// (the dashboard will still see the next event).

import { spawn } from "node:child_process";

type AwEvent = "agent_start" | "agent_end" | "input";

function fire(event: AwEvent, args: string[] = []): void {
  const child = spawn(
    "aw",
    ["hook", "--agent", "pi", "--event", event, ...args],
    { stdio: "ignore", detached: true, env: process.env },
  );
  child.unref();
}

// `pi` is the global runtime injected by the host. Keep this `any` so the
// extension compiles in environments without the pi types installed —
// runtime dispatch will still work.
declare const pi: any;

if (typeof pi !== "undefined") {
  pi.on("agent_start", () => fire("agent_start"));
  pi.on("agent_end", () => fire("agent_end"));
  pi.on("input", (e: { text?: string } | undefined) => {
    fire("input", e?.text ? ["--prompt", e.text] : []);
  });
}
