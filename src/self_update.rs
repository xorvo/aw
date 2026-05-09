//! `aw self check` / `aw self update` — pulls release tarballs from GitHub.
//!
//! Asset naming convention (matched by the release workflow):
//!
//!   aw-v<version>-<target-triple>.tar.gz
//!
//! containing a single `aw` executable at the archive root. The
//! `self_update` crate handles download + extraction + atomic swap with
//! the running binary (Unix unlinks the old inode; replacement just works).
//!
//! macOS specifics: downloaded binaries land with the `com.apple.quarantine`
//! extended attribute set, which makes Gatekeeper prompt the user on next
//! exec. We strip that attribute right after the swap so the upgrade is
//! seamless.

use anyhow::{Context, Result};

const REPO_OWNER: &str = "xorvo";
const REPO_NAME: &str = "aw";
const BIN_NAME: &str = "aw";

pub fn check() -> Result<()> {
    let target = target_triple()?;
    let updater = self_update::backends::github::Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .target(target)
        .current_version(env!("CARGO_PKG_VERSION"))
        .build()
        .context("configuring updater")?;

    let latest = updater.get_latest_release().context(
        "could not reach GitHub Releases (no network, or no releases published yet)",
    )?;

    let current = env!("CARGO_PKG_VERSION");
    println!("current: v{}", current);
    println!("latest:  v{}", latest.version);
    if latest.version != current {
        println!();
        println!("→ run `aw self update` to upgrade.");
    } else {
        println!();
        println!("✅ up to date.");
    }
    Ok(())
}

pub fn update(no_confirm: bool) -> Result<()> {
    let target = target_triple()?;
    let exe = std::env::current_exe()
        .context("could not resolve current binary path")?;

    // Workaround for `self_replace` 1.5 when the binary is reached via a
    // symlink (the common case for Homebrew-installed `aw`). That crate
    // does:
    //
    //   let mut exe = env::current_exe()?;       // /opt/homebrew/bin/aw
    //   if symlink {
    //       exe = fs::read_link(exe)?;           // ../Cellar/aw/X.Y.Z/bin/aw
    //   }
    //   exe.metadata()?                          // resolves relative-to-CWD,
    //                                            // not relative-to-symlink-parent
    //
    // `fs::read_link` returns the target *as written*, which Homebrew makes
    // relative. The subsequent `metadata()` then resolves against the
    // user's CWD — `aw self update` from any directory other than
    // `/opt/homebrew/bin/` dies with ENOENT.
    //
    // chdir into the symlink's parent before invoking the updater so the
    // relative target resolves correctly. No-op when the binary isn't a
    // symlink.
    if std::fs::symlink_metadata(&exe)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        if let Some(parent) = exe.parent() {
            std::env::set_current_dir(parent)
                .with_context(|| format!("could not chdir into {}", parent.display()))?;
        }
    }

    println!("Updating {} (target {})...", exe.display(), target);
    let status = self_update::backends::github::Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .target(target)
        .show_download_progress(true)
        .current_version(env!("CARGO_PKG_VERSION"))
        .no_confirm(no_confirm)
        .build()
        .context("configuring updater")?
        .update()
        .context("upgrade failed")?;

    if status.updated() {
        // Strip macOS Gatekeeper quarantine — silent if xattr is missing,
        // not present (common on Linux), or the file already lacks it.
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("xattr")
                .args(["-d", "com.apple.quarantine"])
                .arg(&exe)
                .stderr(std::process::Stdio::null())
                .status();
        }
        println!("✅ upgraded to v{}", status.version());
    } else {
        println!("✅ already on the latest version (v{})", status.version());
    }
    Ok(())
}

/// Resolve the Rust target triple at runtime so the updater asks GitHub
/// for the right asset. We deliberately match a fixed allow-list rather
/// than `std::env::consts::ARCH` blindly: the release workflow only
/// publishes for the platforms below, and a typo'd triple will surface
/// here rather than as a 404 from GitHub.
fn target_triple() -> Result<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok("aarch64-apple-darwin")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Ok("x86_64-apple-darwin")
    } else {
        Err(anyhow::anyhow!(
            "self-update is only available for macOS today (target: arch={}, os={})",
            std::env::consts::ARCH,
            std::env::consts::OS
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_triple_matches_one_of_the_release_targets() {
        // Compile-time test: we always resolve to a target the workflow
        // builds for, or fail with a clear message. On the test host this
        // means macOS arm64 / x86_64.
        let t = target_triple();
        if cfg!(target_os = "macos") {
            let s = t.unwrap();
            assert!(s.ends_with("-apple-darwin"), "got: {}", s);
        } else {
            assert!(t.is_err());
        }
    }
}
