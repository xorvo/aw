# Contributing to `aw`

## Layout

```
aw/
├── Cargo.toml
├── src/
│   ├── main.rs            # clap dispatch
│   ├── cli.rs             # subcommand surface
│   ├── workspace/         # init / create / list / start / delete / sync / edit-*
│   ├── shell/             # shell-init, completions, _detect-workspace
│   ├── install/           # `aw install ...` (shell rc, hooks, tmux bindings)
│   ├── dash/              # popup TUI, sidebar, hook state, tmux merge
│   ├── hook.rs            # `aw hook` (agent state writer)
│   ├── self_update.rs     # `aw self update|check`
│   ├── config.rs          # config.yaml parser
│   ├── paths.rs           # AW_INSTALL_DIR / AW_WORKSPACES_DIR / etc.
│   └── git.rs             # thin shell-out wrappers
├── hooks/pi/              # vendored pi extension (embedded into the binary)
├── tests/
│   ├── common/            # TestEnv sandbox, fixture builders, snapshot normalization
│   ├── <feature>.rs       # one integration test file per subcommand/feature
│   └── snapshots/         # insta golden files, committed
├── docs/
│   ├── dash.md            # dashboard reference (state schema, keys, hook contract)
│   └── *.md               # resurrect, serve, shell integration, …
├── .github/workflows/
│   ├── ci.yml             # CI: cargo test on every push / PR
│   └── release.yml        # CI: build + publish macOS/Linux tarballs on `v*` tag
├── scripts/
│   └── install-release.sh # the `curl | sh` binary installer (end users)
├── install.sh             # cargo build + place binary + bootstrap config
└── CONTRIBUTING.md        # you are here
```

## Local development

```bash
# Build + run tests:
cargo test --tests

# Run a single test file:
cargo test --test create

# Update a snapshot intentionally:
INSTA_UPDATE=always cargo test --tests
# Then review the diff:
cargo insta review
```

The test harness (`tests/common/`) gives every test a sandboxed `$HOME`,
state directory, and tmux socket dir. Tests that need a real tmux server
spawn one via `tmux -S /tmp/awts-<pid>-<rand>.sock` and kill it on Drop.

## The release process

`aw` ships pre-built macOS and Linux binaries via GitHub Releases, and
`aw self update` pulls them. `scripts/install-release.sh` is the
`curl | sh` installer end users run for the first install; it downloads
from the same release assets and verifies the published `.sha256`.

Its asset-name format, the release matrix, `[package.metadata.binstall]`
in `Cargo.toml`, and `src/self_update.rs::target_triple` all encode the
same naming convention — change one and you must change the rest. The release pipeline is fully automatic — the only
manual steps are bumping the version, tagging, and pushing the tag.

### Cutting a release

```bash
# 1. Land all the changes you want in the release on `main`.

# 2. Bump the version in Cargo.toml. SemVer:
#       MAJOR — breaking change to a CLI surface, on-disk schema, or
#               state-file format.
#       MINOR — new subcommand, new flag, new feature. Backwards-compatible.
#       PATCH — bug fix only.
$EDITOR Cargo.toml

# 3. Refresh Cargo.lock (just the version string, fast):
cargo build --release

# 4. Commit + tag.
git add Cargo.toml Cargo.lock
git commit -m "chore: bump to v$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)"
TAG=v$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
git tag -a "$TAG" -m "$TAG"

# 5. Push commit + tag together.
git push origin main "$TAG"
```

The push of the tag triggers `.github/workflows/release.yml`. You can
follow it at `https://github.com/xorvo/aw/actions`. It typically takes
3–5 minutes.

When the workflow finishes you'll have:

  - A new GitHub Release at `https://github.com/xorvo/aw/releases/tag/<TAG>`
    with auto-generated release notes (commits since the previous tag).
  - Four assets attached:
      - `aw-<TAG>-aarch64-apple-darwin.tar.gz`
      - `aw-<TAG>-x86_64-apple-darwin.tar.gz`
      - `aw-<TAG>-aarch64-unknown-linux-gnu.tar.gz`
      - `aw-<TAG>-x86_64-unknown-linux-gnu.tar.gz`
    each with a sibling `.sha256` file.

After that, on any installed machine:

```bash
aw self check     # confirms the new version is visible
aw self update    # downloads + swaps the binary in place
```

### Important rules

- **Always bump `Cargo.toml` BEFORE tagging.** The release workflow
  builds from `cargo build --release --locked` against the tagged
  commit. The binary's runtime `--version` comes from
  `env!("CARGO_PKG_VERSION")` at compile time, so a tag of `v1.0.1`
  built from a commit that still says `version = "1.0.0"` will produce
  a binary that lies about its version — and `aw self check` will keep
  looping "newer version available."

- **Never re-tag an existing version.** If `release.yml` failed
  mid-flight (rare), delete the tag (`git push origin :refs/tags/vX.Y.Z`),
  fix forward with another commit, bump the patch, and tag the new
  commit. Don't force-push the tag.

- **Pre-releases.** Tags containing a hyphen (e.g. `v1.1.0-rc1`) are
  marked as pre-releases by the workflow. They still publish assets,
  but `aw self update` ignores them — it queries
  `/releases/latest`, which excludes pre-releases. To install a
  pre-release, download the asset by hand.

- **Pulled the wrong version?** Releases can be deleted from the
  GitHub UI without removing the tag. If you do, make sure to also
  delete or re-publish the tag — `aw self check` reads from
  `/releases/latest` which falls back to the next-newest published
  release.

### macOS code signing

The workflow runs `codesign --sign - --force <bin>` (ad-hoc signing).
This is enough to keep Gatekeeper from outright rejecting the binary,
but it does *not* notarize. Downloaded copies still get the
`com.apple.quarantine` extended attribute on first install, which `aw
self update` strips post-swap.

If you ever get an Apple Developer ID and want to notarize:

  1. Add `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_PASSWORD` (app-specific)
     and `APPLE_CERTIFICATE` (base64 .p12) as repo secrets.
  2. Replace the `codesign --sign -` step with a notarization
     workflow — the `apple-actions/import-codesign-certs` and `gon`
     actions are common picks.

Not worth doing until users actually report the install friction.

### Homebrew tap (auto-updated)

`aw` is published to the personal tap at
[`xorvo/homebrew-tap`](https://github.com/xorvo/homebrew-tap). Users
install with:

```bash
brew tap xorvo/tap
brew install aw
```

The formula at `xorvo/homebrew-tap/Formula/aw.rb` is **regenerated on
every release** by the `update-homebrew-tap` job in `release.yml`. Source
of truth lives in this repo at `scripts/homebrew-formula.rb.tmpl` —
edit there, not in the tap.

#### One-time setup

The tap-update job needs a token with write access to the tap repo.
Create it once:

  1. Generate a fine-grained PAT at
     <https://github.com/settings/personal-access-tokens/new>:
       - Resource owner: `xorvo`
       - Repository access: `xorvo/homebrew-tap` only
       - Permissions: Contents → Read and write
       - Expiration: as long as you'll trust the box
  2. Add it to this repo's secrets:
     ```bash
     gh secret set HOMEBREW_TAP_TOKEN --repo xorvo/aw
     ```
     and paste the token. Or use the web UI at Settings → Secrets and
     variables → Actions → New repository secret.

If the secret is missing on a release, the `update-homebrew-tap` job
fails (clearly) and the build job still succeeds — users can still get
the binary via `aw self update` or by downloading the tarball
directly. Set the secret, then tag a patch release to rerun.

#### Manual formula update (escape hatch)

If you need to publish a hotfix without going through the workflow:

```bash
git clone git@github.com:xorvo/homebrew-tap.git /tmp/tap
TAG=v1.2.3
SHA_ARM=$(curl -fsSL "https://github.com/xorvo/aw/releases/download/${TAG}/aw-${TAG}-aarch64-apple-darwin.tar.gz.sha256")
SHA_X86=$(curl -fsSL "https://github.com/xorvo/aw/releases/download/${TAG}/aw-${TAG}-x86_64-apple-darwin.tar.gz.sha256")
sed \
  -e "s|@VERSION@|${TAG#v}|g" \
  -e "s|@SHA_ARM64@|${SHA_ARM}|g" \
  -e "s|@SHA_X86_64@|${SHA_X86}|g" \
  scripts/homebrew-formula.rb.tmpl \
  > /tmp/tap/Formula/aw.rb
( cd /tmp/tap && git add Formula/aw.rb && git commit -m "Formula/aw.rb: bump to ${TAG#v}" && git push )
```

### Adding a new build target

macOS and Linux (arm64 + x86_64) are built today. To add another —
Windows, a musl Linux build, a BSD:

  1. Add a `{target, runner}` pair to `release.yml`'s `matrix.include`.
  2. Add the triple to the allow-list in `src/self_update.rs::target_triple`.
     The two lists must agree: a triple only in the allow-list turns
     `aw self update` into a 404 from GitHub.
  3. Cut a new release; the new asset is published alongside the rest.

The Linux jobs deliberately run on `ubuntu-22.04` rather than the latest
image: the binary links glibc, so building against the oldest image
GitHub still offers is what makes it run on older distros too.

### CI overview

Two workflows live in `.github/workflows/`:

  - **`ci.yml`** — runs on every push and PR. Builds and runs the full
    test suite. Should stay green on `main`.
  - **`release.yml`** — runs only on `v*` tag pushes. Builds artifacts,
    ad-hoc signs, packs tarballs, uploads to a new release. Needs
    `contents: write` permission on the workflow's GITHUB_TOKEN; this
    is granted in the workflow file itself, but if a repo-level setting
    overrides it, flip Settings → Actions → General → Workflow
    permissions to "Read and write."

## Style and patterns

- Don't `unwrap()` outside tests. Use `?` and `anyhow::Context`.
- Don't add error handling, fallbacks, or validation for scenarios that
  can't happen. Trust internal invariants.
- One central choke point per concept: status icons go through
  `dash::render::status_glyph`, tmux pane queries through
  `dash::tmux::list_panes_with_metadata`, marker-block edits through
  `install::marker`, and so on. Don't sprinkle equivalents.
