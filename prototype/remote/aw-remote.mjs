#!/usr/bin/env node
// aw-remote — LAN control plane for `aw` agent sessions.
//
// Zero-dependency Node (stdlib only). Wraps `aw dash json` + tmux so a phone
// on the same Wi-Fi can watch every agent session, see when one is waiting for
// input, read its terminal, and type back / approve permission prompts.
//
//   node aw-remote.mjs            # binds 0.0.0.0:8787, prints a phone URL
//
// Security model (LAN-first): bind to the LAN, gate every request behind a
// bearer token, validate pane targets against the live session list, and never
// build a shell string from user input (execFile with arg arrays only).
//
// Env:
//   AW_REMOTE_PORT   listen port            (default 8787)
//   AW_REMOTE_HOST   listen host            (default 0.0.0.0)
//   AW_BIN           path to the aw binary  (default: ~/.local/bin/aw or `aw`)
//   AW_REMOTE_TOKEN  fixed token            (default: generated + cached)

import { createServer } from 'node:http';
import { execFile, spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { homedir, networkInterfaces } from 'node:os';
import { join } from 'node:path';
import { deflateSync } from 'node:zlib';

const HOME = homedir();
const PORT = parseInt(process.env.AW_REMOTE_PORT || '8787', 10);
const HOST = process.env.AW_REMOTE_HOST || '0.0.0.0';
const AW_BIN = process.env.AW_BIN
  || (existsSync(join(HOME, '.local/bin/aw')) ? join(HOME, '.local/bin/aw') : 'aw');
const TMUX = 'tmux';

// ---- token ----------------------------------------------------------------
const TOKEN_FILE = join(HOME, '.cache/aw/remote-token');
function loadToken() {
  if (process.env.AW_REMOTE_TOKEN) return process.env.AW_REMOTE_TOKEN;
  try { return readFileSync(TOKEN_FILE, 'utf8').trim(); } catch {}
  const t = randomBytes(16).toString('base64url');
  try { mkdirSync(join(HOME, '.cache/aw'), { recursive: true }); writeFileSync(TOKEN_FILE, t, { mode: 0o600 }); } catch {}
  return t;
}
const TOKEN = loadToken();

// ---- small exec helpers ----------------------------------------------------
function run(cmd, args, timeout = 5000) {
  return new Promise((resolve, reject) => {
    execFile(cmd, args, { timeout, maxBuffer: 4 * 1024 * 1024 }, (err, stdout, stderr) => {
      if (err) return reject(new Error(stderr?.toString() || err.message));
      resolve(stdout.toString());
    });
  });
}
// like run() but pipes `input` to the child's stdin (for `tmux load-buffer -`)
function runStdin(cmd, args, input) {
  return new Promise((resolve, reject) => {
    const p = spawn(cmd, args);
    let err = '';
    p.stderr.on('data', (d) => { err += d; });
    p.on('error', reject);
    p.on('close', (c) => (c === 0 ? resolve() : reject(new Error(err || `exit ${c}`))));
    p.stdin.on('error', () => {});
    p.stdin.end(input);
  });
}

// ---- aw / tmux wrappers ---------------------------------------------------
async function awState() {
  const raw = await run(AW_BIN, ['dash', 'json']);
  let list;
  try { list = JSON.parse(raw); } catch { list = []; }
  return list.map((p) => ({
    ...p,
    needsAttention: p.status === 'waiting',
    ageSec: Math.max(0, Math.floor(Date.now() / 1000) - (p.last_activity || 0)),
  })).sort((a, b) => {
    // waiting first, then most recently active
    if (a.needsAttention !== b.needsAttention) return a.needsAttention ? -1 : 1;
    return (b.last_activity || 0) - (a.last_activity || 0);
  });
}

// only pane ids that exist in the current snapshot may be targeted
// cached ~2s so fast screen polling doesn't spawn `aw dash json` every request
let _panes = { t: 0, set: new Set() };
async function knownPanes() {
  const now = Date.now();
  if (now - _panes.t < 2000) return _panes.set;
  const s = await awState();
  _panes = { t: now, set: new Set(s.map((p) => p.pane_id)) };
  return _panes.set;
}

// ---- fit a window to the phone viewport (shared with the Mac client, so we
// remember the original and restore it on unfit) -----------------------------
const _winOrig = new Map(); // window_id -> original window-size value ('' = was unset)
async function windowForPane(pane) {
  return (await run(TMUX, ['display-message', '-p', '-t', pane, '#{window_id}'])).trim();
}
async function fitWindow(pane, cols, rows) {
  const win = await windowForPane(pane);
  if (!_winOrig.has(win)) {
    let orig = '';
    try { orig = (await run(TMUX, ['show-options', '-w', '-t', win, '-v', 'window-size'])).trim(); } catch {}
    _winOrig.set(win, orig);
  }
  const c = Math.max(20, Math.min((cols | 0) || 80, 500));
  const r = Math.max(8, Math.min((rows | 0) || 24, 200));
  await run(TMUX, ['set-option', '-w', '-t', win, 'window-size', 'manual']);
  await run(TMUX, ['resize-window', '-t', win, '-x', String(c), '-y', String(r)]);
  return { cols: c, rows: r };
}
async function unfitWindow(pane) {
  const win = await windowForPane(pane);
  const orig = _winOrig.get(win);
  if (orig === undefined) return;
  if (orig === '') await run(TMUX, ['set-option', '-w', '-u', '-t', win, 'window-size']);
  else await run(TMUX, ['set-option', '-w', '-t', win, 'window-size', orig]);
  _winOrig.delete(win);
}

async function captureScreen(pane, lines) {
  const n = Math.min(Math.max(parseInt(lines || '60', 10) || 60, 10), 400);
  // -p print to stdout, -e keep ANSI escape sequences (colors), -S -n scrollback
  return run(TMUX, ['capture-pane', '-t', pane, '-p', '-e', '-S', `-${n}`]);
}

// allowlisted named keys (everything else must go through literal text)
const KEY_ALLOW = new Set([
  'Enter', 'Escape', 'Up', 'Down', 'Left', 'Right', 'Tab', 'BSpace',
  'C-c', 'C-d', 'C-u', 'Space', 'y', 'n', '1', '2', '3', '4',
]);

async function sendKeys(pane, { text, key, submit, paste }) {
  if (paste != null && paste !== '') {
    // bracketed paste via a private buffer: multi-line / markdown arrives as one
    // paste (no premature submit, no shell involvement)
    await runStdin(TMUX, ['load-buffer', '-b', 'aw-remote', '-'], String(paste));
    await run(TMUX, ['paste-buffer', '-d', '-p', '-b', 'aw-remote', '-t', pane]);
  }
  if (text != null && text !== '') {
    await run(TMUX, ['send-keys', '-t', pane, '-l', '--', String(text)]);
  }
  if (key) {
    if (!KEY_ALLOW.has(key)) throw new Error(`key not allowed: ${key}`);
    await run(TMUX, ['send-keys', '-t', pane, key]);
  }
  if (submit) {
    await run(TMUX, ['send-keys', '-t', pane, 'Enter']);
  }
}

// ---- app icon (generated PNG: terminal "›_" on a dark gradient) -----------
const CRC = (() => { const t = []; for (let n = 0; n < 256; n++) { let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; } return t; })();
function crc32(buf) { let c = 0xffffffff; for (let i = 0; i < buf.length; i++) c = CRC[(c ^ buf[i]) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; }
function pngChunk(type, data) {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
  const t = Buffer.from(type);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(Buffer.concat([t, data])));
  return Buffer.concat([len, t, data, crc]);
}
function buildIcon(size) {
  const rgb = Buffer.alloc(size * size * 3);
  const set = (x, y, r, g, b) => { if (x < 0 || y < 0 || x >= size || y >= size) return;
    const i = (y * size + x) * 3; rgb[i] = r; rgb[i + 1] = g; rgb[i + 2] = b; };
  const lerp = (a, b, t) => Math.round(a + (b - a) * t);
  // dark vertical gradient #141a24 -> #0b0e14
  for (let y = 0; y < size; y++) { const t = y / size;
    for (let x = 0; x < size; x++) set(x, y, lerp(0x14, 0x0b, t), lerp(0x1a, 0x0e, t), lerp(0x24, 0x14, t)); }
  // distance from a point to segment AB
  const dseg = (px, py, ax, ay, bx, by) => { const dx = bx - ax, dy = by - ay; const l2 = dx * dx + dy * dy || 1;
    let t = ((px - ax) * dx + (py - ay) * dy) / l2; t = Math.max(0, Math.min(1, t));
    return Math.hypot(px - (ax + t * dx), py - (ay + t * dy)); };
  const s = size / 180;                                  // design at 180, scale up
  const A = [58 * s, 52 * s], M = [112 * s, 90 * s], B = [58 * s, 128 * s];
  const stroke = 11 * s, G = [0x3f, 0xb9, 0x50];         // green ">"
  const cx0 = 120 * s, cx1 = 152 * s, cy0 = 113 * s, cy1 = 124 * s; // "_" cursor block
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
    const d = Math.min(dseg(x, y, A[0], A[1], M[0], M[1]), dseg(x, y, M[0], M[1], B[0], B[1]));
    if (d <= stroke || (x >= cx0 && x <= cx1 && y >= cy0 && y <= cy1)) set(x, y, G[0], G[1], G[2]);
  }
  // encode (color type 2, RGB; filter byte 0 per row)
  const raw = Buffer.alloc(size * (1 + size * 3));
  for (let y = 0; y < size; y++) { raw[y * (1 + size * 3)] = 0;
    rgb.copy(raw, y * (1 + size * 3) + 1, y * size * 3, (y + 1) * size * 3); }
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(size, 0); ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; ihdr[9] = 2;
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  return Buffer.concat([sig, pngChunk('IHDR', ihdr), pngChunk('IDAT', deflateSync(raw)), pngChunk('IEND', Buffer.alloc(0))]);
}
const ICONS = { 180: buildIcon(180), 512: buildIcon(512) };

// Nerd Font (Meslo Mono) served to the phone so UI glyphs + terminal powerline
// symbols render. Falls back through a few common filenames.
let FONT = null, FONT_PATH = null;
for (const p of [process.env.AW_FONT,
  join(HOME, 'Library/Fonts/MesloLGSNerdFontMono-Regular.ttf'),
  join(HOME, 'Library/Fonts/MesloLGSNerdFont-Regular.ttf'),
  join(HOME, 'Library/Fonts/FiraCodeNerdFontMono-Regular.ttf')].filter(Boolean)) {
  try { FONT = readFileSync(p); FONT_PATH = p; break; } catch {}
}
const MANIFEST = JSON.stringify({
  name: 'aw sessions', short_name: 'aw', display: 'standalone',
  background_color: '#0b0e14', theme_color: '#0b0e14', start_url: '.',
  icons: [{ src: 'icon-512.png', sizes: '512x512', type: 'image/png' }],
});

// ---- http -----------------------------------------------------------------
function authed(req, url) {
  const hdr = req.headers['authorization'];
  if (hdr === `Bearer ${TOKEN}`) return true;
  if (url.searchParams.get('t') === TOKEN) return true;
  const cookie = req.headers.cookie || '';
  if (cookie.split(/;\s*/).some((c) => c === `aw_token=${TOKEN}`)) return true;
  return false;
}

function json(res, code, body) {
  const s = JSON.stringify(body);
  res.writeHead(code, { 'content-type': 'application/json', 'cache-control': 'no-store' });
  res.end(s);
}

async function readBody(req) {
  const chunks = [];
  for await (const c of req) chunks.push(c);
  if (!chunks.length) return {};
  try { return JSON.parse(Buffer.concat(chunks).toString()); } catch { return {}; }
}

const server = createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);
  const { pathname } = url;

  // serve the app shell (auth via ?t= sets a cookie so later calls are clean)
  if (pathname === '/' && req.method === 'GET') {
    if (!authed(req, url)) { res.writeHead(401, { 'content-type': 'text/plain' }); return res.end('unauthorized — append ?t=YOUR_TOKEN'); }
    res.writeHead(200, {
      'content-type': 'text/html; charset=utf-8',
      'set-cookie': `aw_token=${TOKEN}; Path=/; Max-Age=31536000; SameSite=Lax`,
    });
    return res.end(APP_HTML);
  }

  // public, non-sensitive assets (icons fetched at add-to-home-screen time)
  if (pathname === '/icon-180.png' || pathname === '/icon-512.png') {
    res.writeHead(200, { 'content-type': 'image/png', 'cache-control': 'max-age=86400' });
    return res.end(ICONS[pathname === '/icon-512.png' ? 512 : 180]);
  }
  if (pathname === '/manifest.webmanifest') {
    res.writeHead(200, { 'content-type': 'application/manifest+json', 'cache-control': 'max-age=86400' });
    return res.end(MANIFEST);
  }
  if (pathname === '/font.ttf') {
    if (!FONT) return json(res, 404, { error: 'no font' });
    res.writeHead(200, { 'content-type': 'font/ttf', 'cache-control': 'max-age=604800' });
    return res.end(FONT);
  }

  // everything else requires auth
  if (!authed(req, url)) return json(res, 401, { error: 'unauthorized' });

  try {
    if (pathname === '/api/state' && req.method === 'GET') {
      return json(res, 200, { sessions: await awState() });
    }

    if (pathname === '/api/screen' && req.method === 'GET') {
      const pane = url.searchParams.get('pane');
      if (!(await knownPanes()).has(pane)) return json(res, 404, { error: 'unknown pane' });
      const screen = await captureScreen(pane, url.searchParams.get('lines'));
      return json(res, 200, { pane, screen });
    }

    // event-driven screen: tail one pane, push only when its content changes
    if (pathname === '/api/screen-stream' && req.method === 'GET') {
      const pane = url.searchParams.get('pane');
      if (!(await knownPanes()).has(pane)) return json(res, 404, { error: 'unknown pane' });
      const lines = url.searchParams.get('lines');
      res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store', connection: 'keep-alive' });
      let last = null, busy = false;
      const tick = async () => {
        if (busy) return; busy = true;
        try {
          const screen = await captureScreen(pane, lines);
          if (screen !== last) { last = screen; res.write(`data: ${JSON.stringify(screen)}\n\n`); }
        } catch {} finally { busy = false; }
      };
      await tick();
      const iv = setInterval(tick, 150);
      const ping = setInterval(() => res.write(': ping\n\n'), 25000);
      req.on('close', () => { clearInterval(iv); clearInterval(ping); });
      return;
    }

    if (pathname === '/api/keys' && req.method === 'POST') {
      const body = await readBody(req);
      const pane = body.pane;
      if (!(await knownPanes()).has(pane)) return json(res, 404, { error: 'unknown pane' });
      await sendKeys(pane, body);
      return json(res, 200, { ok: true });
    }

    if (pathname === '/api/resize' && req.method === 'POST') {
      const body = await readBody(req);
      if (!(await knownPanes()).has(body.pane)) return json(res, 404, { error: 'unknown pane' });
      const out = await fitWindow(body.pane, body.cols, body.rows);
      return json(res, 200, { ok: true, ...out });
    }

    if (pathname === '/api/unfit' && req.method === 'POST') {
      const body = await readBody(req);
      if (!body.pane) return json(res, 400, { error: 'pane required' });
      try { await unfitWindow(body.pane); } catch {} // pane may already be gone
      return json(res, 200, { ok: true });
    }

    if (pathname === '/api/upload' && req.method === 'POST') {
      const ct = req.headers['content-type'] || '';
      const ext = ct.includes('png') ? 'png' : (ct.includes('jpeg') || ct.includes('jpg')) ? 'jpg'
        : ct.includes('webp') ? 'webp' : ct.includes('heic') ? 'heic' : 'bin';
      const chunks = []; let size = 0;
      for await (const c of req) {
        size += c.length;
        if (size > 25 * 1024 * 1024) return json(res, 413, { error: 'too large' });
        chunks.push(c);
      }
      const dir = join(HOME, '.cache/aw/uploads');
      mkdirSync(dir, { recursive: true });
      const name = `shot-${Date.now()}.${ext}`;
      const p = join(dir, name);
      writeFileSync(p, Buffer.concat(chunks));
      console.log(`  📷 upload: ${p}`);
      return json(res, 200, { ok: true, name, path: p });
    }

    if (pathname === '/api/events' && req.method === 'GET') {
      res.writeHead(200, {
        'content-type': 'text/event-stream',
        'cache-control': 'no-store',
        connection: 'keep-alive',
      });
      let last = '';
      const tick = async () => {
        try {
          const sessions = await awState();
          const s = JSON.stringify(sessions);
          if (s !== last) { last = s; res.write(`data: ${s}\n\n`); }
        } catch {}
      };
      await tick();
      const iv = setInterval(tick, 1500);
      const ping = setInterval(() => res.write(': ping\n\n'), 25000);
      req.on('close', () => { clearInterval(iv); clearInterval(ping); });
      return;
    }

    json(res, 404, { error: 'not found' });
  } catch (e) {
    json(res, 500, { error: String(e.message || e) });
  }
});

server.listen(PORT, HOST, () => {
  const ips = [];
  const ifs = networkInterfaces();
  for (const name of Object.keys(ifs)) {
    for (const ni of ifs[name] || []) {
      if (ni.family === 'IPv4' && !ni.internal) ips.push(ni.address);
    }
  }
  const lan = ips[0] || 'localhost';
  console.log('\n  aw-remote listening');
  console.log(`  token: ${TOKEN}`);
  console.log('\n  Open on your phone (same Wi-Fi):');
  console.log(`    http://${lan}:${PORT}/?t=${TOKEN}`);
  for (const ip of ips.slice(1)) console.log(`    http://${ip}:${PORT}/?t=${TOKEN}`);
  console.log('\n  Add to Home Screen for an app-like experience.\n');
});

// ---- the mobile PWA (inlined so the server is a single file) --------------
const APP_HTML = /* html */ `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content">
<meta name="apple-mobile-web-app-capable" content="yes">
<meta name="apple-mobile-web-app-status-bar-style" content="black-translucent">
<meta name="theme-color" content="#0b0e14">
<meta name="apple-mobile-web-app-title" content="aw">
<link rel="apple-touch-icon" sizes="180x180" href="/icon-180.png">
<link rel="icon" type="image/png" sizes="512x512" href="/icon-512.png">
<link rel="manifest" href="/manifest.webmanifest">
<title>aw remote</title>
<style>
  :root { --bg:#0b0e14; --panel:#141a24; --panel2:#1b2330; --fg:#e6edf3; --dim:#8b98a9;
          --green:#3fb950; --amber:#d29922; --red:#f85149; --grey:#6e7681; --accent:#388bfd; }
  @font-face { font-family:'nf'; src:url(/font.ttf) format('truetype'); font-display:swap; }
  .icon { font-family:'nf'; font-style:normal; font-weight:normal; line-height:1; vertical-align:middle; }
  * { box-sizing:border-box; -webkit-tap-highlight-color:transparent; }
  html,body { margin:0; background:var(--bg); color:var(--fg);
    font:15px/1.4 -apple-system,system-ui,sans-serif; }
  body { padding:0 env(safe-area-inset-right) 0 env(safe-area-inset-left); }
  header { position:sticky; top:0; background:rgba(11,14,20,.92); backdrop-filter:blur(8px);
    padding:calc(12px + env(safe-area-inset-top)) 16px 12px; display:flex; align-items:center;
    gap:10px; border-bottom:1px solid #222b38; z-index:5; }
  header h1 { font-size:16px; margin:0; flex:1; font-weight:600; }
  .dot { width:9px; height:9px; border-radius:50%; flex:0 0 auto; }
  .working { background:var(--green); } .waiting { background:var(--amber); box-shadow:0 0 0 4px rgba(210,153,34,.22); }
  .idle { background:var(--grey); }
  .list { padding:8px 12px 40px; }
  .card { background:var(--panel); border:1px solid #222b38; border-radius:14px; padding:13px 14px;
    margin:8px 0; display:flex; gap:11px; align-items:flex-start; }
  .card:active { background:var(--panel2); }
  .card.attn { border-color:var(--amber); }
  .meta { flex:1; min-width:0; }
  .name { font-weight:600; display:flex; align-items:center; gap:7px; }
  .badge { font-size:11px; padding:1px 7px; border-radius:20px; background:#222b38; color:var(--dim); }
  .badge.attn { background:rgba(210,153,34,.18); color:var(--amber); }
  .sub { color:var(--dim); font-size:12.5px; margin-top:3px; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }
  .prompt { color:var(--fg); font-size:13px; margin-top:6px; opacity:.85;
    display:-webkit-box; -webkit-line-clamp:2; -webkit-box-orient:vertical; overflow:hidden; }
  .empty { color:var(--dim); text-align:center; padding:60px 20px; }
  /* detail */
  .sheet { position:fixed; inset:0; background:var(--bg); display:flex; flex-direction:column;
    transform:translateX(100%); transition:transform .22s ease; z-index:10; }
  .sheet.open { transform:none; }
  .sheet header .back { background:none; border:none; color:var(--accent); font-size:16px; padding:4px 8px 4px 0; }
  .term { flex:1; min-height:0; overflow:auto; margin:0; padding:12px 14px; background:#05070a;
    font:12px/1.35 'nf',ui-monospace,SFMono-Regular,Menlo,monospace; color:#cdd6e0; white-space:pre;
    -webkit-overflow-scrolling:touch; overscroll-behavior:none; }
  .bar { border-top:1px solid #222b38; background:var(--panel); padding:8px 12px calc(8px + env(safe-area-inset-bottom)); }
  .keys { display:flex; gap:7px; flex-wrap:nowrap; overflow-x:auto; margin:0;
    -webkit-overflow-scrolling:touch; scrollbar-width:none; }
  .keys::-webkit-scrollbar { display:none; }
  .keys button { flex:0 0 auto; background:var(--panel2); color:var(--fg); border:1px solid #2a3442;
    border-radius:9px; padding:9px 13px; font-size:14px; min-width:42px; }
  .keys button:active { background:#2a3442; }
  .keys button.yes { color:var(--green); } .keys button.no { color:var(--red); }
  .keys button.on { color:var(--accent); border-color:var(--accent); background:rgba(56,139,253,.15); }
  .keys button.icon { font-size:18px; padding-top:7px; padding-bottom:7px; }
  header .icon { font-size:20px; }
  .back .icon { font-size:19px; margin-right:1px; }
  /* full-screen draft editor (overlay above the session sheet) */
  .editor { position:fixed; inset:0; background:var(--bg); display:flex; flex-direction:column;
    transform:translateY(100%); transition:transform .22s ease; z-index:20; }
  .editor.open { transform:none; }
  .editor header h1 { font-size:15px; }
  #edSend { background:var(--accent); color:#fff; border:none; border-radius:9px; padding:7px 14px; font-size:20px; }
  #draft { flex:1; min-height:0; width:100%; border:0; outline:none; resize:none;
    background:var(--bg); color:var(--fg); padding:14px 16px;
    font:16px/1.6 'nf',ui-monospace,SFMono-Regular,Menlo,monospace;  /* >=16px: no iOS focus-zoom */
    -webkit-overflow-scrolling:touch; overscroll-behavior:none; }
  #draft::placeholder { color:var(--dim); }
  /* invisible keystroke catcher: a focusable element is required to raise the
     mobile keyboard, but we don't want a visible box. font-size:16px stops iOS
     from zooming on focus. */
  #kbd { position:absolute; height:1px; width:1px; opacity:0; padding:0; margin:0;
    border:0; font-size:16px; caret-color:transparent; overflow:hidden; white-space:nowrap;
    outline:none; -webkit-user-modify:read-write-plaintext-only; }
  .term.live { box-shadow:inset 0 0 0 2px rgba(56,139,253,.45); }
  .toast { position:fixed; bottom:calc(20px + env(safe-area-inset-bottom)); left:50%; transform:translateX(-50%);
    background:#222b38; color:var(--fg); padding:9px 16px; border-radius:20px; font-size:13px; opacity:0;
    transition:opacity .2s; z-index:30; pointer-events:none; }
  .toast.show { opacity:1; }
</style>
</head>
<body>
<header>
  <span id="hdrdot" class="dot idle"></span>
  <h1>aw sessions</h1>
  <button id="bell" class="icon" title="alerts" style="background:none;border:none;color:var(--dim)">&#xF009A;</button>
</header>
<div id="list" class="list"><div class="empty">connecting…</div></div>

<div id="sheet" class="sheet">
  <header>
    <button class="back" onclick="closeSheet()"><span class="icon">&#xF0141;</span>Back</button>
    <span id="dDot" class="dot idle"></span>
    <h1 id="dTitle">session</h1>
  </header>
  <pre id="term" class="term"></pre>
  <div class="bar">
    <div class="keys">
      <button id="modeBtn" class="icon" title="draft editor">&#xF03EB;</button>
      <button id="kbBtn" class="icon" title="keyboard">&#xF030C;</button>
      <button id="fitBtn" class="icon" title="fit window to my screen">&#xF1A8E;</button>
      <button id="shotBtn" class="icon" title="screenshot">&#xF0100;</button>
      <button class="yes" data-key="1">1</button>
      <button class="no" data-key="2">2</button>
      <button data-key="3">3</button>
      <button class="icon" data-key="Up">&#xF005D;</button>
      <button class="icon" data-key="Down">&#xF0045;</button>
      <button class="icon" data-key="BSpace">&#xF006B;</button>
      <button class="icon" data-key="Enter">&#xF0311;</button>
      <button data-key="Escape">esc</button>
      <button data-key="C-c">^C</button>
    </div>
    <div id="kbd" contenteditable="true" role="textbox" aria-label="terminal input"
      autocapitalize="off" autocorrect="off" spellcheck="false"></div>
    <input id="shotFile" type="file" accept="image/*" style="display:none">
  </div>
</div>

<div id="editor" class="editor">
  <header>
    <button class="back" onclick="closeEditor()"><span class="icon">&#xF0156;</span></button>
    <h1 id="edTitle">Draft</h1>
    <button id="edSend" class="icon" title="send to session">&#xF048A;</button>
  </header>
  <textarea id="draft" placeholder="Compose your message, then send — saved locally as a draft."
    autocapitalize="off" autocorrect="off" spellcheck="false"></textarea>
</div>
<div id="toast" class="toast"></div>

<script>
const $ = (s) => document.querySelector(s);
let sessions = [], current = null, lastAttn = new Set();
let lastScreen = '', scrInFlight = false;

function toast(t){ const e=$('#toast'); e.textContent=t; e.classList.add('show'); setTimeout(()=>e.classList.remove('show'),1400); }
function rel(s){ if(s<60)return s+'s'; if(s<3600)return Math.floor(s/60)+'m'; return Math.floor(s/3600)+'h'; }

function render(){
  const list=$('#list');
  if(!sessions.length){ list.innerHTML='<div class="empty">no active agent sessions</div>'; $('#hdrdot').className='dot idle'; return; }
  const anyAttn = sessions.some(s=>s.needsAttention);
  $('#hdrdot').className = 'dot ' + (anyAttn?'waiting':'working');
  list.innerHTML = sessions.map(s=>\`
    <div class="card \${s.needsAttention?'attn':''}" onclick="openSheet('\${s.pane_id}')">
      <span class="dot \${s.status}"></span>
      <div class="meta">
        <div class="name">\${esc(s.workspace||s.pane_id)}
          <span class="badge \${s.needsAttention?'attn':''}">\${s.needsAttention?'needs you':s.status}</span></div>
        <div class="sub">\${esc(s.agent)} · \${esc(s.last_event||'')} · \${rel(s.ageSec)} ago</div>
        <div class="prompt">\${esc(s.last_prompt||'')}</div>
      </div>
    </div>\`).join('');
  // keep an open sheet's header fresh as state streams in (incl. deep-link)
  if(current){ const s=sessions.find(x=>x.pane_id===current);
    if(s){ $('#dTitle').textContent=s.workspace||current; $('#dDot').className='dot '+s.status; } }
  // attention notifications
  const now=new Set(sessions.filter(s=>s.needsAttention).map(s=>s.pane_id));
  for(const id of now){ if(!lastAttn.has(id)){ const s=sessions.find(x=>x.pane_id===id); notify(s); } }
  lastAttn=now;
}
function esc(s){ return String(s??'').replace(/[&<>]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;'}[c])); }

// ---- ANSI -> HTML (16/256/truecolor + bold/dim/italic/underline/inverse) ----
const ANSI16=['#0b0e14','#f85149','#3fb950','#d29922','#388bfd','#bc8cff','#39c5cf','#b1bac4',
              '#6e7681','#ff7b72','#56d364','#e3b341','#79c0ff','#d2a8ff','#56d4dd','#ffffff'];
function xterm256(n){
  if(n<16) return ANSI16[n];
  if(n<232){ n-=16; const f=v=>[0,95,135,175,215,255][v];
    return '#'+[f(Math.floor(n/36)),f(Math.floor(n/6)%6),f(n%6)].map(x=>x.toString(16).padStart(2,'0')).join(''); }
  const v=(8+(n-232)*10).toString(16).padStart(2,'0'); return '#'+v+v+v;
}
function ansiToHtml(input){
  // strip non-SGR escapes so they don't render as garbage: OSC strings
  // (titles/hyperlinks), DCS/APC/PM/SOS, non-SGR CSI, and stray string-terminators
  input=String(input)
    .replace(/\\x1b\\][\\s\\S]*?(?:\\x07|\\x1b\\\\)/g,'')         // OSC ... (BEL or ST)
    .replace(/\\x1b[P_^X][\\s\\S]*?\\x1b\\\\/g,'')               // DCS/APC/PM/SOS ... ST
    .replace(/\\x1b\\[[0-9;?<>=!]*[\\x40-\\x6c\\x6e-\\x7e]/g,'') // CSI except SGR ('m')
    .replace(/\\x1b\\\\/g,'');                                  // leftover ST
  let fg=null,bg=null,bold=false,dim=false,ital=false,under=false,inv=false,out='',open=false;
  const close=()=>{ if(open){ out+='</span>'; open=false; } };
  const span=()=>{ close(); let f=fg,b=bg; if(inv){ f=bg||'#0b0e14'; b=fg||'#cdd6e0'; }
    const st=[]; if(f)st.push('color:'+f); if(b)st.push('background:'+b);
    if(bold)st.push('font-weight:700'); if(dim)st.push('opacity:.6');
    if(ital)st.push('font-style:italic'); if(under)st.push('text-decoration:underline');
    if(st.length){ out+='<span style="'+st.join(';')+'">'; open=true; } };
  let dirty=false; const re=/\\x1b\\[([0-9;]*)m/g; let last=0,m;
  const emit=t=>{ if(t){ if(dirty){ span(); dirty=false; } out+=esc(t); } };
  while((m=re.exec(input))){
    emit(input.slice(last,m.index)); last=re.lastIndex;
    const codes=(m[1]===''?'0':m[1]).split(';').map(Number);
    for(let i=0;i<codes.length;i++){ const c=codes[i];
      if(c===0){ fg=bg=null; bold=dim=ital=under=inv=false; }
      else if(c===1)bold=true; else if(c===2)dim=true; else if(c===3)ital=true;
      else if(c===4)under=true; else if(c===7)inv=true;
      else if(c===22){bold=dim=false;} else if(c===23)ital=false; else if(c===24)under=false; else if(c===27)inv=false;
      else if(c>=30&&c<=37)fg=ANSI16[c-30]; else if(c>=90&&c<=97)fg=ANSI16[c-90+8];
      else if(c>=40&&c<=47)bg=ANSI16[c-40]; else if(c>=100&&c<=107)bg=ANSI16[c-100+8];
      else if(c===39)fg=null; else if(c===49)bg=null;
      else if(c===38||c===48){ const set=v=>c===38?fg=v:bg=v;
        if(codes[i+1]===5){ set(xterm256(codes[i+2]||0)); i+=2; }
        else if(codes[i+1]===2){ set('rgb('+(codes[i+2]||0)+','+(codes[i+3]||0)+','+(codes[i+4]||0)+')'); i+=4; } }
    }
    dirty=true;
  }
  emit(input.slice(last)); close(); return out;
}

function notify(s){
  if(!s) return;
  if('Notification'in window && Notification.permission==='granted'){
    new Notification('Agent waiting', { body:(s.workspace||s.pane_id)+' · '+s.agent });
  }
}

// ---- detail sheet (state lives in browser history: back button + refresh) ----
function showSheet(pane){            // UI only — no history side effects
  current=pane; const s=sessions.find(x=>x.pane_id===pane);
  $('#dTitle').textContent=s?(s.workspace||pane):pane;
  $('#dDot').className='dot '+(s?s.status:'idle');
  $('#sheet').classList.add('open');
  lastScreen=''; refreshScreen(); openScreenStream(pane);
  if(fitMode){ lastFit=''; setTimeout(applyFit,150); }   // fit the newly-opened session
}
function hideSheet(){ try{saveDraftNow();}catch{} if(fitMode&&current) unfit(current); $('#sheet').classList.remove('open'); current=null; closeScreenStream(); try{kbd.blur();}catch{} try{draft.blur();}catch{} }
function openSheet(pane){            // from a list tap: push a history entry
  history.pushState({pane}, '', '#'+encodeURIComponent(pane)); showSheet(pane);
}
function closeSheet(){ history.back(); }   // Back button -> popstate -> hideSheet
window.addEventListener('popstate', e=>{ const st=e.state||{};
  if(st.editor){ showEditor(); return; }            // forward into editor
  hideEditor();                                      // ensure editor closed otherwise
  if(st.pane) showSheet(st.pane); else hideSheet(); });
function applyScreen(screen){
  if(screen===lastScreen) return;          // unchanged -> no DOM work, no scroll jump
  lastScreen=screen;
  const t=$('#term'); const atBottom=t.scrollHeight-t.scrollTop-t.clientHeight<40;
  t.innerHTML=ansiToHtml(screen); if(atBottom) t.scrollTop=t.scrollHeight;
}
// live screen over SSE: server pushes only when the pane content changes
let screenES=null;
function openScreenStream(pane){
  closeScreenStream();
  screenES=new EventSource('/api/screen-stream?pane='+encodeURIComponent(pane)+'&lines=80');
  screenES.onmessage=e=>{ try{ applyScreen(JSON.parse(e.data)); }catch{} };
  // EventSource auto-reconnects on transient errors
}
function closeScreenStream(){ if(screenES){ screenES.close(); screenES=null; } }
// one-shot fetch for instant echo right after a local keystroke
async function refreshScreen(){
  if(!current||scrInFlight) return;
  scrInFlight=true;
  try{ const r=await fetch('/api/screen?pane='+encodeURIComponent(current)+'&lines=80');
    const j=await r.json(); applyScreen(j.screen||''); }
  catch{} finally{ scrInFlight=false; }
}
let rt; function liveRefresh(){ clearTimeout(rt); rt=setTimeout(refreshScreen,150); }
async function key(k){ if(!current)return; await post({pane:current,key:k}); liveRefresh(); }
async function post(body){
  try{ const r=await fetch('/api/keys',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)});
    if(!r.ok) toast('failed'); }catch{ toast('offline'); }
}
document.querySelectorAll('.keys button[data-key]').forEach(b=>b.onclick=()=>key(b.dataset.key));

// ---- live terminal typing: forward keystrokes straight to the pane ----
const kbd=$('#kbd');
const SPECIAL={Enter:'Enter',Backspace:'BSpace',Escape:'Escape',Tab:'Tab',
  ArrowUp:'Up',ArrowDown:'Down',ArrowLeft:'Left',ArrowRight:'Right'};
kbd.addEventListener('keydown',e=>{
  if(e.isComposing) return;                 // let IME finish
  const k=SPECIAL[e.key];
  if(k){ e.preventDefault(); key(k); }      // named keys never become local text
});
kbd.addEventListener('input',()=>{          // printable text, paste, autocomplete, IME commit
  const v=kbd.textContent; if(!v||!current){ kbd.textContent=''; return; }
  kbd.textContent=''; post({pane:current,text:v}); liveRefresh();
});
// raise/dismiss the keyboard via the ⌨ button (real toggle) or by tapping the terminal.
// track open state ourselves: tapping the button would blur the field before the
// click fires, so we suppress that blur and toggle off our own flag instead.
let kbOpen=false;
$('#kbBtn').addEventListener('pointerdown',e=>{ if(kbOpen) e.preventDefault(); }); // keep focus so we can blur
$('#kbBtn').addEventListener('click',()=>{ if(!current) return; if(kbOpen) kbd.blur(); else kbd.focus(); });
$('#term').addEventListener('click',()=>{ if(current) kbd.focus(); });
kbd.addEventListener('focus',()=>{ kbOpen=true; $('#term').classList.add('live'); $('#kbBtn').classList.add('on'); setTimeout(fitViewport,60); });
kbd.addEventListener('blur',()=>{ kbOpen=false; $('#term').classList.remove('live'); $('#kbBtn').classList.remove('on'); setTimeout(fitViewport,60); });

// ---- full-screen draft editor: IME-friendly local editing, per-session draft ----
const draft=$('#draft'), editor=$('#editor'), modeBtn=$('#modeBtn'), LS=window.localStorage;
function wsKey(){ const s=sessions.find(x=>x.pane_id===current); return 'aw:draft:'+(s?s.workspace:current); }
function loadDraft(){ if(current) draft.value=LS.getItem(wsKey())||''; }
function saveDraftNow(){ if(current) try{ LS.setItem(wsKey(), draft.value); }catch{} }
let dft; function saveDraft(){ clearTimeout(dft); dft=setTimeout(saveDraftNow,250); }
let editorOpen=false;
function showEditor(){
  const s=sessions.find(x=>x.pane_id===current);
  $('#edTitle').textContent='Draft → '+(s?s.workspace:current);
  loadDraft(); editor.classList.add('open'); editorOpen=true;
  setTimeout(()=>{ draft.focus(); fitViewport(); }, 60);
}
function hideEditor(){ if(!editorOpen) return; saveDraftNow(); try{draft.blur();}catch{} editor.classList.remove('open'); editorOpen=false; }
function openEditor(fromPop){ if(!current) return;
  if(!fromPop) history.pushState({pane:current,editor:1}, '', '#'+encodeURIComponent(current));
  showEditor(); }
function closeEditor(){ history.back(); }            // -> popstate -> hideEditor
modeBtn.addEventListener('click',()=>openEditor());
draft.addEventListener('input',saveDraft);
draft.addEventListener('focus',()=>{ kbOpen=true; setTimeout(fitViewport,60); });
draft.addEventListener('blur',()=>{ kbOpen=false; saveDraftNow(); setTimeout(fitViewport,60); });
async function sendDraft(){ const v=draft.value; if(!v.trim()||!current) return;
  await post({pane:current, paste:v, submit:true});  // bracketed paste + Enter
  draft.value=''; try{LS.removeItem(wsKey());}catch{} toast('sent'); liveRefresh();
  history.back(); }                                  // close the editor after sending
$('#edSend').addEventListener('click',sendDraft);

// ---- fit the tmux window to this phone (opt-in; only the open session, and
// restored when you leave it or close the app, so a window never sticks small) ----
let fitMode=LS.getItem('aw:fit')==='1', lastFit='';
$('#fitBtn').classList.toggle('on',fitMode);
function termCell(){ const t=$('#term'), cs=getComputedStyle(t);
  const probe=document.createElement('span'); probe.textContent='X'.repeat(80);
  // measure with explicit font props — getComputedStyle().font shorthand is '' in Safari
  probe.style.cssText='position:absolute;visibility:hidden;white-space:pre;font-size:'+cs.fontSize+';font-family:'+cs.fontFamily+';letter-spacing:'+cs.letterSpacing;
  t.appendChild(probe); const cw=probe.getBoundingClientRect().width/80; probe.remove();
  const lh=parseFloat(cs.lineHeight)||(parseFloat(cs.fontSize)*1.35);
  const padX=parseFloat(cs.paddingLeft)+parseFloat(cs.paddingRight);
  const padY=parseFloat(cs.paddingTop)+parseFloat(cs.paddingBottom);
  return { cols:Math.max(20,Math.floor((t.clientWidth-padX)/cw)),
           rows:Math.max(8,Math.floor((t.clientHeight-padY)/lh)) }; }
async function applyFit(){ if(!fitMode||!current) return;
  const {cols,rows}=termCell(), k=cols+'x'+rows; if(k===lastFit) return; lastFit=k;
  try{ await fetch('/api/resize',{method:'POST',headers:{'content-type':'application/json'},
    body:JSON.stringify({pane:current,cols,rows})}); liveRefresh(); }catch{} }
async function unfit(pane){ lastFit=''; if(!pane) return;
  try{ await fetch('/api/unfit',{method:'POST',headers:{'content-type':'application/json'},
    body:JSON.stringify({pane})}); }catch{} }
$('#fitBtn').addEventListener('click',()=>{ if(!current) return;
  fitMode=!fitMode; LS.setItem('aw:fit',fitMode?'1':'0'); $('#fitBtn').classList.toggle('on',fitMode);
  if(fitMode){ applyFit(); toast('fit to screen — note: also resizes on your Mac'); }
  else { unfit(current); toast('size restored'); } });
addEventListener('orientationchange',()=>{ if(fitMode&&current) setTimeout(applyFit,350); });
// restore on background/close so the Mac window is never stuck small...
addEventListener('pagehide',()=>{ if(fitMode&&current)
  navigator.sendBeacon?.('/api/unfit', new Blob([JSON.stringify({pane:current})],{type:'application/json'})); });
// ...and re-fit when you come back to the app
addEventListener('pageshow',()=>{ if(fitMode&&current){ lastFit=''; setTimeout(applyFit,200); } });
document.addEventListener('visibilitychange',()=>{ if(document.visibilityState==='visible'&&fitMode&&current){ lastFit=''; setTimeout(applyFit,200); } });

// keep the sheet pinned to the *visible* viewport so the bottom bar sits just
// above the soft keyboard (the flex terminal shrinks to fill the space above it)
function fitViewport(){
  const vv=window.visualViewport; if(!vv) return;
  for(const el of [$('#sheet'), $('#editor')]){
    el.style.top=vv.offsetTop+'px'; el.style.bottom='auto'; el.style.height=vv.height+'px';
  }
  const t=$('#term'); if(t) t.scrollTop=t.scrollHeight;
}
if(window.visualViewport){
  visualViewport.addEventListener('resize',fitViewport);
  visualViewport.addEventListener('scroll',fitViewport);
}
$('#bell').onclick=async()=>{ if('Notification'in window){ const p=await Notification.requestPermission(); toast(p==='granted'?'alerts on':'alerts blocked'); } };
// upload a screenshot, then paste its path into THIS session's prompt
$('#shotBtn').onclick=()=>{ if(current) $('#shotFile').click(); };
$('#shotFile').addEventListener('change',async(e)=>{
  const f=e.target.files[0]; e.target.value=''; if(!f||!current) return;
  toast('uploading…');
  try{
    const r=await fetch('/api/upload',{method:'POST',headers:{'content-type':f.type||'application/octet-stream'},body:f});
    const j=await r.json();
    if(!r.ok){ toast('upload failed'); return; }
    await post({pane:current,text:j.path+' '});   // type the path into Claude Code (no Enter)
    liveRefresh(); toast('pasted image path');
  }catch{ toast('offline'); }
});

// ---- live state via SSE (falls back to polling) ----
function connect(){
  const es=new EventSource('/api/events');
  es.onmessage=(e)=>{ try{ sessions=JSON.parse(e.data); render(); }catch{} };
  es.onerror=()=>{ es.close(); setTimeout(connect,2000); };
}
connect();

// refresh straight into the session named in the URL hash, with the list as
// the entry beneath it so Back returns to the list
(function bootDeepLink(){
  const h=location.hash?decodeURIComponent(location.hash.slice(1)):'';
  if(h){ history.replaceState(null,'',location.pathname+location.search); openSheet(h); }
})();
</script>
</body>
</html>`;
