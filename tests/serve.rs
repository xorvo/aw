//! Integration tests for `aw serve` — the phone remote-control daemon.
//!
//! Each test boots a real server on an ephemeral port with a sandboxed
//! state dir and a fixed token, then speaks raw HTTP/1.1 over TcpStream
//! (`Connection: close` keeps the client trivial). tmux-dependent
//! endpoints are exercised only for their validation paths — CI has no
//! tmux server, and the pane allowlist rejects targets before any tmux
//! call anyway.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

const TOKEN: &str = "test-token-abc";

struct ServeGuard {
    child: Child,
    port: u16,
    _state: TempDir,
}

impl Drop for ServeGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Boot `aw serve --port 0` and parse the bound port from the banner.
fn start_server() -> ServeGuard {
    let state = TempDir::new().expect("state dir");
    std::fs::create_dir_all(state.path().join("panes")).expect("panes dir");
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("aw"))
        .args(["serve", "--host", "127.0.0.1", "--port", "0"])
        .env("AW_STATE_DIR", state.path())
        .env("AW_REMOTE_TOKEN", TOKEN)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn aw serve");

    // Banner line: "  aw serve listening on 127.0.0.1:<port>"
    let stdout = child.stdout.take().expect("piped stdout");
    let mut reader = BufReader::new(stdout);
    let deadline = Instant::now() + Duration::from_secs(10);
    let port = loop {
        assert!(Instant::now() < deadline, "server did not print its banner in time");
        let mut line = String::new();
        let n = reader.read_line(&mut line).expect("read banner");
        assert!(n > 0, "server exited before printing its banner");
        if let Some(rest) = line.trim().strip_prefix("aw serve listening on ") {
            let port: u16 = rest
                .rsplit(':')
                .next()
                .and_then(|p| p.trim().parse().ok())
                .expect("port in banner");
            break port;
        }
    };
    // Drain the rest of the banner in the background so the server never
    // blocks on a full stdout pipe.
    std::thread::spawn(move || {
        let mut sink = String::new();
        let _ = reader.read_to_string(&mut sink);
    });
    ServeGuard { child, port, _state: state }
}

/// One-shot HTTP/1.1 request; returns (status, headers, body).
fn http(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    let mut req = format!("{} {} HTTP/1.1\r\nHost: t\r\nConnection: close\r\n", method, path);
    for (k, v) in headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    if !body.is_empty() {
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    req.push_str(body);
    stream.write_all(req.as_bytes()).expect("send request");

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read response");
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    // tiny_http chunk-encodes string bodies on HTTP/1.1 + Connection:
    // close; strip the framing when present so assertions see clean JSON.
    let body = if head.to_lowercase().contains("transfer-encoding: chunked") {
        dechunk(body)
    } else {
        body.to_string()
    };
    (status, head.to_string(), body)
}

fn dechunk(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some((size_line, after)) = rest.split_once("\r\n") {
        let size = usize::from_str_radix(size_line.trim(), 16).unwrap_or(0);
        if size == 0 {
            break;
        }
        out.push_str(&after[..size.min(after.len())]);
        rest = after.get(size + 2..).unwrap_or("");
    }
    out
}

#[test]
fn rejects_requests_without_token() {
    let server = start_server();
    let (status, _, body) = http(server.port, "GET", "/api/state", &[], "");
    assert_eq!(status, 401, "body: {}", body);
    assert!(body.contains("unauthorized"));

    // Wrong token is rejected too — query, bearer, and cookie forms.
    let (status, _, _) = http(server.port, "GET", "/api/state?t=wrong", &[], "");
    assert_eq!(status, 401);
    let (status, _, _) = http(server.port, "GET", "/api/state", &[("Authorization", "Bearer wrong")], "");
    assert_eq!(status, 401);
    let (status, _, _) = http(server.port, "GET", "/api/state", &[("Cookie", "aw_token=wrong")], "");
    assert_eq!(status, 401);

    // The app shell gives a friendly plain-text hint instead of JSON.
    let (status, _, body) = http(server.port, "GET", "/", &[], "");
    assert_eq!(status, 401);
    assert!(body.contains("?t=YOUR_TOKEN"), "hint missing: {}", body);
}

#[test]
fn accepts_all_three_auth_forms() {
    let server = start_server();
    for headers in [
        vec![("Authorization", "Bearer test-token-abc")],
        vec![("Cookie", "other=1; aw_token=test-token-abc")],
    ] {
        let (status, _, body) = http(server.port, "GET", "/api/state", &headers, "");
        assert_eq!(status, 200, "headers {:?} body {}", headers, body);
    }
    let (status, _, body) = http(server.port, "GET", "/api/state?t=test-token-abc", &[], "");
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).expect("state is json");
    assert!(v["sessions"].is_array(), "got: {}", body);
}

#[test]
fn app_shell_sets_cookie_and_serves_assets() {
    let server = start_server();
    let (status, head, body) = http(server.port, "GET", &format!("/?t={}", TOKEN), &[], "");
    assert_eq!(status, 200);
    assert!(
        head.contains(&format!("Set-Cookie: aw_token={}; Path=/", TOKEN)),
        "cookie header missing:\n{}",
        head
    );
    assert!(body.contains("<title>aw remote</title>"), "app shell html");
    assert!(body.contains("/app.js"), "shell must reference the script");

    // Public assets need no token.
    let (status, _, body) = http(server.port, "GET", "/app.js", &[], "");
    assert_eq!(status, 200);
    assert!(body.contains("EventSource"), "client script content");

    // Icons are binary — fetch raw bytes (the string helper would mangle
    // the 0x89 signature byte through from_utf8_lossy).
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(10))).expect("timeout");
    write!(stream, "GET /icon-180.png HTTP/1.0\r\nHost: t\r\n\r\n").expect("send");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read icon");
    let header_end = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("header end") + 4;
    let head = String::from_utf8_lossy(&raw[..header_end]);
    assert!(head.contains("200"), "head: {}", head);
    assert!(head.contains("image/png"), "head: {}", head);
    assert!(raw[header_end..].starts_with(&[0x89, b'P', b'N', b'G']), "png signature");

    let (status, _, body) = http(server.port, "GET", "/manifest.webmanifest", &[], "");
    assert_eq!(status, 200);
    let m: serde_json::Value = serde_json::from_str(&body).expect("manifest json");
    assert_eq!(m["short_name"], "aw");
}

#[test]
fn pane_endpoints_reject_unknown_panes() {
    let server = start_server();
    let auth = [("Cookie", "aw_token=test-token-abc")];

    let (status, _, body) = http(server.port, "GET", "/api/screen?pane=%25999", &auth, "");
    assert_eq!(status, 404, "body: {}", body);
    assert!(body.contains("unknown pane"));

    let (status, _, body) = http(
        server.port,
        "POST",
        "/api/keys",
        &auth,
        r#"{"pane":"%999","text":"hi"}"#,
    );
    assert_eq!(status, 404, "body: {}", body);

    let (status, _, body) = http(server.port, "POST", "/api/unfit", &auth, "{}");
    assert_eq!(status, 400, "body: {}", body);
    assert!(body.contains("pane required"));

    let (status, _, _) = http(server.port, "GET", "/api/nope", &auth, "");
    assert_eq!(status, 404);
}

#[test]
fn events_stream_pushes_state_immediately() {
    let server = start_server();
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    write!(
        stream,
        "GET /api/events?t={} HTTP/1.1\r\nHost: t\r\n\r\n",
        TOKEN
    )
    .expect("send");

    // The first event must arrive promptly (the flush-per-event path) —
    // read until the SSE blank-line terminator.
    let mut got = String::new();
    let mut buf = [0u8; 4096];
    let deadline = Instant::now() + Duration::from_secs(5);
    while !got.contains("\n\n") {
        assert!(Instant::now() < deadline, "no SSE event within 5s: {:?}", got);
        let n = stream.read(&mut buf).expect("read sse");
        assert!(n > 0, "server closed the stream early: {:?}", got);
        got.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    let (head, body) = got.split_once("\r\n\r\n").expect("response head");
    assert!(head.contains("200"), "head: {}", head);
    assert!(head.contains("text/event-stream"), "head: {}", head);
    let event = body.lines().find(|l| l.starts_with("data: ")).expect("data line");
    let v: serde_json::Value =
        serde_json::from_str(event.trim_start_matches("data: ")).expect("event json");
    assert!(v.is_array(), "events payload is the session array: {}", event);
}
