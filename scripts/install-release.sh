#!/bin/sh
# Install a prebuilt `aw` binary from GitHub Releases.
#
#   curl -fsSL https://raw.githubusercontent.com/xorvo/aw/main/scripts/install-release.sh | sh
#
# This is the *binary* installer — no toolchain needed. To build from
# source instead, clone the repo and run ./install.sh (that one needs
# Rust).
#
# Knobs, all optional:
#   AW_VERSION    tag to install (default: the latest release)
#   AW_BIN_DIR    where the binary goes (default: ~/.local/bin)
#   AW_BASE_URL   where to fetch release assets from, for mirrors and
#                 air-gapped installs (default: GitHub Releases)
#
# Everything lives inside main(), invoked on the last line: `curl | sh`
# feeds the script to the shell as it arrives, so a download truncated
# mid-flight would otherwise execute as a partial script.

set -eu

REPO="xorvo/aw"

say() { printf '%s\n' "$*"; }
err() { printf 'error: %s\n' "$*" >&2; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || err "\`$1\` is required but not on PATH"
}

# Rust target triple for this machine, matching the names release.yml
# publishes. Anything else is a build-from-source case.
detect_triple() {
    arch=$(uname -m)
    case "$arch" in
        x86_64 | amd64) arch=x86_64 ;;
        aarch64 | arm64) arch=aarch64 ;;
        *) err "unsupported architecture '$arch' — build from source: https://github.com/$REPO" ;;
    esac
    os=$(uname -s)
    case "$os" in
        Darwin) printf '%s-apple-darwin\n' "$arch" ;;
        Linux) printf '%s-unknown-linux-gnu\n' "$arch" ;;
        *) err "unsupported OS '$os' — build from source: https://github.com/$REPO" ;;
    esac
}

# Resolve the newest published tag by following the /releases/latest
# redirect. The JSON API would need a parser and rate-limits unauthenticated
# callers to 60/hour per IP; this is one HEAD request and no dependencies.
latest_tag() {
    url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
        "https://github.com/$REPO/releases/latest") ||
        err "could not reach GitHub to resolve the latest release"
    tag=${url##*/}
    case "$tag" in
        v*) printf '%s\n' "$tag" ;;
        *) err "could not parse a version tag out of '$url'" ;;
    esac
}

# Compare the download against the .sha256 sidecar the release publishes.
# A missing sha256 tool is a warning, not a failure — the alternative is
# refusing to install on a minimal system.
verify_checksum() {
    file=$1
    expected=$2
    if command -v sha256sum >/dev/null 2>&1; then
        actual=$(sha256sum "$file" | awk '{ print $1 }')
    elif command -v shasum >/dev/null 2>&1; then
        actual=$(shasum -a 256 "$file" | awk '{ print $1 }')
    else
        say "!  no sha256sum/shasum found — skipping checksum verification"
        return 0
    fi
    [ "$actual" = "$expected" ] ||
        err "checksum mismatch for $file
  expected: $expected
  actual:   $actual
This is worth investigating rather than retrying."
}

main() {
    need curl
    need tar

    triple=$(detect_triple)
    version=${AW_VERSION:-$(latest_tag)}
    bin_dir=${AW_BIN_DIR:-$HOME/.local/bin}
    base_url=${AW_BASE_URL:-https://github.com/$REPO/releases/download/$version}

    name="aw-$version-$triple"

    say "Installing aw $version ($triple) into $bin_dir"

    tmp=$(mktemp -d)
    # shellcheck disable=SC2064  # expand $tmp now, not at trap time
    trap "rm -rf '$tmp'" EXIT INT TERM

    curl -fsSL "$base_url/$name.tar.gz" -o "$tmp/aw.tar.gz" ||
        err "no build published for $triple at $version.
Available builds are listed at https://github.com/$REPO/releases/tag/$version
— or build from source: https://github.com/$REPO"

    # The sidecar holds the bare hex digest, no filename.
    if sum=$(curl -fsSL "$base_url/$name.tar.gz.sha256" 2>/dev/null); then
        verify_checksum "$tmp/aw.tar.gz" "$sum"
        say "Checksum OK"
    else
        say "!  no published checksum for $name — skipping verification"
    fi

    tar -xzf "$tmp/aw.tar.gz" -C "$tmp"
    [ -f "$tmp/aw" ] || err "the archive did not contain an 'aw' binary"

    mkdir -p "$bin_dir"
    # Stage beside the target and rename: replacing a *running* binary in
    # place fails with ETXTBSY, and a rename within one directory is atomic,
    # so a re-run can never leave a half-written aw behind.
    cp "$tmp/aw" "$bin_dir/.aw.new"
    chmod 0755 "$bin_dir/.aw.new"
    mv -f "$bin_dir/.aw.new" "$bin_dir/aw"

    # Downloaded files carry Gatekeeper's quarantine attribute on macOS;
    # strip it so the first run isn't a scary dialog.
    if [ "$(uname -s)" = Darwin ]; then
        xattr -d com.apple.quarantine "$bin_dir/aw" 2>/dev/null || true
    fi

    say ""
    say "Installed: $("$bin_dir/aw" --version) → $bin_dir/aw"

    case ":${PATH:-}:" in
    *":$bin_dir:"*) ;;
    *)
        say ""
        say "!  $bin_dir is not on your PATH. Add this to your shell rc:"
        say "     export PATH=\"\$PATH:$bin_dir\""
        ;;
    esac

    say ""
    say "Next steps:"
    say "  • aw install all      Shell integration, agent hooks, tmux bindings, serve-at-login"
    say "  • aw edit-config      Point aw at the repos you work in"
    say "  • aw init             Materialize a base workspace"
    say "  • aw create my-task   Create a workspace"
    say ""
    say "Upgrade later with: aw self update"
}

main "$@"
