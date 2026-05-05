//! `aw install hooks --agent pi` — drop the vendored pi extension into
//! pi's auto-discovery directory.
//!
//! Pi auto-loads extensions from `~/.pi/agent/extensions/<name>/index.ts`
//! (or single `.ts` files at the same level). TypeScript runs directly via
//! pi's bundled jiti loader — no compile step. Files are embedded at
//! compile time so the binary stays self-contained.

use anyhow::{Context, Result};

const PACKAGE_JSON: &str = include_str!("../../hooks/pi/package.json");
const INDEX_TS: &str = include_str!("../../hooks/pi/index.ts");

pub fn install() -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    let dest = home.join(".pi/agent/extensions/aw-dash");
    std::fs::create_dir_all(&dest)
        .with_context(|| format!("mkdir {}", dest.display()))?;
    std::fs::write(dest.join("package.json"), PACKAGE_JSON)?;
    std::fs::write(dest.join("index.ts"), INDEX_TS)?;

    // Best-effort cleanup of the wrong-path install from earlier versions.
    let stale = home.join(".config/pi/extensions/aw-dash");
    if stale.is_dir() {
        let _ = std::fs::remove_dir_all(&stale);
        println!("ℹ️  Removed stale extension at {}", stale.display());
    }

    println!("✅ Pi extension written to {}", dest.display());
    println!("   Auto-loaded by pi on next launch — no compile step needed.");
    Ok(())
}
