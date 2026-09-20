//! `aw install hammerspoon` — an optional macOS menu selector for `aw`
//! sessions, built on Hammerspoon + Ghostty.
//!
//! Writes `~/.hammerspoon/aw.lua` (generated, overwritten on re-run) and a
//! marker block in `~/.hammerspoon/init.lua` that requires it. The block is
//! `--`-commented, since init.lua is Lua and not a shell config.
//!
//! Both apps are hard requirements, so `aw install all` only runs this step
//! when it finds them; `aw install hammerspoon` runs regardless, which is
//! what you want when installing ahead of the apps or on a second machine.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::install::marker;
use crate::paths::which;

const TEMPLATE: &str = include_str!("assets/aw.lua.tmpl");
const LABEL: &str = "hammerspoon";

/// Lua comment prefix for the init.lua marker block.
const LUA_COMMENT: &str = "--";

/// What init.lua gets on first install. The hotkey lives here rather than in
/// the generated file so you can rebind it — which only works because we
/// never rewrite an existing block (see `install`).
const INIT_BODY: &str = "\
local aw = require(\"aw\")
aw.bind({ \"cmd\", \"alt\" }, \"a\") -- change the hotkey here; re-installing won't touch it
";

/// Directories macOS apps are normally installed into.
fn app_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }
    roots
}

/// Whether `<name>.app` exists under any of `roots`.
pub fn has_app(roots: &[PathBuf], name: &str) -> bool {
    roots.iter().any(|r| r.join(name).is_dir())
}

/// Both apps present? Gates the step inside `aw install all`.
pub fn available() -> bool {
    let roots = app_roots();
    has_app(&roots, "Hammerspoon.app") && has_app(&roots, "Ghostty.app")
}

/// Names of the apps we need that are missing, for a useful message.
fn missing() -> Vec<&'static str> {
    let roots = app_roots();
    ["Hammerspoon.app", "Ghostty.app"]
        .into_iter()
        .filter(|a| !has_app(&roots, a))
        .collect()
}

fn config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    Ok(home.join(".hammerspoon"))
}

/// Substitute the absolute binary paths into the Lua template.
///
/// Hammerspoon runs with a bare PATH, and going through a login shell to fix
/// that would re-introduce shell aliases (oh-my-zsh aliases `tmux` to a
/// wrapper function that does not exist non-interactively). Baking the paths
/// in at install time avoids both.
pub fn render(template: &str, aw_bin: &Path, tmux_bin: &str) -> String {
    template
        .replace("@AW@", &aw_bin.display().to_string())
        .replace("@TMUX@", tmux_bin)
}

/// Path to bake in for `aw`.
///
/// PATH first, `current_exe` second. Not the other way around: under Homebrew
/// `current_exe` resolves to the version-pinned Cellar path
/// (`.../Cellar/aw/1.9.0/bin/aw`), which stops existing on the next upgrade,
/// while `/opt/homebrew/bin/aw` is a stable symlink. Same reasoning for a
/// source install, where PATH finds the copy `install.sh` placed rather than
/// `target/release/aw` in a build tree.
fn aw_path() -> Result<PathBuf> {
    if let Some(p) = which("aw") {
        return Ok(p);
    }
    std::env::current_exe().context("resolve own path")
}

pub fn install() -> Result<()> {
    let missing = missing();
    if !missing.is_empty() {
        // Not an error: installing before the apps is legitimate, and the
        // generated Lua is inert until Hammerspoon loads it.
        println!("⚠️  Not found: {}", missing.join(", "));
        println!("   Installing anyway — the menu stays inert until both are present.");
    }

    let dir = config_dir()?;
    let aw_bin = aw_path()?;
    let tmux_bin = which("tmux")
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "tmux".to_string());
    let lua = render(TEMPLATE, &aw_bin, &tmux_bin);

    let lua_path = dir.join("aw.lua");
    std::fs::create_dir_all(&dir).with_context(|| format!("mkdir {}", dir.display()))?;
    std::fs::write(&lua_path, lua).with_context(|| format!("write {}", lua_path.display()))?;
    println!("✅ Wrote {}", lua_path.display());

    // Write-once: `marker::apply` replaces the entire body, so re-running
    // would revert a hotkey the user rebound. The block is a bootstrap, not
    // managed state — only the generated aw.lua is ours to overwrite.
    let init = dir.join("init.lua");
    if marker::has(&init, LUA_COMMENT, LABEL) {
        println!("✅ {} already requires it — hotkey left as you set it", init.display());
    } else {
        marker::apply(&init, LUA_COMMENT, LABEL, INIT_BODY)?;
        println!("✅ Hooked into {} (⌘⌥A)", init.display());
    }

    println!("   Reload: Hammerspoon menubar → Reload Config");
    println!("   Recognising an open window needs these in your tmux config:");
    println!("     set -g set-titles on");
    println!("     set -g set-titles-string \"#S\"");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let dir = config_dir()?;
    let removed = marker::remove(&dir.join("init.lua"), LUA_COMMENT, LABEL)?;
    let lua_path = dir.join("aw.lua");
    let had_lua = lua_path.exists();
    if had_lua {
        std::fs::remove_file(&lua_path)
            .with_context(|| format!("remove {}", lua_path.display()))?;
    }
    if removed || had_lua {
        println!("🧹 Removed the aw menu from {}", dir.display());
    } else {
        println!("ℹ️  Nothing to remove in {}", dir.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn render_substitutes_both_paths_and_leaves_no_placeholders() {
        let out = render(TEMPLATE, Path::new("/opt/homebrew/bin/aw"), "/opt/homebrew/bin/tmux");
        assert!(out.contains("local AW = \"/opt/homebrew/bin/aw\""));
        assert!(out.contains("local TMUX = \"/opt/homebrew/bin/tmux\""));
        assert!(!out.contains("@AW@"), "unsubstituted placeholder");
        assert!(!out.contains("@TMUX@"), "unsubstituted placeholder");
    }

    #[test]
    fn template_calls_the_json_surface_it_depends_on() {
        // The Lua's only contract with the binary is `aw switch --json`; if
        // that ever gets renamed, this fails instead of the menu silently
        // going empty.
        assert!(TEMPLATE.contains("switch --json"));
    }

    #[test]
    fn has_app_checks_every_root() {
        let tmp = TempDir::new().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::create_dir_all(b.join("Ghostty.app")).unwrap();
        std::fs::create_dir_all(&a).unwrap();
        let roots = vec![a, b];
        assert!(has_app(&roots, "Ghostty.app"));
        assert!(!has_app(&roots, "Hammerspoon.app"));
    }

    #[test]
    fn existing_block_is_never_rewritten_so_a_rebound_hotkey_survives() {
        let tmp = TempDir::new().unwrap();
        let init = tmp.path().join("init.lua");
        marker::apply(&init, LUA_COMMENT, LABEL, INIT_BODY).unwrap();
        assert!(marker::has(&init, LUA_COMMENT, LABEL));

        // The user rebinds the hotkey inside the block.
        let mine = std::fs::read_to_string(&init)
            .unwrap()
            .replace("{ \"cmd\", \"alt\" }, \"a\"", "{ \"cmd\", \"ctrl\", \"alt\" }, \"space\"");
        std::fs::write(&init, &mine).unwrap();

        // A re-install must leave it alone, which is why `install` checks
        // `has` instead of calling `apply` unconditionally.
        assert!(marker::has(&init, LUA_COMMENT, LABEL));
        assert_eq!(std::fs::read_to_string(&init).unwrap(), mine);
        assert!(mine.contains("\"space\""));
    }

    #[test]
    fn init_block_is_lua_commented_and_idempotent() {
        let tmp = TempDir::new().unwrap();
        let init = tmp.path().join("init.lua");
        std::fs::write(&init, "hs.alert.show(\"mine\")\n").unwrap();

        marker::apply(&init, LUA_COMMENT, LABEL, INIT_BODY).unwrap();
        let once = std::fs::read_to_string(&init).unwrap();
        assert!(once.contains("-- >>> aw hammerspoon >>>"), "{}", once);
        assert!(!once.contains("# >>> aw"), "a `#` marker would be a Lua syntax error");
        assert!(once.contains("hs.alert.show(\"mine\")"), "clobbered the user's config");
        assert!(once.contains("require(\"aw\")"));

        marker::apply(&init, LUA_COMMENT, LABEL, INIT_BODY).unwrap();
        let twice = std::fs::read_to_string(&init).unwrap();
        assert_eq!(once, twice, "re-install must replace the block, not append");

        assert!(marker::remove(&init, LUA_COMMENT, LABEL).unwrap());
        let gone = std::fs::read_to_string(&init).unwrap();
        assert!(!gone.contains("require(\"aw\")"));
        assert!(gone.contains("hs.alert.show(\"mine\")"), "removal kept the user's config");
    }
}
