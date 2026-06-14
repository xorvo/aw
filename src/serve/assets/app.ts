// aw remote — mobile PWA client for `aw serve`.
//
// TypeScript source; compiled to app.js (committed) by scripts/build-frontend.sh.
// Served by `aw serve` (src/serve/), which embeds index.html + app.js at build
// time. The logic is a 1:1 port of the proven prototype client.

interface Session {
  pane_id: string;
  session: string;
  workspace: string;
  cwd: string;
  agent: string;
  status: 'working' | 'waiting' | 'idle';
  last_event: string;
  last_activity: number;
  last_prompt: string;
  // added server-side on top of the dash snapshot:
  needsAttention: boolean;
  ageSec: number;
}

interface KeysBody {
  pane: string;
  text?: string;
  key?: string;
  submit?: boolean;
  paste?: string;
}

const $ = <T extends HTMLElement = HTMLElement>(s: string): T =>
  document.querySelector(s) as T;
let sessions: Session[] = [];
let current: string | null = null;
let lastAttn = new Set<string>();
let lastScreen = '', scrInFlight = false;

function toast(t: string){ const e=$('#toast'); e.textContent=t; e.classList.add('show'); setTimeout(()=>e.classList.remove('show'),1400); }
function rel(s: number): string{ if(s<60)return s+'s'; if(s<3600)return Math.floor(s/60)+'m'; return Math.floor(s/3600)+'h'; }

function render(){
  const list=$('#list');
  if(!sessions.length){ list.innerHTML='<div class="empty">no active agent sessions</div>'; $('#hdrdot').className='dot idle'; return; }
  const anyAttn = sessions.some(s=>s.needsAttention);
  $('#hdrdot').className = 'dot ' + (anyAttn?'waiting':'working');
  list.innerHTML = sessions.map(s=>`
    <div class="card ${s.needsAttention?'attn':''}" onclick="openSheet('${s.pane_id}')">
      <span class="dot ${s.status}"></span>
      <div class="meta">
        <div class="name">${esc(s.workspace||s.pane_id)}
          <span class="badge ${s.needsAttention?'attn':''}">${s.needsAttention?'needs you':s.status}</span></div>
        <div class="sub">${esc(s.agent)} · ${esc(s.last_event||'')} · ${rel(s.ageSec)} ago</div>
        <div class="prompt">${esc(s.last_prompt||'')}</div>
      </div>
    </div>`).join('');
  // keep an open sheet's header fresh as state streams in (incl. deep-link)
  if(current){ const s=sessions.find(x=>x.pane_id===current);
    if(s){ $('#dTitle').textContent=s.workspace||current; $('#dDot').className='dot '+s.status; } }
  // attention notifications
  const now=new Set(sessions.filter(s=>s.needsAttention).map(s=>s.pane_id));
  for(const id of now){ if(!lastAttn.has(id)){ const s=sessions.find(x=>x.pane_id===id); notify(s); } }
  lastAttn=now;
}
// Symbols that Mac terminals draw as monochrome text but iOS promotes to
// colorful emoji presentation (Claude Code's tool-call marker U+23FA "⏺",
// its ✳/✻ spinner frames, ⚠, ✅, …). Appending VS15 (U+FE0E, the
// text-presentation selector) pins them to text form. Skip characters
// already carrying a variation selector — VS16 (U+FE0F) means the
// content explicitly asked for emoji.
const EMOJI_AMBIGUOUS=/([\u2139\u21a9\u21aa\u231a\u231b\u23cf-\u23fa\u24c2\u25aa\u25ab\u25b6\u25c0\u25fb-\u25fe\u2600-\u27bf\u2934\u2935\u2b05-\u2b07\u2b1b\u2b1c\u2b50\u2b55\u3030\u303d\u3297\u3299])(?![\ufe0e\ufe0f])/g;
function esc(s: unknown): string{
  return String(s??'')
    .replace(/[&<>]/g,c=>(({'&':'&amp;','<':'&lt;','>':'&gt;'} as Record<string,string>)[c]))
    .replace(EMOJI_AMBIGUOUS,'$1︎');
}

// ---- ANSI -> HTML (16/256/truecolor + bold/dim/italic/underline/inverse) ----
const ANSI16=['#0b0e14','#f85149','#3fb950','#d29922','#388bfd','#bc8cff','#39c5cf','#b1bac4',
              '#6e7681','#ff7b72','#56d364','#e3b341','#79c0ff','#d2a8ff','#56d4dd','#ffffff'];
function xterm256(n: number): string{
  if(n<16) return ANSI16[n];
  if(n<232){ n-=16; const f=(v: number)=>[0,95,135,175,215,255][v];
    return '#'+[f(Math.floor(n/36)),f(Math.floor(n/6)%6),f(n%6)].map(x=>x.toString(16).padStart(2,'0')).join(''); }
  const v=(8+(n-232)*10).toString(16).padStart(2,'0'); return '#'+v+v+v;
}
function ansiToHtml(input: string): string{
  // strip non-SGR escapes so they don't render as garbage: OSC strings
  // (titles/hyperlinks), DCS/APC/PM/SOS, non-SGR CSI, and stray string-terminators
  input=String(input)
    .replace(/\x1b\][\s\S]*?(?:\x07|\x1b\\)/g,'')         // OSC ... (BEL or ST)
    .replace(/\x1b[P_^X][\s\S]*?\x1b\\/g,'')               // DCS/APC/PM/SOS ... ST
    .replace(/\x1b\[[0-9;?<>=!]*[\x40-\x6c\x6e-\x7e]/g,'') // CSI except SGR ('m')
    .replace(/\x1b\\/g,'');                                  // leftover ST
  let fg: string | null=null, bg: string | null=null;
  let bold=false,dim=false,ital=false,under=false,inv=false,out='',open=false;
  const close=()=>{ if(open){ out+='</span>'; open=false; } };
  const span=()=>{ close(); let f=fg,b=bg; if(inv){ f=bg||'#0b0e14'; b=fg||'#cdd6e0'; }
    const st: string[]=[]; if(f)st.push('color:'+f); if(b)st.push('background:'+b);
    if(bold)st.push('font-weight:700'); if(dim)st.push('opacity:.6');
    if(ital)st.push('font-style:italic'); if(under)st.push('text-decoration:underline');
    if(st.length){ out+='<span style="'+st.join(';')+'">'; open=true; } };
  let dirty=false; const re=/\x1b\[([0-9;]*)m/g; let last=0; let m: RegExpExecArray | null;
  const emit=(t: string)=>{ if(t){ if(dirty){ span(); dirty=false; } out+=esc(t); } };
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
      else if(c===38||c===48){ const set=(v: string)=>{ if(c===38)fg=v; else bg=v; };
        if(codes[i+1]===5){ set(xterm256(codes[i+2]||0)); i+=2; }
        else if(codes[i+1]===2){ set('rgb('+(codes[i+2]||0)+','+(codes[i+3]||0)+','+(codes[i+4]||0)+')'); i+=4; } }
    }
    dirty=true;
  }
  emit(input.slice(last)); close(); return out;
}

function notify(s: Session | undefined){
  if(!s) return;
  if('Notification'in window && Notification.permission==='granted'){
    new Notification('Agent waiting', { body:(s.workspace||s.pane_id)+' · '+s.agent });
  }
}

// ---- detail sheet (state lives in browser history: back button + refresh) ----
function showSheet(pane: string){    // UI only — no history side effects
  current=pane; const s=sessions.find(x=>x.pane_id===pane);
  $('#dTitle').textContent=s?(s.workspace||pane):pane;
  $('#dDot').className='dot '+(s?s.status:'idle');
  $('#sheet').classList.add('open');
  lastScreen=''; pendingScreen=null; refreshScreen(); openScreenStream(pane);
  if(fitMode){ lastFit=''; setTimeout(applyFit,150); }   // fit the newly-opened session
}
function hideSheet(){ try{saveDraftNow();}catch{} if(fitMode&&current) unfit(current); $('#sheet').classList.remove('open'); current=null; closeScreenStream(); try{kbd.blur();}catch{} try{draft.blur();}catch{} }
function openSheet(pane: string){    // from a list tap: push a history entry
  history.pushState({pane}, '', '#'+encodeURIComponent(pane)); showSheet(pane);
}
function closeSheet(){ history.back(); }   // Back button -> popstate -> hideSheet
window.addEventListener('popstate', e=>{ const st=(e.state||{}) as {pane?: string; editor?: number};
  if(st.editor){ showEditor(); return; }            // forward into editor
  hideEditor();                                      // ensure editor closed otherwise
  if(st.pane) showSheet(st.pane); else hideSheet(); });
// Defer DOM swaps while the finger is down / momentum is running:
// replacing a large <pre> mid-scroll makes iOS Safari composite tiles
// from two different frames (visible tearing), and content jumping
// under the reader is wrong anyway. Frames buffer (latest wins) and
// flush ~200ms after the last scroll/touch event.
let pendingScreen: string | null=null, userScrolling=false, progScroll=false;
let scrollQuiet: number | undefined;
function applyScreen(screen: string){
  if(userScrolling){ pendingScreen=screen; return; }
  pendingScreen=null;
  if(screen===lastScreen) return;          // unchanged -> no DOM work, no scroll jump
  lastScreen=screen;
  const t=$('#term'); const atBottom=t.scrollHeight-t.scrollTop-t.clientHeight<40;
  t.innerHTML=ansiToHtml(screen);
  if(atBottom){
    progScroll=true;                       // our own scrollTop write fires 'scroll'
    t.scrollTop=t.scrollHeight;
    requestAnimationFrame(()=>{ progScroll=false; });
  }
}
function noteUserScroll(){
  if(progScroll) return;                   // auto-follow isn't user scrolling
  userScrolling=true;
  clearTimeout(scrollQuiet);
  scrollQuiet=setTimeout(()=>{ userScrolling=false;
    if(pendingScreen!==null){ const s=pendingScreen; pendingScreen=null; applyScreen(s); } },200);
}
$('#term').addEventListener('scroll',noteUserScroll,{passive:true});
$('#term').addEventListener('touchstart',noteUserScroll,{passive:true});
// live screen over SSE: server pushes only when the pane content changes
let screenES: EventSource | null=null;
function openScreenStream(pane: string){
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
let rt: number | undefined;
function liveRefresh(){ clearTimeout(rt); rt=setTimeout(refreshScreen,150); }
async function key(k: string){ if(!current)return; await post({pane:current,key:k}); liveRefresh(); }
async function post(body: KeysBody){
  try{ const r=await fetch('/api/keys',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)});
    if(!r.ok) toast('failed'); }catch{ toast('offline'); }
}
document.querySelectorAll<HTMLButtonElement>('.keys button[data-key]').forEach(b=>b.onclick=()=>key(b.dataset.key!));

// ---- live terminal typing: forward keystrokes straight to the pane ----
const kbd=$('#kbd');
const SPECIAL: Record<string,string>={Enter:'Enter',Backspace:'BSpace',Escape:'Escape',Tab:'Tab',
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
const draft=$<HTMLTextAreaElement>('#draft'), editor=$('#editor'), modeBtn=$('#modeBtn'), LS=window.localStorage;
function wsKey(): string{ const s=sessions.find(x=>x.pane_id===current); return 'aw:draft:'+(s?s.workspace:current); }
function loadDraft(){ if(current) draft.value=LS.getItem(wsKey())||''; }
function saveDraftNow(){ if(current) try{ LS.setItem(wsKey(), draft.value); }catch{} }
let dft: number | undefined;
function saveDraft(){ clearTimeout(dft); dft=setTimeout(saveDraftNow,250); }
let editorOpen=false;
function showEditor(){
  const s=sessions.find(x=>x.pane_id===current);
  $('#edTitle').textContent='Draft → '+(s?s.workspace:current);
  loadDraft(); editor.classList.add('open'); editorOpen=true;
  setTimeout(()=>{ draft.focus(); fitViewport(); }, 60);
}
function hideEditor(){ if(!editorOpen) return; saveDraftNow(); try{draft.blur();}catch{} editor.classList.remove('open'); editorOpen=false; }
function openEditor(fromPop?: boolean){ if(!current) return;
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
function termCell(): {cols: number; rows: number}{ const t=$('#term'), cs=getComputedStyle(t);
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
async function unfit(pane: string | null){ lastFit=''; if(!pane) return;
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
  window.visualViewport.addEventListener('resize',fitViewport);
  window.visualViewport.addEventListener('scroll',fitViewport);
}
$('#bell').onclick=async()=>{ if('Notification'in window){ const p=await Notification.requestPermission(); toast(p==='granted'?'alerts on':'alerts blocked'); } };
// upload a screenshot, then paste its path into THIS session's prompt
$('#shotBtn').onclick=()=>{ if(current) $('#shotFile').click(); };
$<HTMLInputElement>('#shotFile').addEventListener('change',async(e)=>{
  const inp=e.target as HTMLInputElement;
  const f=inp.files&&inp.files[0]; inp.value=''; if(!f||!current) return;
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
