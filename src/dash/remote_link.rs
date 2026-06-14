//! Phone-pairing link for the remote-control daemon.
//!
//! The dashboard and the `aw serve` daemon share
//! `~/.cache/aw/remote-token` (`AW_REMOTE_TOKEN` overrides); whichever
//! runs first generates it. The dashboard renders the resulting
//! `http://<lan-ip>:<port>/?t=<token>` URL as a terminal QR code so a
//! phone on the same Wi-Fi can connect without typing.

use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result};

/// Default port the remote daemon listens on (`AW_REMOTE_PORT` overrides).
/// Single source of truth shared by the QR pairing URL here and the
/// server in `crate::serve`. 7340 sits in the quiet 7xxx band, clear of
/// the congested 8xxx web-dev range (8080/8443/8787-RStudio/8888) that
/// tends to collide with other local dev servers.
pub const DEFAULT_PORT: u16 = 7340;

/// `http://<lan-ip>:<port>/?t=<token>` — what the phone opens.
pub fn pairing_url() -> Result<String> {
    let token = load_or_create_token()?;
    let port = std::env::var("AW_REMOTE_PORT")
        .ok()
        .and_then(|p| p.trim().parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);
    let host = lan_ip().unwrap_or_else(|| "localhost".into());
    Ok(format!("http://{}:{}/?t={}", host, port, token))
}

/// `~/.cache/aw/remote-token` (or `$AW_STATE_DIR/remote-token`).
fn token_file() -> Result<PathBuf> {
    Ok(crate::dash::state_root()?.join("remote-token"))
}

/// Shared by the dash QR overlay and `aw serve` — both sides must agree
/// on the token, so this is the only place that resolves it.
pub(crate) fn load_or_create_token() -> Result<String> {
    if let Ok(t) = std::env::var("AW_REMOTE_TOKEN") {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return Ok(t);
        }
    }
    let file = token_file()?;
    if let Ok(existing) = std::fs::read_to_string(&file) {
        let t = existing.trim().to_string();
        if !t.is_empty() {
            return Ok(t);
        }
    }
    let token = generate_token()?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
    }
    write_secret(&file, &token)
        .with_context(|| format!("writing {}", file.display()))?;
    Ok(token)
}

/// 128-bit token, base64url — same shape the Node daemon generates.
fn generate_token() -> Result<String> {
    let mut buf = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .context("opening /dev/urandom")?
        .read_exact(&mut buf)
        .context("reading /dev/urandom")?;
    Ok(base64url(&buf))
}

#[cfg(unix)]
fn write_secret(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(contents.as_bytes())
}

#[cfg(not(unix))]
fn write_secret(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    std::fs::write(path, contents)
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = ((chunk[0] as u32) << 16)
            | ((*chunk.get(1).unwrap_or(&0) as u32) << 8)
            | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        }
    }
    out
}

/// The machine's LAN addresses, best candidate first. Enumerates real
/// interfaces and ranks them — a plain UDP-connect route lookup is wrong
/// whenever a VPN is up: the default route goes through the tunnel
/// (e.g. `utun4` → 198.18.0.1), which the phone can't reach.
pub(crate) fn lan_ips() -> Vec<String> {
    let cands: Vec<(String, std::net::Ipv4Addr)> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(v4) => Some((i.name, v4)),
            _ => None,
        })
        .collect();
    let ranked = rank_lan_candidates(cands);
    if !ranked.is_empty() {
        return ranked;
    }
    // Fallback (enumeration failed): UDP-connect route lookup — no
    // packet leaves the machine, `local_addr` reports the chosen
    // interface address.
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .ok()
        .map(|a| a.ip())
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
        .map(|ip| ip.to_string())
        .into_iter()
        .collect()
}

/// Best LAN address for pairing URLs, or None if the machine has none.
pub(crate) fn lan_ip() -> Option<String> {
    lan_ips().into_iter().next()
}

/// Order candidate (interface, IPv4) pairs by how likely a phone on the
/// same Wi-Fi can reach them. Private RFC1918 addresses on physical
/// interfaces win; tunnel interfaces and CGNAT/benchmark ranges (the
/// fingerprints of VPNs) sink to the bottom. Loopback, link-local, and
/// unspecified addresses are dropped entirely.
fn rank_lan_candidates(cands: Vec<(String, std::net::Ipv4Addr)>) -> Vec<String> {
    let mut ranked: Vec<(u32, String, String)> = cands
        .into_iter()
        .filter(|(_, ip)| !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified())
        .map(|(name, ip)| {
            let o = ip.octets();
            let mut score: u32 = if ip.is_private() {
                0
            } else if o[0] == 100 && (64..128).contains(&o[1]) {
                20 // CGNAT 100.64/10 (Tailscale et al.)
            } else if o[0] == 198 && (18..20).contains(&o[1]) {
                30 // benchmark 198.18/15 (WARP-style tunnels)
            } else {
                10 // public — unusual on a laptop, but reachable in theory
            };
            if is_tunnel_ifname(&name) {
                score += 5;
            }
            (score, name, ip.to_string())
        })
        .collect();
    // Tie-break on interface name so en0 beats en1 deterministically.
    ranked.sort();
    ranked.into_iter().map(|(_, _, ip)| ip).collect()
}

fn is_tunnel_ifname(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["utun", "tun", "tap", "wg", "ppp", "ipsec", "zt", "tailscale", "awdl", "llw", "bridge", "vmnet", "docker", "veth"]
        .iter()
        .any(|p| n.starts_with(p))
}

/// Render `text` as QR half-block lines. Each terminal row carries two
/// module rows (`▀`/`▄`/`█`/space); light modules are the drawn blocks so
/// the code shows standard dark-on-light polarity on a dark terminal.
/// Includes the spec's quiet zone (drawn light) on all sides.
pub fn qr_lines(text: &str) -> Result<Vec<String>> {
    let qr = qrcodegen::QrCode::encode_text(text, qrcodegen::QrCodeEcc::Low)
        .map_err(|e| anyhow::anyhow!("QR encode failed: {:?}", e))?;
    const QUIET: i32 = 2;
    let size = qr.size();
    let width = (size + 2 * QUIET) as usize;
    let mut lines = Vec::with_capacity(width.div_ceil(2));
    let mut y = -QUIET;
    while y < size + QUIET {
        let mut line = String::with_capacity(width * 3);
        for x in -QUIET..size + QUIET {
            // get_module returns false (light) for out-of-bounds, which
            // conveniently paints the quiet zone.
            let top_dark = qr.get_module(x, y);
            let bottom_dark = qr.get_module(x, y + 1);
            line.push(match (top_dark, bottom_dark) {
                (false, false) => '█',
                (false, true) => '▀',
                (true, false) => '▄',
                (true, true) => ' ',
            });
        }
        lines.push(line);
        y += 2;
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_known_vectors() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(&[0]), "AA");
        assert_eq!(base64url(b"hello world!"), "aGVsbG8gd29ybGQh");
        // The two chars outside the standard alphabet: 0xfb 0xff → "-_8".
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn generated_token_is_22_chars_urlsafe() {
        let t = generate_token().expect("urandom available");
        assert_eq!(t.len(), 22, "16 bytes → 22 base64url chars: {:?}", t);
        assert!(
            t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "token must be URL-safe: {:?}",
            t
        );
    }

    #[test]
    fn qr_lines_are_rectangular_and_block_drawn() {
        let lines = qr_lines("http://192.168.1.10:8787/?t=abc123").expect("encodable");
        assert!(!lines.is_empty());
        let width = lines[0].chars().count();
        assert!(width >= 25, "v1 QR + quiet zone is at least 25 wide, got {}", width);
        for l in &lines {
            assert_eq!(l.chars().count(), width, "ragged QR line");
        }
        // Half-block rendering: two module rows per line.
        assert_eq!(lines.len(), width.div_ceil(2));
        // The quiet zone guarantees the first line is solid light blocks.
        assert!(lines[0].chars().all(|c| c == '█'), "top quiet zone: {:?}", lines[0]);
        // And the body must contain dark modules (spaces or half blocks).
        assert!(
            lines.iter().any(|l| l.chars().any(|c| c != '█')),
            "QR body should contain dark modules"
        );
    }

    #[test]
    fn qr_lines_round_trip_to_the_module_grid() {
        // Reconstruct every module from the half-block characters and
        // compare against the library's grid — catches any inversion,
        // mirroring, or off-by-one in the renderer.
        let text = "http://10.0.0.7:8787/?t=YWJjZGVmZ2hpamtsbW5vcA";
        let qr = qrcodegen::QrCode::encode_text(text, qrcodegen::QrCodeEcc::Low).unwrap();
        let lines = qr_lines(text).unwrap();
        const QUIET: i32 = 2;
        for (row, line) in lines.iter().enumerate() {
            for (col, ch) in line.chars().enumerate() {
                let x = col as i32 - QUIET;
                let y = 2 * row as i32 - QUIET;
                let (top_dark, bottom_dark) = match ch {
                    '█' => (false, false),
                    '▀' => (false, true),
                    '▄' => (true, false),
                    ' ' => (true, true),
                    other => panic!("unexpected char {:?}", other),
                };
                assert_eq!(qr.get_module(x, y), top_dark, "top at ({}, {})", x, y);
                assert_eq!(qr.get_module(x, y + 1), bottom_dark, "bottom at ({}, {})", x, y);
            }
        }
    }

    #[test]
    fn lan_ranking_prefers_real_lan_over_vpn_tunnel() {
        // The exact situation from the field: WARP-style tunnel owns the
        // default route, but the phone can only reach the en1 address.
        let ranked = rank_lan_candidates(vec![
            ("utun4".into(), "198.18.0.1".parse().unwrap()),
            ("en1".into(), "192.168.50.17".parse().unwrap()),
            ("lo0".into(), "127.0.0.1".parse().unwrap()),
        ]);
        assert_eq!(ranked.first().map(String::as_str), Some("192.168.50.17"));
        assert!(!ranked.contains(&"127.0.0.1".to_string()), "loopback dropped");
    }

    #[test]
    fn lan_ranking_demotes_cgnat_and_tunnel_names() {
        // Tailscale (CGNAT range on a tunnel) loses to a wired RFC1918.
        let ranked = rank_lan_candidates(vec![
            ("tailscale0".into(), "100.101.102.103".parse().unwrap()),
            ("eth0".into(), "10.1.2.3".parse().unwrap()),
        ]);
        assert_eq!(ranked, vec!["10.1.2.3".to_string(), "100.101.102.103".to_string()]);

        // A VPN that hands out RFC1918 internally still loses to a
        // physical interface with RFC1918 (name-based penalty).
        let ranked = rank_lan_candidates(vec![
            ("utun0".into(), "10.8.0.2".parse().unwrap()),
            ("en0".into(), "192.168.1.5".parse().unwrap()),
        ]);
        assert_eq!(ranked.first().map(String::as_str), Some("192.168.1.5"));
    }

    #[test]
    fn lan_ranking_filters_link_local_and_ties_break_by_name() {
        let ranked = rank_lan_candidates(vec![
            ("awdl0".into(), "169.254.3.4".parse().unwrap()),
            ("en1".into(), "192.168.0.2".parse().unwrap()),
            ("en0".into(), "192.168.0.1".parse().unwrap()),
        ]);
        assert_eq!(ranked, vec!["192.168.0.1".to_string(), "192.168.0.2".to_string()]);
    }

    #[test]
    fn lan_ranking_falls_back_to_vpn_when_it_is_all_there_is() {
        // Headless box reachable only via Tailscale: better to show the
        // tunnel IP than nothing.
        let ranked = rank_lan_candidates(vec![
            ("tailscale0".into(), "100.96.0.7".parse().unwrap()),
        ]);
        assert_eq!(ranked, vec!["100.96.0.7".to_string()]);
    }

    #[test]
    #[serial_test::serial]
    fn token_env_override_wins() {
        std::env::set_var("AW_REMOTE_TOKEN", "fixed-token-123");
        let url = pairing_url().expect("url");
        std::env::remove_var("AW_REMOTE_TOKEN");
        assert!(url.ends_with("/?t=fixed-token-123"), "got {}", url);
        assert!(url.starts_with("http://"), "got {}", url);
    }

    #[test]
    #[serial_test::serial]
    fn token_is_generated_once_and_persisted() {
        std::env::remove_var("AW_REMOTE_TOKEN");
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("AW_STATE_DIR", dir.path());
        let first = load_or_create_token().expect("generate");
        let second = load_or_create_token().expect("reload");
        std::env::remove_var("AW_STATE_DIR");
        assert_eq!(first, second, "second call must reuse the cached token");
        let on_disk = std::fs::read_to_string(dir.path().join("remote-token")).unwrap();
        assert_eq!(on_disk.trim(), first);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("remote-token"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "token file must be private");
        }
    }

    #[test]
    #[serial_test::serial]
    fn port_env_override_lands_in_url() {
        std::env::set_var("AW_REMOTE_TOKEN", "tok");
        std::env::set_var("AW_REMOTE_PORT", "9999");
        let url = pairing_url().expect("url");
        std::env::remove_var("AW_REMOTE_PORT");
        std::env::remove_var("AW_REMOTE_TOKEN");
        assert!(url.contains(":9999/"), "got {}", url);
    }
}
