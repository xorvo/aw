#!/usr/bin/env bash
# Rebuild the committed src/serve/assets/app.js from app.ts.
#
# Run after editing app.ts. The generated app.js is committed so that
# `cargo build` stays self-contained (no Node toolchain at Rust build time);
# `aw serve` embeds index.html and app.js via include_str!.
set -euo pipefail
cd "$(dirname "$0")/../src/serve/assets"
npx -y -p typescript@5 tsc -p tsconfig.json
echo "built $(pwd)/app.js"
