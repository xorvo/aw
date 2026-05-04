//! `aw install tmux-bindings` — append our key-binding block to `~/.tmux.conf`.

use anyhow::Result;

use crate::install::marker;

const TMUX_BLOCK: &str = "\
bind-key a display-popup -E -w 80% -h 60% \"aw dash\"
bind-key N run-shell \"aw dash next-ready\"
bind-key C-p run-shell \"aw dash park\"
bind-key o run-shell \"aw dash sidebar\"
";

pub fn install() -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    let path = home.join(".tmux.conf");
    marker::apply(&path, "tmux bindings", TMUX_BLOCK)?;
    println!("✅ Tmux bindings written to {}", path.display());
    println!("   Reload with: tmux source-file ~/.tmux.conf");
    Ok(())
}
