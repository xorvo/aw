#!/usr/bin/env bash
# Install the `aw` Rust binary and bootstrap the configuration directory.
#
# This used to be an 800-line bash script that embedded the entire CLI as a
# heredoc. The CLI is now a Rust binary at the repo root; this file only
# builds + installs it and seeds `~/.agent-workspaces/config.yaml`.
#
# After this runs, the user can `aw install all` to wire shell integration,
# agent hooks, and tmux key bindings.

set -euo pipefail

AW_BIN_DIR="${AW_BIN_DIR:-$HOME/.local/bin}"
AW_INSTALL_DIR="${AW_INSTALL_DIR:-$HOME/.agent-workspaces}"
AW_WORKSPACES_DIR="${AW_WORKSPACES_DIR:-$HOME/agent-workspaces}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if ! command -v cargo >/dev/null 2>&1; then
    echo "❌ Rust toolchain required. Install via https://rustup.rs and rerun." >&2
    exit 1
fi

echo "🚀 Building aw (Rust)..."
(cd "$SCRIPT_DIR" && cargo build --release --quiet)

mkdir -p "$AW_BIN_DIR" "$AW_INSTALL_DIR" "$AW_WORKSPACES_DIR"
install -m 0755 "$SCRIPT_DIR/target/release/aw" "$AW_BIN_DIR/aw"

if [ ! -f "$AW_INSTALL_DIR/config.yaml" ]; then
    echo "📋 Creating config from template..."
    cp "$SCRIPT_DIR/config.example.yaml" "$AW_INSTALL_DIR/config.yaml"
fi

echo
echo "✅ Installed: $("$AW_BIN_DIR/aw" --version)"
echo
if [[ ":$PATH:" != *":$AW_BIN_DIR:"* ]]; then
    echo "⚠️  $AW_BIN_DIR is not on PATH. Add to your shell rc:"
    echo "    export PATH=\"\$PATH:$AW_BIN_DIR\""
    echo
fi

echo "Next steps:"
echo "  • aw install all      Shell integration, agent hooks, tmux bindings, serve-at-login"
echo "  • aw edit-config      Edit your repos / local files config"
echo "  • aw init             Materialize a base from your config"
echo "  • aw create my-task   Create a workspace"
echo "  • aw dash             Open the agent dashboard"
