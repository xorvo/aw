# Migrating from the bash `aw`

The `aw` CLI is now a single Rust binary. The on-disk layout is unchanged —
your `~/.agent-workspaces/`, `~/agent-workspaces/`, configs, and `hooks.d/`
files are all read by the new binary as-is.

## What changed

| Before | After |
|---|---|
| Bash CLI embedded in `install.sh` (~800 lines) | Rust binary at `target/release/aw` |
| `~/.local/bin/aw` was a bash script | `~/.local/bin/aw` is a compiled binary |
| Shell integration via `bin/aw-shell-hook.sh` | `eval "$(aw shell-init zsh)"` |
| `yq` dependency required at runtime | embedded `serde_yaml` parser |
| No agent dashboard | `aw dash` (popup TUI + sidebar + status-line) |
| No agent hooks | `aw install hooks --agent claude\|codex\|pi` |

Subcommand names match the old surface 1:1, including aliases:

- `list` / `ls`
- `start` / `enter` / `open`
- `delete` / `rm`

## What you need to do

1. **Rebuild and reinstall.**
   ```bash
   git pull
   ./install.sh           # builds the Rust binary, installs to ~/.local/bin
   ```

2. **Replace your shell hook line.**
   Your old `.zshrc` / `.bashrc` likely has:
   ```bash
   source ~/.agent-workspaces/bin/aw-shell-hook.sh
   ```
   Replace with:
   ```bash
   eval "$(aw shell-init zsh)"
   ```
   Or run `aw install shell --shell zsh` and let it append the right line.

3. **(Optional) Wire agent hooks for `aw dash`.**
   ```bash
   aw install hooks --agent all
   aw install tmux-bindings
   ```

## What you can throw away

After confirming the new CLI works, you can delete the old shell-integration
scripts:

```bash
rm -rf ~/.agent-workspaces/bin   # only if you used the bash version
```

These are no longer used. Shell integration is generated on demand by
`aw shell-init`.

## Things to manually verify

These can't be automated — run through them once:

- [ ] `cd ~/agent-workspaces/<some-workspace>` auto-exports
      `AGENT_WORKSPACE` / `AGENT_WORKSPACE_NAME`.
- [ ] Your prompt segment (powerlevel10k / oh-my-zsh / starship / native)
      shows the workspace name when inside a workspace. The shell hook now
      only sets env vars; prompt segments are framework-specific and live
      in your existing configuration.
- [ ] `Tab` completion works for `aw create <TAB>`, `aw start <TAB>`,
      `aw delete <TAB>`. If not, run `aw completions zsh > ~/.zfunc/_aw`
      (or the appropriate path for your completion loader).
- [ ] `aw open <name>` (alias of `aw start`) launches a tmux session.

## Rolling back

If something's broken, the old bash CLI is preserved as a test fixture at
`tests/fixtures/aw-bash`. You can run it directly:

```bash
~/agent-workspaces/tmux-manager/aw/tests/fixtures/aw-bash help
```

It honors the same `AW_INSTALL_DIR` / `AW_WORKSPACES_DIR` env vars, so you
can sanity-check anything against it side-by-side.
