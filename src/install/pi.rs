//! `aw install hooks --agent pi` — copy the vendored pi extension into
//! `~/.config/pi/extensions/aw-dash/`.
//!
//! The extension files are embedded at compile time so the binary is
//! self-contained.

use anyhow::{Context, Result};

const PACKAGE_JSON: &str = include_str!("../../hooks/pi/package.json");
const INDEX_TS: &str = include_str!("../../hooks/pi/index.ts");

pub fn install() -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    let dest = home.join(".config/pi/extensions/aw-dash");
    std::fs::create_dir_all(&dest)
        .with_context(|| format!("mkdir {}", dest.display()))?;
    std::fs::write(dest.join("package.json"), PACKAGE_JSON)?;
    std::fs::write(dest.join("index.ts"), INDEX_TS)?;
    println!("✅ Pi extension written to {}", dest.display());
    println!("   (No npm install needed — runtime deps come from pi itself.)");
    Ok(())
}
