//! `aw serve` — the phone remote-control daemon (LAN).
//!
//! Rust port of the proven Node prototype (formerly
//! `prototype/remote/aw-remote.mjs`, retired in the same change); the
//! HTTP contract is unchanged. Wraps the dash snapshot + tmux so a
//! phone on the same Wi-Fi can watch every agent session, read its
//! terminal, and type back / approve permission prompts.
//!
//! Security model (LAN-first): bind to the LAN, gate every request behind
//! a bearer token (`Authorization`, `?t=` bootstrap, or cookie), validate
//! pane targets against the live session list, and never build a shell
//! string from client input (arg arrays only — see `term.rs`).

pub mod icon;
pub mod sessions;
pub mod term;

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::dash::remote_link::DEFAULT_PORT;
const PING_EVERY: Duration = Duration::from_secs(25);
const EVENTS_POLL: Duration = Duration::from_millis(1500);
const SCREEN_POLL: Duration = Duration::from_millis(150);
const UPLOAD_MAX: usize = 25 * 1024 * 1024;

const INDEX_HTML: &str = include_str!("assets/index.html");
const APP_JS: &str = include_str!("assets/app.js");

struct ServeState {
    token: String,
    fit: term::FitState,
    panes: sessions::KnownPanes,
}

/// `aw serve [--host H] [--port P]` — flags win, then `AW_REMOTE_HOST` /
/// `AW_REMOTE_PORT`, then `0.0.0.0:<DEFAULT_PORT>`.
pub fn run(host: Option<String>, port: Option<u16>) -> Result<()> {
    let host = host
        .or_else(|| std::env::var("AW_REMOTE_HOST").ok())
        .filter(|h| !h.trim().is_empty())
        .unwrap_or_else(|| "0.0.0.0".into());
    let port = port
        .or_else(|| {
            std::env::var("AW_REMOTE_PORT")
                .ok()
                .and_then(|p| p.trim().parse().ok())
        })
        .unwrap_or(DEFAULT_PORT);

    let token = crate::dash::remote_link::load_or_create_token()?;
    let server = Server::http((host.as_str(), port))
        .map_err(|e| anyhow::anyhow!("binding {}:{}: {}", host, port, e))?;
    // Resolve the real port (matters when the caller passed --port 0).
    let port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .unwrap_or(port);

    // Show only the best-ranked address: alternates (usually VPN tunnel
    // IPs the phone can't reach anyway) read as noise. Revisit when
    // Tailscale/WAN support lands and a tunnel address becomes the
    // legitimate way in.
    let lan = crate::dash::remote_link::lan_ip().unwrap_or_else(|| "localhost".into());
    let url = format!("http://{}:{}/?t={}", lan, port, token);
    println!("\n  aw serve listening on {}:{}", host, port);
    println!("  token: {}", token);
    println!("\n  Open on your phone (same Wi-Fi):");
    println!("    {}", url);
    if let Ok(lines) = crate::dash::remote_link::qr_lines(&url) {
        println!();
        for l in &lines {
            println!("    {}", l);
        }
    }
    println!("\n  Add to Home Screen for an app-like experience.\n");

    let state = Arc::new(ServeState {
        token,
        fit: term::FitState::default(),
        panes: sessions::KnownPanes::new(),
    });
    for request in server.incoming_requests() {
        let state = Arc::clone(&state);
        // Thread-per-request: SSE handlers block for the lifetime of the
        // stream, so they can't share the accept loop.
        std::thread::spawn(move || handle(request, &state));
    }
    Ok(())
}

fn handle(mut req: Request, state: &ServeState) {
    let method = req.method().clone();
    let (path, query) = split_url(req.url());
    let path = path.to_string();

    // serve the app shell (auth via ?t= sets a cookie so later calls are clean)
    if path == "/" && method == Method::Get {
        if !authed(&req, &query, &state.token) {
            respond(
                req,
                Response::from_string("unauthorized — append ?t=YOUR_TOKEN")
                    .with_status_code(401),
            );
            return;
        }
        let cookie = format!(
            "aw_token={}; Path=/; Max-Age=31536000; SameSite=Lax",
            state.token
        );
        respond(
            req,
            Response::from_string(INDEX_HTML)
                .with_header(header("Content-Type", "text/html; charset=utf-8"))
                .with_header(header("Cache-Control", "no-cache"))
                .with_header(header("Set-Cookie", &cookie)),
        );
        return;
    }

    // public, non-sensitive assets (icons are fetched by the OS at
    // add-to-home-screen time, without our cookie; app.js carries no
    // secrets and skipping auth means a blocked cookie can't wedge the
    // shell into a half-loaded state)
    match (&method, path.as_str()) {
        (Method::Get, "/app.js") => {
            // no-cache: the phone revalidates on each launch, so client
            // fixes ship with a plain server restart (there's no asset
            // versioning or service worker to bust).
            respond(
                req,
                Response::from_string(APP_JS)
                    .with_header(header("Content-Type", "text/javascript; charset=utf-8"))
                    .with_header(header("Cache-Control", "no-cache")),
            );
            return;
        }
        (Method::Get, "/icon-180.png") | (Method::Get, "/icon-512.png") => {
            let size = if path.contains("512") { 512 } else { 180 };
            respond(
                req,
                Response::from_data(icon::icon_png(size))
                    .with_header(header("Content-Type", "image/png"))
                    .with_header(header("Cache-Control", "max-age=86400")),
            );
            return;
        }
        (Method::Get, "/manifest.webmanifest") => {
            respond(
                req,
                Response::from_string(manifest())
                    .with_header(header("Content-Type", "application/manifest+json"))
                    .with_header(header("Cache-Control", "max-age=86400")),
            );
            return;
        }
        (Method::Get, "/font.ttf") => {
            match load_font() {
                Some(font) => respond(
                    req,
                    Response::from_data(font)
                        .with_header(header("Content-Type", "font/ttf"))
                        .with_header(header("Cache-Control", "max-age=604800")),
                ),
                None => respond_json(req, 404, serde_json::json!({"error": "no font"})),
            }
            return;
        }
        _ => {}
    }

    // everything else requires auth
    if !authed(&req, &query, &state.token) {
        respond_json(req, 401, serde_json::json!({"error": "unauthorized"}));
        return;
    }

    // SSE routes hold the request (and this thread) for the lifetime of
    // the stream, so they're dispatched here where we still own it.
    if method == Method::Get && path == "/api/screen-stream" {
        // event-driven screen: tail one pane, push only when its content changes
        let pane = query.get("pane").cloned().unwrap_or_default();
        if !state.panes.contains(&pane) {
            respond_json(req, 404, serde_json::json!({"error": "unknown pane"}));
            return;
        }
        let lines = query.get("lines").cloned();
        let mut last: Option<String> = None;
        stream_sse(req, SCREEN_POLL, move || {
            let screen = term::capture_screen(&pane, lines.as_deref()).ok()?;
            if last.as_deref() == Some(screen.as_str()) {
                return None;
            }
            let event = serde_json::to_string(&screen).ok()?;
            last = Some(screen);
            Some(event)
        });
        return;
    }
    if method == Method::Get && path == "/api/events" {
        let mut last = String::new();
        stream_sse(req, EVENTS_POLL, move || {
            let s = sessions::sessions_value().ok()?.to_string();
            if s == last {
                return None;
            }
            last = s.clone();
            Some(s)
        });
        return;
    }

    match api(&mut req, &method, &path, &query, state) {
        Ok(reply) => respond_json(req, reply.0, reply.1),
        Err(e) => respond_json(req, 500, serde_json::json!({"error": e.to_string()})),
    }
}

/// Plain-JSON API routing. Returns `(status, body)`; errors map to 500.
fn api(
    req: &mut Request,
    method: &Method,
    path: &str,
    query: &HashMap<String, String>,
    state: &ServeState,
) -> Result<(u16, serde_json::Value)> {
    match (method, path) {
        (Method::Get, "/api/state") => {
            Ok((200, serde_json::json!({"sessions": sessions::sessions_value()?})))
        }

        (Method::Get, "/api/screen") => {
            let pane = query.get("pane").map(String::as_str).unwrap_or("");
            if !state.panes.contains(pane) {
                return Ok((404, serde_json::json!({"error": "unknown pane"})));
            }
            let screen = term::capture_screen(pane, query.get("lines").map(String::as_str))?;
            Ok((200, serde_json::json!({"pane": pane, "screen": screen})))
        }

        (Method::Post, "/api/keys") => {
            let body = read_json_body(req, 1024 * 1024)?;
            let pane = body["pane"].as_str().unwrap_or("");
            if !state.panes.contains(pane) {
                return Ok((404, serde_json::json!({"error": "unknown pane"})));
            }
            term::send_keys(
                pane,
                &term::KeysRequest {
                    text: body["text"].as_str(),
                    key: body["key"].as_str(),
                    submit: body["submit"].as_bool().unwrap_or(false),
                    paste: body["paste"].as_str(),
                },
            )?;
            Ok((200, serde_json::json!({"ok": true})))
        }

        (Method::Post, "/api/resize") => {
            let body = read_json_body(req, 64 * 1024)?;
            let pane = body["pane"].as_str().unwrap_or("");
            if !state.panes.contains(pane) {
                return Ok((404, serde_json::json!({"error": "unknown pane"})));
            }
            let (cols, rows) = state.fit.fit(
                pane,
                body["cols"].as_i64().unwrap_or(0),
                body["rows"].as_i64().unwrap_or(0),
            )?;
            Ok((200, serde_json::json!({"ok": true, "cols": cols, "rows": rows})))
        }

        (Method::Post, "/api/unfit") => {
            let body = read_json_body(req, 64 * 1024)?;
            let pane = body["pane"].as_str().unwrap_or("");
            if pane.is_empty() {
                return Ok((400, serde_json::json!({"error": "pane required"})));
            }
            let _ = state.fit.unfit(pane); // pane may already be gone
            Ok((200, serde_json::json!({"ok": true})))
        }

        (Method::Post, "/api/upload") => {
            let ct = header_value(req, "Content-Type").unwrap_or_default();
            let ext = if ct.contains("png") {
                "png"
            } else if ct.contains("jpeg") || ct.contains("jpg") {
                "jpg"
            } else if ct.contains("webp") {
                "webp"
            } else if ct.contains("heic") {
                "heic"
            } else {
                "bin"
            };
            let mut buf = Vec::new();
            req.as_reader()
                .take(UPLOAD_MAX as u64 + 1)
                .read_to_end(&mut buf)
                .context("reading upload body")?;
            if buf.len() > UPLOAD_MAX {
                return Ok((413, serde_json::json!({"error": "too large"})));
            }
            let dir = crate::dash::state_root()?.join("uploads");
            std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
            let ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let name = format!("shot-{}.{}", ms, ext);
            let path = dir.join(&name);
            std::fs::write(&path, &buf).with_context(|| format!("writing {}", path.display()))?;
            println!("  upload: {}", path.display());
            Ok((
                200,
                serde_json::json!({"ok": true, "name": name, "path": path.display().to_string()}),
            ))
        }

        _ => Ok((404, serde_json::json!({"error": "not found"}))),
    }
}

// ---- SSE ---------------------------------------------------------------

/// Serve a `text/event-stream` response on this thread until the client
/// disconnects. `tick` is polled every `interval` and returns the next
/// `data:` payload when something changed (it must be newline-free —
/// JSON-encode anything multi-line). A comment ping goes out when idle
/// for 25 s so proxies don't reap the connection and dead clients get
/// noticed (the failed write ends the loop).
///
/// The response is written by hand via `into_writer` rather than a
/// `Response` body: tiny_http only flushes its 1 KiB write buffer when a
/// response *completes*, so small SSE events would sit in the buffer
/// forever. `Connection: close` makes the unframed body valid HTTP/1.1
/// (EventSource reconnects on a fresh socket anyway).
fn stream_sse<F>(req: Request, interval: Duration, mut tick: F)
where
    F: FnMut() -> Option<String>,
{
    use std::io::Write;
    let mut w = req.into_writer();
    let head = "HTTP/1.1 200 OK\r\n\
                Content-Type: text/event-stream\r\n\
                Cache-Control: no-store\r\n\
                Connection: close\r\n\r\n";
    if w.write_all(head.as_bytes()).and_then(|_| w.flush()).is_err() {
        return;
    }
    let mut last_ping = Instant::now();
    let mut first = true;
    loop {
        if first {
            first = false; // first tick fires immediately
        } else {
            std::thread::sleep(interval);
        }
        let payload = match tick() {
            Some(event) => format!("data: {}\n\n", event),
            None if last_ping.elapsed() >= PING_EVERY => {
                last_ping = Instant::now();
                ": ping\n\n".to_string()
            }
            None => continue,
        };
        if w.write_all(payload.as_bytes()).and_then(|_| w.flush()).is_err() {
            return; // client gone
        }
    }
}

// ---- http plumbing -------------------------------------------------------

fn authed(req: &Request, query: &HashMap<String, String>, token: &str) -> bool {
    if let Some(auth) = header_value(req, "Authorization") {
        if auth == format!("Bearer {}", token) {
            return true;
        }
    }
    if query.get("t").map(String::as_str) == Some(token) {
        return true;
    }
    if let Some(cookie) = header_value(req, "Cookie") {
        let want = format!("aw_token={}", token);
        if cookie.split(';').any(|c| c.trim() == want) {
            return true;
        }
    }
    false
}

fn header_value(req: &Request, name: &str) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_string())
}

/// Build a static header. The inputs are compile-time constants (or a
/// token, which is base64url), so parsing can't fail.
fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes())
        .unwrap_or_else(|_| unreachable!("static header {name}"))
}

fn respond<R: Read>(req: Request, response: Response<R>) {
    // Client disconnects surface here as io errors; nothing to do.
    let _ = req.respond(response);
}

fn respond_json(req: Request, status: u16, body: serde_json::Value) {
    respond(
        req,
        Response::from_string(body.to_string())
            .with_status_code(StatusCode(status))
            .with_header(header("Content-Type", "application/json"))
            .with_header(header("Cache-Control", "no-store")),
    );
}

/// Lenient body parse, like the prototype: malformed/empty JSON becomes
/// `{}` so routes fall through to their own "pane required" handling.
fn read_json_body(req: &mut Request, limit: u64) -> Result<serde_json::Value> {
    let mut buf = Vec::new();
    req.as_reader()
        .take(limit)
        .read_to_end(&mut buf)
        .context("reading request body")?;
    Ok(serde_json::from_slice(&buf).unwrap_or_else(|_| serde_json::json!({})))
}

/// Split a request URL into its path and percent-decoded query params
/// (`+` decodes to space, matching `URLSearchParams`). The path is left
/// encoded — routes compare literal constants, as the prototype did.
fn split_url(url: &str) -> (&str, HashMap<String, String>) {
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url, ""),
    };
    let mut params = HashMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        params.insert(percent_decode(k), percent_decode(v));
    }
    (path, params)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                // bytes.get bounds-checks the 2-char hex lookahead.
                let hex = bytes.get(i + 1..i + 3).and_then(|h| {
                    u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()
                });
                match hex {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn manifest() -> String {
    serde_json::json!({
        "name": "aw sessions", "short_name": "aw", "display": "standalone",
        "background_color": "#0b0e14", "theme_color": "#0b0e14", "start_url": ".",
        "icons": [{"src": "icon-512.png", "sizes": "512x512", "type": "image/png"}],
    })
    .to_string()
}

/// Nerd Font served to the phone so UI glyphs + terminal powerline
/// symbols render. Falls back through a few common filenames.
fn load_font() -> Option<Vec<u8>> {
    let home = dirs::home_dir()?;
    let candidates = [
        std::env::var("AW_FONT").ok().map(std::path::PathBuf::from),
        Some(home.join("Library/Fonts/MesloLGSNerdFontMono-Regular.ttf")),
        Some(home.join("Library/Fonts/MesloLGSNerdFont-Regular.ttf")),
        Some(home.join("Library/Fonts/FiraCodeNerdFontMono-Regular.ttf")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find_map(|p| std::fs::read(p).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_url_decodes_query_params() {
        let (path, q) = split_url("/api/screen?pane=%2542&lines=80");
        assert_eq!(path, "/api/screen");
        assert_eq!(q.get("pane").map(String::as_str), Some("%42")); // %25 -> '%'
        assert_eq!(q.get("lines").map(String::as_str), Some("80"));
    }

    #[test]
    fn split_url_handles_no_query_and_plus() {
        let (path, q) = split_url("/");
        assert_eq!(path, "/");
        assert!(q.is_empty());
        let (_, q) = split_url("/x?a=hello+world&b=");
        assert_eq!(q.get("a").map(String::as_str), Some("hello world"));
        assert_eq!(q.get("b").map(String::as_str), Some(""));
    }

    #[test]
    fn percent_decode_tolerates_malformed_sequences() {
        assert_eq!(percent_decode("a%2Gb"), "a%2Gb"); // bad hex -> literal
        assert_eq!(percent_decode("trailing%2"), "trailing%2");
        assert_eq!(percent_decode("%"), "%");
        assert_eq!(percent_decode("caf%C3%A9"), "café");
    }

    #[test]
    fn manifest_is_valid_json_with_expected_fields() {
        let m: serde_json::Value = serde_json::from_str(&manifest()).expect("valid json");
        assert_eq!(m["short_name"], "aw");
        assert_eq!(m["icons"][0]["src"], "icon-512.png");
    }
}
