//! `aw edit-config`, `aw edit-base`, `aw open-home` — invoke an editor on
//! the relevant directory.
//!
//! Editor selection mirrors the bash CLI: prefer `$EDITOR` for `open-home`;
//! for `edit-config` and `edit-base`, walk a hard-coded preference list
//! (cursor → code → nvim → vim → nano), and fall back to opening the
//! containing directory in a file manager (macOS `open`, Linux `xdg-open`).

use std::path::Path;
use std::process::Command;

use anyhow::Result;

use crate::paths::Paths;

pub fn edit_config() -> Result<()> {
    let paths = Paths::from_env()?;
    let editor = preferred_text_editor().unwrap_or_else(|| {
        eprintln!("❌ No suitable editor found");
        std::process::exit(1)
    });
    println!("📝 Opening config with {}...", editor);
    let _ = Command::new(&editor).arg(&paths.config_file).status();
    Ok(())
}

pub fn edit_base(base_name: &str) -> Result<()> {
    let paths = Paths::from_env()?;
    let base_dir = paths.base_dir(base_name);
    if !base_dir.is_dir() {
        eprintln!("❌ Base workspace '{}' not found", base_name);
        eprintln!("Available bases:");
        if let Ok(read) = std::fs::read_dir(&paths.bases_root()) {
            let mut names: Vec<String> = read
                .filter_map(|d| d.ok())
                .filter(|d| d.path().is_dir())
                .map(|d| d.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            for n in names {
                eprintln!("  • {}", n);
            }
        }
        std::process::exit(1);
    }
    open_in_editor_or_filemanager(&base_dir, "base workspace");
    Ok(())
}

pub fn open_home() -> Result<()> {
    let paths = Paths::from_env()?;
    if let Some(editor) = std::env::var_os("EDITOR")
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string_lossy().into_owned())
    {
        println!("📝 Opening installation directory with {}...", editor);
        let _ = Command::new(&editor).arg(&paths.install_dir).status();
        return Ok(());
    }
    open_in_editor_or_filemanager(&paths.install_dir, "installation directory");
    Ok(())
}

fn preferred_text_editor() -> Option<String> {
    for cmd in ["cursor", "code", "nvim", "vim", "nano"] {
        if which(cmd) {
            return Some(cmd.to_string());
        }
    }
    None
}

fn open_in_editor_or_filemanager(path: &Path, label: &str) {
    if which("cursor") {
        println!("📝 Opening {} with Cursor...", label);
        let _ = Command::new("cursor").arg(path).status();
        return;
    }
    if which("code") {
        println!("📝 Opening {} with VS Code...", label);
        let _ = Command::new("code").arg(path).status();
        return;
    }
    if cfg!(target_os = "macos") {
        println!("📂 Opening {} in file manager...", label);
        let _ = Command::new("open").arg(path).status();
        return;
    }
    if std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if which("xdg-open") {
            println!("📂 Opening {} in file manager...", label);
            let _ = Command::new("xdg-open").arg(path).status();
            return;
        }
    }
    println!("📂 {} location: {}", capitalize(label), path.display());
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn which(cmd: &str) -> bool {
    let path = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return false,
    };
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(cmd);
        if candidate.is_file() {
            // Check executability via metadata mode bits.
            if let Ok(meta) = candidate.metadata() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if meta.permissions().mode() & 0o111 != 0 {
                        return true;
                    }
                }
                #[cfg(not(unix))]
                {
                    let _ = meta;
                    return true;
                }
            }
        }
    }
    false
}
