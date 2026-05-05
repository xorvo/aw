// Pi extension that forwards agent lifecycle events to `aw hook` so they
// surface in `aw dash`. Auto-loaded from `~/.pi/agent/extensions/aw-dash/`.
//
// Pi's loader expects a default-exported factory function and runs `.ts`
// directly via jiti — no compile step.

import { spawn } from "node:child_process";

type AwEvent = "agent_start" | "agent_end" | "input";

function fire(event: AwEvent, args: string[] = []): void {
  // Detached + unref so pi never waits on us. Failures are silent —
  // a missed event is far better than a broken pi session.
  const child = spawn(
    "aw",
    ["hook", "--agent", "pi", "--event", event, ...args],
    { stdio: "ignore", detached: true, env: process.env },
  );
  child.unref();
}

// `pi` is the runtime API object handed to extensions. Typed as `any` so the
// extension compiles without the pi types installed.
type PiApi = {
  on: (event: string, handler: (event?: any, ctx?: any) => void) => void;
};

export default function (pi: PiApi): void {
  pi.on("agent_start", () => fire("agent_start"));
  pi.on("agent_end", () => fire("agent_end"));
  pi.on("input", (event?: { text?: string }) => {
    fire("input", event?.text ? ["--prompt", event.text] : []);
  });
}
