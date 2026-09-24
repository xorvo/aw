//! `aw install service` — run `aw serve` at login.
//!
//! Two backends, one per platform, behind a single command:
//!
//!  - **macOS**: a launchd LaunchAgent at
//!    `~/Library/LaunchAgents/com.agent-workspaces.serve.plist`, loaded
//!    into the per-user GUI domain.
//!  - **Linux**: a systemd *user* unit at
//!    `~/.config/systemd/user/aw-serve.service`, enabled in the user
//!    manager's `default.target`.
//!
//! Both are idempotent: re-running rewrites the unit file and reloads,
//! which is also how an upgrade refreshes the running daemon onto a new
//! binary (`refresh_after_upgrade`, called from `aw self update`).
//!
//! Why a baked PATH: neither launchd agents nor systemd user units
//! inherit your shell's environment, but `aw serve` shells out to
//! `tmux` — often in `/opt/homebrew/bin` or `~/.local/bin`, neither on
//! the default PATH. We capture a sensible PATH at install time and
//! write it into the unit so tmux is found at runtime.
//!
//! Note for the Linux login case: a systemd user manager normally exits
//! when your last session ends. `loginctl enable-linger <user>` keeps it
//! (and therefore `aw serve`) running between logins; we print that hint
//! rather than doing it, since it needs privileges.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// launchd label and plist basename. Reverse-DNS by convention; stable
/// across versions so self-update never has to rename anything.
const LABEL: &str = "com.agent-workspaces.serve";

/// systemd user unit name. Same stability requirement as `LABEL`.
const UNIT: &str = "aw-serve.service";

/// Which init system this build drives. Split out so every `cfg!` test
/// lives in one place and the rest of the module reads as plain dispatch.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Backend {
    Launchd,
    Systemd,
}

fn backend() -> Result<Backend> {
    if cfg!(target_os = "macos") {
        Ok(Backend::Launchd)
    } else if cfg!(target_os = "linux") {
        Ok(Backend::Systemd)
    } else {
        anyhow::bail!(
            "`aw install service` supports macOS (launchd) and Linux (systemd user units); \
             this is {}. Run `aw serve` yourself, or wire it into your own supervisor.",
            std::env::consts::OS
        )
    }
}

/// Where this platform's unit file lives: the launchd plist, or the
/// systemd user unit.
pub fn unit_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    Ok(match backend()? {
        Backend::Launchd => home.join("Library/LaunchAgents").join(format!("{LABEL}.plist")),
        Backend::Systemd => home.join(".config/systemd/user").join(UNIT),
    })
}

/// Whether the unit file is present (i.e. the service has been
/// installed). Cheap file check — used by the upgrade hook to decide
/// whether there's anything to refresh.
pub fn is_installed() -> bool {
    unit_path().map(|p| p.is_file()).unwrap_or(false)
}

/// Install (or reinstall) the login service. `host`/`port` override the
/// `aw serve` defaults (`0.0.0.0:<DEFAULT_PORT>`) when provided.
pub fn install(host: Option<&str>, port: Option<u16>) -> Result<()> {
    let backend = backend()?;
    let exe = std::env::current_exe().context("resolving the aw binary path")?;
    let log = log_path()?;
    let unit = unit_path()?;
    let bin = exe.to_string_lossy();
    let body = match backend {
        Backend::Launchd => render_plist(&bin, host, port, &log.to_string_lossy(), &service_path()),
        Backend::Systemd => render_unit(&bin, host, port, &log.to_string_lossy(), &service_path()),
    };

    if let Some(dir) = unit.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&unit, &body).with_context(|| format!("writing {}", unit.display()))?;

    match backend {
        Backend::Launchd => println!("Wrote LaunchAgent: {}", unit.display()),
        Backend::Systemd => println!("Wrote systemd user unit: {}", unit.display()),
    }

    let activated = match backend {
        Backend::Launchd => launchd_reload(&unit)?,
        Backend::Systemd => systemd_reload()?,
    };
    if activated {
        let where_ = host.unwrap_or("0.0.0.0");
        let p = port.unwrap_or(crate::dash::remote_link::DEFAULT_PORT);
        println!("Loaded and started — aw serve will run at login on {where_}:{p}.");
        println!("Logs: {}", log.display());
        if backend == Backend::Systemd {
            // Without lingering, the user manager (and aw serve with it)
            // is torn down when the last session ends — so the daemon
            // would not actually be up to greet a phone before login.
            println!(
                "To keep it running while you're logged out: \
                 sudo loginctl enable-linger {}",
                whoami()
            );
        }
        println!("Stop/remove with: aw install service --uninstall");
    }
    Ok(())
}

/// Remove the login service: unload it and delete the unit file.
pub fn uninstall() -> Result<()> {
    let backend = backend()?;
    let unit = unit_path()?;
    if !unit.exists() {
        println!("No aw serve service installed (nothing at {}).", unit.display());
        return Ok(());
    }
    // Unload before deleting so the init system forgets the job immediately.
    match backend {
        Backend::Launchd => {
            let _ = launchd_bootout();
        }
        Backend::Systemd => {
            let _ = systemctl(&["disable", "--now", UNIT]);
        }
    }
    std::fs::remove_file(&unit).with_context(|| format!("removing {}", unit.display()))?;
    if backend == Backend::Systemd {
        let _ = systemctl(&["daemon-reload"]);
    }
    println!("Removed aw serve service ({}).", unit.display());
    Ok(())
}

/// Called after `aw self update` swaps the binary. If the service is
/// installed, rewrite the unit (paths/PATH may have shifted) and restart
/// it so the running daemon comes up on the new binary. A no-op — and
/// never an error — when no service is installed; the upgrade must not
/// fail just because the daemon couldn't be bounced.
pub fn refresh_after_upgrade() {
    if !is_installed() {
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

/// Test/CI escape hatch: write the unit file but don't touch the init
/// system. `AW_SERVICE_SKIP_LAUNCHCTL` is the original (macOS-era) name,
/// still honored so existing setups keep working.
fn skip_activation() -> bool {
    std::env::var_os("AW_SERVICE_SKIP_ACTIVATION").is_some()
        || std::env::var_os("AW_SERVICE_SKIP_LAUNCHCTL").is_some()
}

/// Login name, for the `enable-linger` hint. Falls back to `$USER`, then
/// a placeholder — it only ever ends up in printed advice.
fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "$USER".to_string())
}

/// PATH to bake into the unit: the aw binary's own dir and the common
/// package-manager prefixes first (so `tmux` resolves), then whatever
/// PATH the install was run with, then the standard system dirs —
/// de-duplicated, order preserved.
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
    // Homebrew (macOS arm64 / Intel) and Homebrew on Linux.
    push("/opt/homebrew/bin");
    push("/usr/local/bin");
    push("/home/linuxbrew/.linuxbrew/bin");
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

// ---- launchd (macOS) -----------------------------------------------------

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
    <!-- launchd provides no locale; without a UTF-8 one, tmux strips the
         tab separators from its -F output and the session list parses
         empty. aw forces this per-tmux-call too, but set it here as well. -->
    <key>LC_ALL</key>
    <string>en_US.UTF-8</string>
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
fn launchd_bootout() -> Result<()> {
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
fn launchd_reload(plist: &Path) -> Result<bool> {
    if skip_activation() {
        return Ok(false);
    }
    let domain = gui_domain()?;
    let _ = launchd_bootout(); // clear any prior instance first
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

// ---- systemd (Linux) -----------------------------------------------------

/// Render the systemd user unit. Pure, for the same reason as
/// [`render_plist`]. `StandardOutput=append:` needs systemd 240+ (2018);
/// older systems still get the unit, they just log to the journal
/// instead of the file.
fn render_unit(bin: &str, host: Option<&str>, port: Option<u16>, log: &str, path_env: &str) -> String {
    let mut exec = unit_quote(bin);
    exec.push_str(" serve");
    if let Some(h) = host {
        exec.push_str(&format!(" --host {}", unit_quote(h)));
    }
    if let Some(p) = port {
        exec.push_str(&format!(" --port {p}"));
    }
    format!(
        r#"[Unit]
Description=aw serve — phone remote control for agent workspaces
Documentation=https://github.com/xorvo/aw/blob/main/docs/serve.md
After=default.target

[Service]
Type=simple
ExecStart={exec}
Restart=always
RestartSec=2
# systemd user units inherit almost no environment; aw serve shells out
# to tmux, so bake a PATH that can find it.
Environment="PATH={path}"
# Without a UTF-8 locale, tmux strips the tab separators from its -F
# output and the session list parses empty. aw forces this per-tmux-call
# too, but set it here as well.
Environment="LC_ALL=C.UTF-8"
StandardOutput=append:{log}
StandardError=append:{log}

[Install]
WantedBy=default.target
"#,
        exec = exec,
        path = unit_escape(path_env),
        log = unit_escape(log),
    )
}

/// Escape a value for a systemd double-quoted string: backslash and
/// double quote are the only characters with meaning inside one.
fn unit_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Quote a single `ExecStart` word so paths with spaces survive
/// systemd's command-line splitting.
fn unit_quote(s: &str) -> String {
    format!("\"{}\"", unit_escape(s))
}

/// Run `systemctl --user <args>`, returning whether it succeeded and the
/// captured stderr. Never propagates a spawn failure as an error — a box
/// with no systemd should get a hint, not a stack of context.
fn systemctl(args: &[&str]) -> (bool, String) {
    match std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
    {
        Ok(out) => (
            out.status.success(),
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ),
        Err(e) => (false, e.to_string()),
    }
}

/// Reload the user manager, enable the unit for login, and (re)start it
/// now. Returns false (with a printed hint) if systemctl is unavailable
/// or unhappy — typically no user D-Bus session — so the caller can
/// still report the unit was written.
fn systemd_reload() -> Result<bool> {
    if skip_activation() {
        return Ok(false);
    }
    let (ok, err) = systemctl(&["daemon-reload"]);
    if !ok {
        eprintln!(
            "note: `systemctl --user daemon-reload` failed ({err}). The unit is in \
             place; enable it from a logged-in session with:\n  \
             systemctl --user daemon-reload && systemctl --user enable --now {UNIT}"
        );
        return Ok(false);
    }
    let (ok, err) = systemctl(&["enable", UNIT]);
    if !ok {
        eprintln!("note: `systemctl --user enable {UNIT}` failed ({err}).");
        return Ok(false);
    }
    // `enable --now` won't restart an already-running unit, and a refresh
    // after an upgrade needs exactly that — so start with `restart`.
    let (ok, err) = systemctl(&["restart", UNIT]);
    if !ok {
        eprintln!("note: `systemctl --user restart {UNIT}` failed ({err}).");
        return Ok(false);
    }
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
    fn unit_has_execstart_restart_and_install_section() {
        let u = render_unit("/home/me/.local/bin/aw", None, None, "/tmp/serve.log", "/usr/bin:/bin");
        assert!(u.contains(r#"ExecStart="/home/me/.local/bin/aw" serve"#), "{u}");
        assert!(u.contains("Restart=always"));
        assert!(u.contains("WantedBy=default.target"));
        assert!(u.contains(r#"Environment="PATH=/usr/bin:/bin""#), "PATH baked in");
        assert!(u.contains("StandardOutput=append:/tmp/serve.log"));
        assert!(u.contains("StandardError=append:/tmp/serve.log"));
        assert!(!u.contains("--host"));
        assert!(!u.contains("--port"));
    }

    #[test]
    fn unit_includes_host_and_port_when_set() {
        let u = render_unit("/bin/aw", Some("127.0.0.1"), Some(9001), "/l", "/bin");
        assert!(u.contains(r#"ExecStart="/bin/aw" serve --host "127.0.0.1" --port 9001"#), "{u}");
    }

    #[test]
    fn unit_quotes_paths_with_spaces_and_escapes_quotes() {
        let u = render_unit(r#"/home/a b/aw"#, None, None, "/l", r#"/p"q"#);
        assert!(u.contains(r#"ExecStart="/home/a b/aw" serve"#), "{u}");
        // A quote in PATH must not terminate the Environment= value early.
        assert!(u.contains(r#"Environment="PATH=/p\"q""#), "{u}");
    }

    #[test]
    fn service_path_prepends_package_prefixes_and_dedupes() {
        // Force a known PATH; service_path should front-load the package
        // prefixes and not repeat entries.
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

    #[test]
    fn unit_path_matches_the_platform_backend() {
        let p = unit_path().unwrap();
        match backend().unwrap() {
            Backend::Launchd => assert!(p.ends_with("Library/LaunchAgents/com.agent-workspaces.serve.plist")),
            Backend::Systemd => assert!(p.ends_with(".config/systemd/user/aw-serve.service")),
        }
    }
}
