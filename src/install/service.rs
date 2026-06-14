//! `aw install service` — run `aw serve` at login via a launchd
//! LaunchAgent (macOS).
//!
//! Writes `~/Library/LaunchAgents/com.agent-workspaces.serve.plist` and
//! loads it into the per-user GUI domain, so the phone-remote daemon
//! comes up on login and is kept alive (restarted on crash). Idempotent:
//! re-running rewrites the plist and reloads, which is also how an
//! upgrade refreshes the running daemon onto a new binary
//! (`refresh_after_upgrade`, called from `aw self update`).
//!
//! Why a baked PATH: launchd agents inherit a minimal environment, but
//! `aw serve` shells out to `tmux` — usually in `/opt/homebrew/bin` or
//! `/usr/local/bin`, neither on launchd's default PATH. We capture a
//! sensible PATH at install time and write it into the plist so tmux is
//! found at runtime.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// launchd label and plist basename. Reverse-DNS by convention; stable
/// across versions so self-update never has to rename anything.
const LABEL: &str = "com.agent-workspaces.serve";

/// `~/Library/LaunchAgents/<label>.plist`.
pub fn plist_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    Ok(home.join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
}

/// Whether the LaunchAgent plist is present (i.e. the service has been
/// installed). Cheap file check — used by the upgrade hook to decide
/// whether there's anything to refresh.
pub fn is_installed() -> bool {
    plist_path().map(|p| p.is_file()).unwrap_or(false)
}

/// Install (or reinstall) the login service. `host`/`port` override the
/// `aw serve` defaults (`0.0.0.0:<DEFAULT_PORT>`) when provided.
pub fn install(host: Option<&str>, port: Option<u16>) -> Result<()> {
    if !cfg!(target_os = "macos") {
        anyhow::bail!(
            "`aw install service` uses launchd and is macOS-only.\n\
             On Linux, create a systemd user unit running `aw serve`:\n  \
             ~/.config/systemd/user/aw-serve.service  (ExecStart=<aw> serve),\n  \
             then: systemctl --user enable --now aw-serve"
        );
    }

    let exe = std::env::current_exe().context("resolving the aw binary path")?;
    let log = log_path()?;
    let plist = plist_path()?;
    let body = render_plist(&exe.to_string_lossy(), host, port, &log.to_string_lossy(), &service_path());

    if let Some(dir) = plist.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&plist, &body).with_context(|| format!("writing {}", plist.display()))?;

    println!("Wrote LaunchAgent: {}", plist.display());
    if reload(&plist)? {
        let where_ = host.unwrap_or("0.0.0.0");
        let p = port.unwrap_or(crate::dash::remote_link::DEFAULT_PORT);
        println!("Loaded and started — aw serve will run at login on {where_}:{p}.");
        println!("Logs: {}", log.display());
        println!("Stop/remove with: aw install service --uninstall");
    }
    Ok(())
}

/// Remove the login service: unload it and delete the plist.
pub fn uninstall() -> Result<()> {
    let plist = plist_path()?;
    if !plist.exists() {
        println!("No aw serve service installed (nothing at {}).", plist.display());
        return Ok(());
    }
    if cfg!(target_os = "macos") {
        // Unload before deleting so launchd forgets the job immediately.
        let _ = bootout();
    }
    std::fs::remove_file(&plist).with_context(|| format!("removing {}", plist.display()))?;
    println!("Removed aw serve service ({}).", plist.display());
    Ok(())
}

/// Called after `aw self update` swaps the binary. If the service is
/// installed, rewrite the plist (paths/PATH may have shifted) and
/// kickstart it so the running daemon restarts onto the new binary. A
/// no-op — and never an error — when no service is installed; the
/// upgrade must not fail just because the daemon couldn't be bounced.
pub fn refresh_after_upgrade() {
    if !is_installed() || !cfg!(target_os = "macos") {
        return;
    }
    match install(None, None) {
        Ok(()) => println!("Restarted the aw serve login service onto the new binary."),
        Err(e) => eprintln!(
            "note: aw serve service not refreshed ({e}). \
             Restart it with: aw install service"
        ),
    }
}

/// `~/.cache/aw/serve.log` (or `$AW_STATE_DIR/serve.log`).
fn log_path() -> Result<PathBuf> {
    Ok(crate::dash::state_root()?.join("serve.log"))
}

/// PATH to bake into the plist: the aw binary's own dir and the common
/// Homebrew prefixes first (so `tmux` resolves), then whatever PATH the
/// install was run with, then the standard system dirs — de-duplicated,
/// order preserved.
fn service_path() -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut push = |p: &str| {
        if !p.is_empty() && !parts.iter().any(|e| e == p) {
            parts.push(p.to_string());
        }
    };
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            push(&dir.to_string_lossy());
        }
    }
    push("/opt/homebrew/bin");
    push("/usr/local/bin");
    if let Ok(path) = std::env::var("PATH") {
        for p in path.split(':') {
            push(p);
        }
    }
    for p in ["/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
        push(p);
    }
    parts.join(":")
}

/// Render the LaunchAgent plist. Pure (all inputs explicit) so it's unit
/// testable without touching disk or launchd. `host`/`port` are passed
/// as `aw serve` args only when set; otherwise the daemon uses its own
/// defaults.
fn render_plist(bin: &str, host: Option<&str>, port: Option<u16>, log: &str, path_env: &str) -> String {
    let mut args = format!("    <string>{}</string>\n    <string>serve</string>\n", xml_escape(bin));
    if let Some(h) = host {
        args.push_str(&format!(
            "    <string>--host</string>\n    <string>{}</string>\n",
            xml_escape(h)
        ));
    }
    if let Some(p) = port {
        args.push_str(&format!(
            "    <string>--port</string>\n    <string>{p}</string>\n"
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
{args}  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>{path}</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>ProcessType</key>
  <string>Background</string>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#,
        label = LABEL,
        args = args,
        path = xml_escape(path_env),
        log = xml_escape(log),
    )
}

/// Minimal XML escaping for the text we interpolate (paths, PATH). Paths
/// can in principle contain `&`/`<`; the rest can't appear in our inputs
/// but escaping them is free insurance.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ---- launchctl (macOS) ---------------------------------------------------

/// The current user's GUI launchd domain target, e.g. `gui/501`.
fn gui_domain() -> Result<String> {
    let out = std::process::Command::new("id")
        .arg("-u")
        .output()
        .context("running `id -u`")?;
    let uid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if uid.is_empty() {
        anyhow::bail!("could not resolve current uid");
    }
    Ok(format!("gui/{uid}"))
}

/// Unload the job if it's currently loaded. Best-effort: a not-loaded job
/// makes `bootout` exit non-zero, which we ignore.
fn bootout() -> Result<()> {
    let domain = gui_domain()?;
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &format!("{domain}/{LABEL}")])
        .stderr(std::process::Stdio::null())
        .status();
    Ok(())
}

/// (Re)load the plist and start it now. Returns false (with a printed
/// hint) if launchctl is unhappy — typically because there's no active
/// GUI session — so the caller can still report the plist was written.
fn reload(plist: &std::path::Path) -> Result<bool> {
    if std::env::var_os("AW_SERVICE_SKIP_LAUNCHCTL").is_some() {
        // Test/CI escape hatch: write the plist but don't touch launchd.
        return Ok(false);
    }
    let domain = gui_domain()?;
    let _ = bootout(); // clear any prior instance first
    let boot = std::process::Command::new("launchctl")
        .args(["bootstrap", &domain])
        .arg(plist)
        .output()
        .context("running launchctl bootstrap")?;
    if !boot.status.success() {
        eprintln!(
            "note: `launchctl bootstrap` failed ({}). The plist is in place; \
             load it from a desktop session with:\n  launchctl bootstrap {} {}",
            String::from_utf8_lossy(&boot.stderr).trim(),
            domain,
            plist.display()
        );
        return Ok(false);
    }
    let target = format!("{domain}/{LABEL}");
    let _ = std::process::Command::new("launchctl").args(["enable", &target]).status();
    let _ = std::process::Command::new("launchctl")
        .args(["kickstart", "-k", &target])
        .status();
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_has_program_args_and_keepalive() {
        let p = render_plist("/Users/me/.local/bin/aw", None, None, "/tmp/serve.log", "/usr/bin:/bin");
        assert!(p.contains("<string>com.agent-workspaces.serve</string>"));
        assert!(p.contains("<string>/Users/me/.local/bin/aw</string>"));
        assert!(p.contains("<string>serve</string>"));
        assert!(p.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(p.contains("<key>KeepAlive</key>\n  <true/>"));
        assert!(p.contains("<string>/usr/bin:/bin</string>"), "PATH baked in");
        assert!(p.contains("<string>/tmp/serve.log</string>"), "log path");
        // No host/port args when unset.
        assert!(!p.contains("--host"));
        assert!(!p.contains("--port"));
    }

    #[test]
    fn plist_includes_host_and_port_when_set() {
        let p = render_plist("/bin/aw", Some("127.0.0.1"), Some(9001), "/l", "/bin");
        assert!(p.contains("<string>--host</string>\n    <string>127.0.0.1</string>"));
        assert!(p.contains("<string>--port</string>\n    <string>9001</string>"));
    }

    #[test]
    fn plist_escapes_xml_metacharacters_in_paths() {
        // A home dir with '&' is unusual but legal; the plist must stay
        // well-formed.
        let p = render_plist("/Users/a&b/aw", None, None, "/l", "/p&q");
        assert!(p.contains("/Users/a&amp;b/aw"));
        assert!(p.contains("/p&amp;q"));
        assert!(!p.contains("a&b/aw"), "raw ampersand must be escaped");
    }

    #[test]
    fn service_path_prepends_homebrew_and_dedupes() {
        // Force a known PATH; service_path should front-load homebrew and
        // not repeat entries.
        std::env::set_var("PATH", "/opt/homebrew/bin:/usr/bin");
        let p = service_path();
        std::env::remove_var("PATH");
        assert!(p.contains("/opt/homebrew/bin"));
        assert!(p.contains("/usr/bin"));
        // /opt/homebrew/bin appears once despite being in PATH and forced.
        assert_eq!(p.matches("/opt/homebrew/bin").count(), 1, "deduped: {p}");
        // Standard dirs are present as a backstop.
        assert!(p.contains("/sbin"));
    }
}
