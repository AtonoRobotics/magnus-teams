// The operator view: Home, Inbox, New Task, Task Detail and Models, all from
// derived views of the log (K45). The developer view is in dev.js.
"use strict";

const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const api = async (m,p,b) => {
  const owner=App.me?.id, appliance=App.appliance, session=App.sessionGeneration, view=App.seq;
  let result;
  try { result=await Magnus.api(m,p,b); }
  catch(error) {
    // A lost mutation reply does not establish whether its effect happened.
    // Keep authoritative error metadata (including send rejection handling).
    // A spent limit of the person's own: offer to ask for more where it appears (admin-spend.js).
    if(error.status===402 && error.body?.spend_limit && typeof offerMoreSpend==="function")offerMoreSpend(error.body);
    // An admin change the server holds for the password again (admin.js): ask once, then send it again.
    if(error.status===403 && error.body?.error==="step_up" && typeof stepUp==="function")return stepUp().then(ok=>{if(!ok)throw Object.assign(new Error("The change was not made."),{cancelled:true});return Magnus.api(m,p,b);});
    if(!["GET","HEAD","OPTIONS"].includes(m) && (!error.status || error.status>=500))
      throw Object.assign(new Error(`Outcome not confirmed. ${error.message || "Check the current state before trying again."}`),{status:error.status,body:error.body});
    throw error;
  }
  if(m==="GET" && (owner!==App.me?.id || appliance!==App.appliance || session!==App.sessionGeneration || view!==App.seq)) throw Object.assign(new Error("This view changed."),{cancelled:true});
  return result;
};
const enc = encodeURIComponent;

// Read aloud in the appliance's own voice, a sentence at a time, so the first
// is heard while the next is made. With no voice on the appliance, the
// system's own voice reads it.
const voice = { run: 0, audio: null, done: null, entry: null };
function speechEntry(id) {
  voice.entry=id;
  for(const button of document.querySelectorAll("button[data-voice-entry]")) {
    const reading=button.dataset.voiceEntry===id, action=reading ? "hush" : "listen", label=reading ? "Stop reading" : "Read aloud";
    if(button.hasAttribute("data-msg-act"))button.dataset.msgAct=action;else button.dataset.act=reading ? "hush" : "speak";
    button.setAttribute("aria-label",label);button.dataset.tooltip=label;
    const svg=button.querySelector("svg");if(svg)svg.innerHTML=ICONS[action];
  }
}
function hush() {
  voice.run++;
  if (voice.audio) voice.audio.pause();
  if (voice.done) voice.done();
  voice.audio = voice.done = null;
  speechEntry(null);
  return Magnus.stopSpeaking();
}
async function readAloud(source, entryId) {
  const textReady=Promise.resolve(source);textReady.catch(()=>{});
  const stopped=hush(), run=voice.run;
  speechEntry(entryId);
  // Own startup before awaiting the host or the spoken-script request.
  try {
    await stopped;if(run!==voice.run)return;
    const text=await textReady;if(run!==voice.run)return;
    if(typeof text!=="string")throw new Error("No spoken text is available.");
    const pieces = s => { const out = []; let cur = ""; for (const w of s.split(/\s+/)) { if (cur && (cur + " " + w).length > 1500) { out.push(cur); cur = w; } else cur = cur ? `${cur} ${w}` : w; } if (cur) out.push(cur); return out; };
    const parts=text.split(/(?<=[.!?])\s+/).filter(s=>s.trim()).flatMap(pieces);
    const ask=async(s,tries=8)=>{
      if(run!==voice.run)return null;
      try{return await api("POST","/v1/speak",{text:s});}catch(e){
        if(e.status!==429 || tries<=1 || run!==voice.run)throw e;
        await new Promise(r=>setTimeout(r,400));return ask(s,tries-1);
      }
    };
    const make=s=>{const p=ask(s);p.catch(()=>{});return p;};
    let next=parts.length ? make(parts[0]) : null;
    for(let i=0;i<parts.length;i++) {
      let r;
      try{r=await next;}catch(e){if(e.status===503 && i===0 && run===voice.run){for(const part of parts){if(run!==voice.run)return;await Magnus.speak(part);}return;}throw e;}
      if(run!==voice.run || !r)return;
      next=i+1<parts.length ? make(parts[i+1]) : null;
      const src=URL.createObjectURL(new Blob([Uint8Array.from(atob(r.audio_b64),c=>c.charCodeAt(0))],{type:r.media_type}));
      await new Promise((done,fail)=>{
        const audio=voice.audio=new Audio(src);voice.done=done;
        audio.onended=done;audio.onerror=()=>fail(new Error("Could not play the audio."));audio.play().catch(fail);
      }).finally(()=>URL.revokeObjectURL(src));
      if(run!==voice.run)return;
    }
  } finally {
    if(run===voice.run){voice.audio=voice.done=null;speechEntry(null);}
  }
}

const App = {
  me: null,
  appliance: "",
  inbox: [],
  tasks: [],
  schedules: [],
  threads: [],
  projects: [],
  health: null,
  thread: null,
  /** The person closed this session's "Updates since your last session" notice. */
  feedDismissed: false,
  versions: null,
  update: null,
  timers: [],
  leave: [],
  routes: [],
  seq: 0,
  sessionGeneration: 0,
};

// Unsent messages, kept in memory for this session: what was typed and the
// files attached survive redraws and moving between views. One per
// conversation, and one for a new conversation in each project. Nothing is
// written to the device; signing out forgets them (#544).
const MessageDrafts = (() => {
  let person = null, rows = {}, epoch = 0;
  const listeners = new Set(), originals = new Map();
  const emit = () => { for (const f of listeners) f(); };
  const key = (thread, project) => thread ? `thread:${thread}` : `new:${project || ""}`;
  function forThread(thread, project = null) {
    const id = key(thread, project);
    return rows[id] ||= {id, thread, project, text:"", files:[], selection:[0,0], anchor:null};
  }
  function clear() { rows = {}; originals.clear(); epoch++; if (typeof TeamDrafts !== "undefined") TeamDrafts.clear(); emit(); }
  // Another person signing in on this computer starts with nothing.
  function open(appliance, who) { const next = `${appliance}/${who}`; if (next !== person) { person = next; clear(); } }
  function discard(id) { if (!rows[id]) return; for (const f of rows[id].files) originals.delete(f.uid); delete rows[id]; emit(); }
  return {forThread, open, clear, discard, changed:emit, uid:()=>crypto.randomUUID(),
    get:id=>rows[id], all:()=>Object.values(rows), originals, epoch:()=>epoch,
    subscribe:f=>{listeners.add(f);return()=>listeners.delete(f);}};
})();

// Capture an entry and its relative offset before content/layout changes.
const ReadingAnchors = (() => {
  const state = new WeakMap();
  function capture(root) {
    if (!root) return null;
    if (!root.clientHeight) return state.get(root) || null;
    const bounds = root.getBoundingClientRect();
    const frames=[...root.querySelectorAll("iframe.composed")];
    for (const [frameIndex,frame] of frames.entries()) {
      const rect=frame.getBoundingClientRect();
      if (rect.top>bounds.top || rect.bottom<=bounds.top) continue;
      try {
        const nodes=[...frame.contentDocument.querySelectorAll("h1,h2,h3,h4,p,li,tr,pre")];
        const index=nodes.findIndex(n=>rect.top+n.getBoundingClientRect().bottom>bounds.top);
        if (index>=0) return {follow:root.scrollHeight-root.scrollTop-root.clientHeight<=80,top:root.scrollTop,frameIndex,nodeIndex:index,offset:rect.top+nodes[index].getBoundingClientRect().top-bounds.top};
      } catch { /* frame not loaded */ }
    }
    const node = [...root.querySelectorAll("[data-entry], [data-shown], [data-key], h2, h3, p")].find(n => n.getBoundingClientRect().bottom > bounds.top);
    return {follow:root.scrollHeight-root.scrollTop-root.clientHeight<=80, top:root.scrollTop,
      entry:node?.dataset.entry, shown:node?.dataset.shown, key:node?.dataset.key,
      index:node ? [...root.querySelectorAll("h2,h3,p")].indexOf(node) : -1,
      offset:node ? node.getBoundingClientRect().top-bounds.top : 0};
  }
  function restore(root, anchor) {
    if (!root || !anchor || !root.clientHeight) return;
    if (anchor.follow) root.scrollTop = root.scrollHeight;
    else if (anchor.frameIndex!==undefined) {
      try {
        const frame=root.querySelectorAll("iframe.composed")[anchor.frameIndex];
        const node=frame?.contentDocument.querySelectorAll("h1,h2,h3,h4,p,li,tr,pre")[anchor.nodeIndex];
        root.scrollTop=node ? root.scrollTop+frame.getBoundingClientRect().top+node.getBoundingClientRect().top-root.getBoundingClientRect().top-anchor.offset : anchor.top;
      } catch { root.scrollTop=anchor.top; }
    } else {
      const node = anchor.entry ? root.querySelector(`[data-entry="${CSS.escape(anchor.entry)}"]`) : anchor.shown ? root.querySelector(`[data-shown="${CSS.escape(anchor.shown)}"]`) : anchor.key ? root.querySelector(`[data-key="${CSS.escape(anchor.key)}"]`) : root.querySelectorAll("h2,h3,p")[anchor.index];
      root.scrollTop = node ? root.scrollTop + node.getBoundingClientRect().top-root.getBoundingClientRect().top-anchor.offset : anchor.top;
    }
  }
  function remember(root, preferred) { const a = preferred || capture(root); if (root) state.set(root,a); return a; }
  function hold(root, change) { const a = capture(root); const result = change(); restore(root,a); if (root) state.set(root,a); return result; }
  function watch(root, record, persist=true) {
    let restoredTop = null;
    if (record?.anchor) restore(root,record.anchor); else restore(root,{follow:true});
    state.set(root,record?.anchor || capture(root));
    const onScroll = () => {
      // Ignore only our own restored offset. Deliberate scrolling can
      // arrive before the next animation frame and must update the anchor.
      if(restoredTop!==null && Math.abs(root.scrollTop-restoredTop)<1){restoredTop=null;return;}
      restoredTop=null;
      if (!root.clientHeight) return;
      const a = remember(root); if (!persist) a.follow=false; if (a.follow) root.parentElement?.querySelector("[data-jump-latest]")?.setAttribute("hidden", ""); if (record) record.anchor = a;
    };
    root.addEventListener("scroll",onScroll,{passive:true});
    const resize = new ResizeObserver(() => {
      restore(root,state.get(root));restoredTop=root.scrollTop;
    });
    const observe = () => { resize.disconnect(); resize.observe(root); for (const n of root.children) resize.observe(n); };
    const content = new MutationObserver(observe); content.observe(root,{childList:true}); observe();
    return () => { if (record) { record.anchor = capture(root) || record.anchor; if (!persist && record.anchor) record.anchor.follow=false; } root.removeEventListener("scroll",onScroll); content.disconnect(); resize.disconnect(); };
  }
  return {capture,restore,remember,hold,watch};
})();

// Each resource retains its last good data; only its newest response may reconcile it.
const Resources = (()=>{
 let epoch=0, serial=0;const states=new Map();let announced="";
 function render() {
  const shell=$(".shell");if(!shell)return;
  let el=$("#connection-status");if(!el){el=document.createElement("div");el.id="connection-status";el.className="connection-status";shell.append(el);}
  const stale=[...states].filter(([,s])=>s.failed), text=stale.map(([name,s])=>`${name} may be stale · ${s.at ? `Last refreshed ${new Date(s.at).toLocaleTimeString()}` : "Not yet refreshed"}`).join(". ");
  el.hidden=!stale.length;
  if(el.textContent!==text)el.textContent=text;
  // Announce transitions once, never repeat unchanged polling failures or timestamps.
  const summary=stale.map(([name])=>name).join(", ");
  if(summary!==announced){announced=summary;let live=$("#connection-announcement");if(!live){live=document.createElement("div");live.id="connection-announcement";live.className="sr";live.setAttribute("role","status");document.body.append(live);}live.textContent=summary ? `${summary} may be stale. Cached information remains available.` : "Connection restored.";}
 }
 function reset(){epoch++;states.clear();announced="";render();}
 function forget(name){states.delete(name);render();}
 function good(name){const s=states.get(name)||{};states.set(name,{...s,failed:false,at:Date.now()});render();}
 function bad(name,error){if(error?.cancelled)return;const s=states.get(name)||{};states.set(name,{...s,failed:true});render();if(error?.status===401 && App.me){stop();signInScreen("Your session ended. Sign in again.");}}
 async function read(name,path,key,cached=false){const scope=epoch,token=++serial,prior=states.get(name)||{};states.set(name,{...prior,token});
  try{const response=await api("GET",path);if(scope!==epoch || states.get(name)?.token!==token)return;const value=key ? response[key] : response;states.set(name,{...states.get(name),path,value});good(name);return value;}
  catch(error){if(cached && error?.cancelled)throw error;if(!error?.cancelled && scope===epoch && states.get(name)?.token===token){bad(name,error);if(cached && prior.path===path)return prior.value;}}
 }

 return {read,reset,forget,good,bad,render};
})();

// Retain the existing first-presentation effect on actual visible update cards.
let openedFrame=null;
function reportOpenedSoon() {
  if(openedFrame!==null)return;
  openedFrame=requestAnimationFrame(()=>{
    openedFrame=null;if(!App.me)return;
    const reported=App.reported ||= new Set(),reporting=App.reporting ||= new Set();
    const eligible=new Map(App.inbox.filter(i=>i.status==="open" && ["observation","interrupt"].includes(i.kind) && !i.opened_at && !reported.has(i.id) && !reporting.has(i.id)).map(i=>[i.id,i]));
    const ids=new Set();
    for(const card of document.querySelectorAll("article[data-id]")){
      const id=card.dataset.id;if(!eligible.has(id) || card.closest("details:not([open])") || !card.getClientRects().length)continue;
      const bounds=card.getBoundingClientRect(),root=card.closest(".detail-body,.thread"),clip=root?.getBoundingClientRect();
      if(bounds.bottom<=Math.max(0,clip?.top||0) || bounds.top>=Math.min(innerHeight,clip?.bottom??innerHeight) || bounds.right<=Math.max(0,clip?.left||0) || bounds.left>=Math.min(innerWidth,clip?.right??innerWidth))continue;
      ids.add(id);
    }
    if(!ids.size)return;
    for(const id of ids)reporting.add(id);
    api("POST","/v1/inbox/opened",{ids:[...ids]}).then(response=>{
      for(const id of response.opened || [])reported.add(id);
    },()=>{}).finally(()=>{for(const id of ids)reporting.delete(id);});
  });
}

// A question before an action, asked in the app itself: a webview may
// ignore window.confirm and answer no without showing it, so a button
// behind one did nothing at all (the Windows app, owner 2026-11-03:
// Disconnect in the Library). Escape, or Back, is no.
function ask(msg, yes) {
  return new Promise(done => {
    const d = document.createElement("dialog");
    d.className = "ask";
    d.innerHTML = `<p></p><div class="row"><button data-ask="no">Back</button><button class="primary" data-ask="yes"></button></div>`;
    $("p", d).textContent = msg;
    $("[data-ask=yes]", d).textContent = yes;
    d.addEventListener("click", e => { const b = e.target.closest("[data-ask]"); if (b) d.close(b.dataset.ask); });
    d.addEventListener("close", () => { d.remove(); done(d.returnValue === "yes"); });
    document.body.append(d);
    d.showModal();
    $("[data-ask=no]", d).focus();
  });
}

function toast(msg, ms = 3200) {
  const t = document.createElement("div");
  t.className = "toast"; t.textContent = msg; t.setAttribute("role", "status");
  // Toasts stack, newest at the bottom, rather than covering each other.
  let box = $("#toasts");
  if (!box) { box = document.createElement("div"); box.id = "toasts"; document.body.append(box); }
  box.append(t);
  setTimeout(() => t.remove(), ms);
}

function when(t) {
  if (!t) return "";
  const d = new Date(t), now = Date.now(), s = (now - d.getTime()) / 1000;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return d.toLocaleString();
}
function until(t) {
  if (!t) return "";
  const d = new Date(t), s = (d.getTime() - Date.now()) / 1000;
  if (s < 60) return "now";
  if (s < 3600) return `in ${Math.ceil(s / 60)} min`;
  if (s < 86400) return `in ${Math.round(s / 3600)} h`;
  return d.toLocaleString(undefined, { weekday: "short", month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
}
const firstLine = s => (s || "").split("\n").find(l => l.trim()) || "";

const isOwner = () => App.me?.roles?.includes("owner");
const isDev = () => App.me?.roles?.includes("developer");

// Replace a region's markup only when it changed, keeping what the person
// was typing (anything marked data-draft), where the focus was, and which
// sections they opened (details marked data-key).
const drafts = {};
function paint(el, html) {
  if (!el || el._html === html) return false;
  // A card can be shown in several places at once (its conversation, the
  // panel, Needs you), each copy with the same draft key: the copy being
  // typed in is the draft, and only a region that held the focus takes it.
  const active = document.activeElement;
  // Keep explicitly transient credential controls in the live DOM only.
  // Polling may redraw their surrounding form; never put keys in drafts.
  const ephemeral=new Map($$("input[data-ephemeral][id]",el).map(input=>[input.id,{input,start:input.selectionStart,end:input.selectionEnd}]));
  const menuState = Menus.capture(el);
  $$("[data-draft]", el).forEach(i => { if (!(active !== i && active?.dataset?.draft === i.dataset.draft)) drafts[i.dataset.draft] = i.type === "checkbox" ? i.checked : i.value; });
  const opened = new Set($$("details[data-key]", el).filter(d => d.open).map(d => d.dataset.key));
  const focused = el.contains(active) ? active?.dataset?.draft : undefined;
  const control = el.contains(active) && !focused ? {
    id:active.id, entry:active.closest("[data-entry]")?.dataset.entry,
    act:active.dataset?.msgAct, label:active.getAttribute("aria-label"), open:active.dataset?.open,
    tag:active.tagName.toLowerCase(),
  } : null;
  el.innerHTML = html;
  el._html = html;
  for(const input of $$("input[data-ephemeral][id]",el)){
    const prior=ephemeral.get(input.id);if(!prior || prior.input.type!==input.type)continue;
    input.replaceWith(prior.input);if(prior.start!==null)prior.input.setSelectionRange(prior.start,prior.end);
  }
  loadMarkdownImages(el);
  $$("details[data-key]", el).forEach(d => { if (opened.has(d.dataset.key)) d.open = true; });
  $$("[data-draft]", el).forEach(i => { const v = drafts[i.dataset.draft]; if (v === undefined) return; if (i.type === "checkbox") i.checked = !!v; else i.value = v; });
  if (focused) $(`[data-draft="${CSS.escape(focused)}"]`, el)?.focus({preventScroll:true});
  if (control) {
    const within=control.entry ? $(`[data-entry="${CSS.escape(control.entry)}"]`,el) : el;
    const target=control.id ? $(`#${CSS.escape(control.id)}`,el) : within && (control.act ? $(`[data-msg-act="${CSS.escape(control.act)}"]`,within) : control.open ? $(`[data-open="${CSS.escape(control.open)}"]`,within) : control.label ? $(`${control.tag}[aria-label="${CSS.escape(control.label)}"]`,within) : null);
    target?.focus({preventScroll:true});
  }
  Menus.restore(menuState);
  reportOpenedSoon();
  return true;
}

function every(ms, fn) { const t = setInterval(fn, ms); App.timers.push(t); }
function onLeave(fn) { App.leave.push(fn); }

const STATUS = {
  running: ["run", "Working"], suspended: ["warn", "Paused"], completed: ["ok", "Done"], failed: ["bad", "Failed"], cancelled: ["", "Cancelled"],
};
const WAITING = {
  approval: "waiting for an approval", review: "waiting for your review", question: "waiting for your answer", budget: "paused: budget reached",
  contract_change: "waiting to go on",
  provider: "paused: the model provider is unavailable", usage_limit: "paused: model usage limit", auth: "paused: sign in to the model again",
  monitor: "waiting for you: the safety monitor held an action", secure_form: "waiting for you to sign in", payment: "waiting for you to approve a purchase", takeover: "paused: you have control of the computer", takeover_request: "waiting for you to take over the computer",
  kill_switch: "stopped by the kill switch", timer: "waiting until a set time", invalid_request: "paused: the model rejected the request",
  spend_limit: "paused: a monthly spend limit is reached",
};
const chip = st => { const [c, l] = STATUS[st] || ["", st]; return `<span class="chip ${c}">${esc(l)}</span>`; };

// ---------- shell ----------

// One workspace: the conversation in the middle, what was asked of it beside
// it in a panel, and the sidebar. Everything it shows is read together
// (refresh), so a task, a schedule or a decision is one object wherever it
// appears, and acting on it anywhere redraws every place it is shown.
const DECISION_KINDS=new Set(["monitor","secure_form","takeover_request","payment","approval","question","contract_change","review","policy_proposal","suggestion","app_tools","deletion_request","endorsement"]);
const needsDecision=i=>i.status==="open" && i.can_respond && (DECISION_KINDS.has(i.kind) || (i.kind==="notice" && i.blocking)) && !(i.details?.notify_at && Date.parse(i.details.notify_at)>Date.now());
const needsMe = () => App.inbox.filter(needsDecision);

// The workspace's icons, drawn as in the Magnus Workspace design: 24px
// outlines in the current color.
const UI = {
  plus: '<path d="M12 5v14M5 12h14"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
  bell: '<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  tasks: '<path d="M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01"/>',
  calendar: '<rect x="3" y="5" width="18" height="16" rx="2"/><path d="M16 3v4M8 3v4M3 10h18"/>',
  file: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>',
  chat: '<path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12z"/>',
  trash: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>',
  gear: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>',
  done: '<circle cx="12" cy="12" r="9"/><path d="m8.5 12 2.5 2.5 4.5-5"/>',
  need: '<circle cx="12" cy="12" r="9"/><path d="M12 8v5M12 16h.01"/>',
  working: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  stopped: '<circle cx="12" cy="12" r="9"/><path d="M15 9l-6 6M9 9l6 6"/>',
  globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>',
  attach: '<path d="m21 11-8.6 8.6a5 5 0 0 1-7-7l8.5-8.6a3.3 3.3 0 0 1 4.7 4.7l-8.6 8.6a1.7 1.7 0 0 1-2.3-2.3l8-8"/>',
  send: '<path d="M12 19V5M5 12l7-7 7 7"/>',
  shield: '<path d="M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z"/><path d="m9 12 2 2 4-4"/>',
  close: '<path d="M18 6 6 18M6 6l12 12"/>',
  download: '<path d="M12 4v11M7 10l5 5 5-5M5 20h14"/>',
  chev: '<path d="m9 6 6 6-6 6"/>',
  grid: '<rect x="4" y="4" width="7" height="7" rx="1.5"/><rect x="13" y="4" width="7" height="7" rx="1.5"/><rect x="4" y="13" width="7" height="7" rx="1.5"/><rect x="13" y="13" width="7" height="7" rx="1.5"/>',
};
const icon = (k, cls = "") => `<svg class="ic${cls ? ` ${cls}` : ""}" viewBox="0 0 24 24" aria-hidden="true">${UI[k]}</svg>`;

function navigationTree() {
  const current=href=>(location.hash || "#/home")===href ? 'class="on" aria-current="page"' : "";
  const conv=t=>`<a class="conv${t.thread===App.thread ? " on" : ""}" href="#/chat/${enc(t.thread)}" ${t.thread===App.thread ? 'aria-current="page"' : ""} title="${esc(t.title)}"><span class="ttl">${esc(t.title)}</span></a>`;
  const loose=App.threads.filter(t=>!t.project);
  return `<div class="sect" role="group" aria-label="Projects"><div class="group"><span>Projects</span><a href="#/projects" data-open="projects" class="more">All</a></div>
    ${App.projects.map(p=>`<a href="#/project/${enc(p.id)}" data-open="project:${esc(p.id)}" title="${esc(p.name)}">${icon("folder")}<span class="ttl">${esc(p.name)}</span></a>${App.threads.filter(t=>t.project?.id===p.id).map(conv).join("")}`).join("") || '<p class="small muted pad">No projects yet.</p>'}</div>
    <div class="sect" role="group" aria-label="Conversations"><div class="group"><span>Conversations</span></div>${loose.map(conv).join("") || '<p class="small muted pad">No conversations yet.</p>'}</div>
    <div class="sect"><a href="#/trash" ${current("#/trash")}>${icon("trash")}<span>Trash</span></a>${isDev() ? `<details class="developer-nav"><summary>Developer</summary>${[["runs","Runs"],["experiments","Experiments"],["system","System"],["policy","Policy"],["playbook","Playbook"],["computers","Computers"]].map(([k,l])=>`<a href="#/dev/${k}" ${current(`#/dev/${k}`)}>${l}</a>`).join("")}</details>` : ""}</div>`;
}
function nav() {
  const route=location.hash || "#/home", waiting=needsMe();
  const item=Panel.what?.startsWith("item:") ? App.inbox.find(i=>i.id===Panel.what.slice(5)) : null;
  const active=k=>k==="activity" ? ["activity","tasks","schedules"].includes(Panel.what) || /^(task|schedule):/.test(Panel.what || "") || (Panel.what?.startsWith("item:") && !needsDecision(item || {})) : k==="files" ? Panel.what==="files" || Panel.what?.startsWith("file:") : k==="needs" ? Panel.what==="needs" || (item && needsDecision(item)) : route.startsWith("#/library");
  const primary=(k,label,ic,href)=>`<a class="primary-nav${k==="needs" ? " needs" : ""}${active(k) ? " on" : ""}" href="${href}" ${!["apps","computer","teams"].includes(k) ? `data-open="${k}"` : ""} ${active(k) ? 'aria-current="page"' : ""} aria-label="${label}${k==="needs" && waiting.length ? `, ${waiting.length} decisions` : ""}" title="${label}">${icon(ic)}<span class="nav-label">${label}</span>${k==="needs" ? `<span class="pill${waiting.length ? "" : " quiet"}" data-needs-count aria-hidden="true">${waiting.length}</span>` : ""}</a>`;
  return `${App.update?.ready ? `<div class="update-ready"><span class="small">Magnus ${esc(App.update.ready.version)} is ready.</span><button data-act="restart-update">Restart to update</button></div>` : ""}
    <a href="#/chat/new" class="new-chat primary-nav" aria-label="New conversation" title="New conversation">${icon("plus")}<span class="nav-label">New conversation</span></a>
    <div class="primary-destinations">${primary("activity","Activity","tasks","#/activity")}${primary("needs","Needs you","bell","#/inbox")}${primary("files","Files","file","#/files")}${primary("apps","Apps","grid","#/library")}${primary("computer","Computers","computer","#/computer")}${primary("teams","Teams","team","#/teams")}${adminNavItem(route)}</div>
    <button type="button" class="rail-tree icon-btn" data-nav-tree aria-label="Projects and conversations" title="Projects and conversations" aria-expanded="false" aria-controls="navigation-flyout">${icon("folder")}</button>
    <div class="nav-tree">${navigationTree()}</div>
    <div class="foot"><div class="who"><span class="avatar" aria-hidden="true">${esc((App.me?.name || "?").trim().charAt(0).toUpperCase())}</span><div class="whoami" data-appliance="${esc(App.appliance)}"><span class="ttl" title="${esc(App.me?.id)}">${esc(App.me?.name)}</span><button class="link" data-act="sign-out">Sign out</button></div><a href="#/settings" class="icon-btn${route.startsWith("#/settings") ? " on" : ""}" ${route.startsWith("#/settings") ? 'aria-current="page"' : ""} aria-label="Settings" title="Settings">${icon("gear")}</a></div></div>`;
}
function sidebar() {
  paint($("#side"),nav());
  if (!$("#navigation-flyout")?.hidden) paint($("#navigation-flyout"),`<h2>Projects and conversations</h2>${navigationTree()}<button data-act="sign-out">Sign out</button><button data-close-tree>Close navigation list</button>`);
}

// Fit is based on usable content width, independently of stored preferences.
const Workspace = (() => {
  let expanded=false, region="conversation", forceExpanded=false, installed=false, restoreDrawer=true;
  // A team workspace brings its own rail, so the app's navigation folds to
  // icons there unless the person opens it.
  function fit(width, preferred="expanded", conversation=420, work=false, team=false) {
    const mobile=width<760;
    const temporary=(work || team) && !expanded && !forceExpanded && preferred==="expanded" && (team || width-228<936);
    const nav=mobile ? "drawer" : preferred==="compact" || temporary ? "compact" : "expanded";
    const navWidth=mobile ? 0 : nav==="compact" ? 64 : 228, available=width-navWidth;
    const eligible=work && available>=936;
    const max=Math.min(560,available-576), actual=Math.max(360,Math.min(560,Number(conversation)||420));
    return {nav,navWidth,available,eligible,split:eligible && !expanded,min:360,max:Math.max(360,max),conversation:eligible ? Math.min(actual,max) : actual,temporary};
  }
  function update() {
    const shell=$(".shell"); if (!shell) return;
    const prefs=Theme.prefs(), work=!!Panel.what && !!App.workspace;
    const team=/^#\/team\//.test(location.hash);
    const m=fit(shell.clientWidth,prefs.navigation || "expanded",prefs.conversationWidthPx || 420,work,team);
    if(team)document.body.dataset.team="";else delete document.body.dataset.team;
    const prior=[...document.querySelectorAll("#thread, #detail .detail-body")].filter(el=>el.clientHeight).map(root=>{
      const rect=root.getBoundingClientRect(), nodes=[...root.querySelectorAll("[data-entry], iframe, h2, h3, p")], node=nodes.find(n=>n.getBoundingClientRect().bottom>rect.top);
      return {root,node,top:root.scrollTop,offset:node ? node.getBoundingClientRect().top-rect.top : 0,follow:root.id==="thread" && root.scrollHeight-root.scrollTop-root.clientHeight<=80};
    });
    shell.style.setProperty("--nav-width",`${m.navWidth}px`);shell.style.setProperty("--conversation-width",`${m.conversation}px`);
    document.body.dataset.navigation=m.nav;document.body.dataset.workspace=!work ? "conversation" : m.split ? "split" : "single";document.body.dataset.region=region;
    const height=Math.min(innerHeight,window.visualViewport?.height || innerHeight);shell.style.setProperty("--workspace-height",`${height}px`);shell.style.setProperty("--workspace-top",`${window.visualViewport?.offsetTop || 0}px`);document.body.classList.toggle("short-viewport",height<520);
    const toggle=$("[data-nav-toggle]");if(toggle){toggle.setAttribute("aria-label",m.nav==="expanded" ? "Collapse navigation" : "Expand navigation");toggle.title=toggle.getAttribute("aria-label");toggle.setAttribute("aria-expanded",String(m.nav==="expanded"));}
    $("[data-open-drawer]").hidden=m.nav!=="drawer";
    const selectors=$(".workspace-selectors");selectors.hidden=!work || m.split;
    for(const b of $$("[data-region]",selectors)){const on=b.dataset.region===region;b.setAttribute("aria-pressed",String(on));}
    const grow=$("[data-expand-work]");grow.hidden=!work || (!m.eligible && !expanded);grow.setAttribute("aria-label",expanded ? "Restore split view" : "Expand work");grow.title=grow.getAttribute("aria-label");grow.setAttribute("aria-pressed",String(expanded));
    $("[data-close-work]").hidden=!work;
    $(".workspace-heading").textContent=App.workspace ? "Conversation" : location.hash.startsWith("#/library") ? "Apps" : location.hash.startsWith("#/settings") ? "Settings" : location.hash.startsWith("#/admin") ? "Admin" : location.hash.startsWith("#/dev") ? "Developer" : "Magnus";
    $(".workspace-heading").hidden=work;
    const separator=$("#workspace-splitter");separator.hidden=!m.split;separator.setAttribute("aria-valuenow",String(Math.round(m.conversation)));separator.setAttribute("aria-valuemin",String(m.min));separator.setAttribute("aria-valuemax",String(m.max));separator.setAttribute("aria-valuetext",`${Math.round(m.conversation)} pixels`);
    if (m.nav!=="drawer" && $("#navigation-drawer").open) closeDrawer(false);
    for(const a of prior){if(!a.root.clientHeight)continue;a.root.scrollTop=a.follow ? a.root.scrollHeight : a.node?.isConnected ? a.root.scrollTop+a.node.getBoundingClientRect().top-a.root.getBoundingClientRect().top-a.offset : a.top;}
    reportOpenedSoon();
    return m;
  }
  function remember() { history.replaceState({...history.state,workspace:{work:Panel.what,expanded,region}},"",location.href); }
  function choose(which) {region=which;remember();update();const target=which==="work" ? $("#detail h2") : $("#msg");if(target){if(which==="work")target.tabIndex=-1;target.focus({preventScroll:true});}}
  function opened() {const saved=history.state?.workspace;if(saved?.work===Panel.what){expanded=!!saved.expanded;region=saved.region==="conversation" ? "conversation" : "work";}else region="work";forceExpanded=false;remember();update();}
  function closed() {expanded=false;region="conversation";remember();update();}
  function openDrawer() {
    const dialog=$("#navigation-drawer"),nav=$("nav.side");if(dialog.open)return;
    dialog.append(nav);restoreDrawer=true;dialog.showModal();$("[data-close-drawer]",nav).focus();
  }
  function closeDrawer(restore=true) {const d=$("#navigation-drawer");if(!d?.open)return;restoreDrawer=restore;d.close();}
  function closeTree(restore=true) {const fly=$("#navigation-flyout");if(!fly)return;fly.hidden=true;const b=$("[data-nav-tree]");b?.setAttribute("aria-expanded","false");if(restore)b?.focus({preventScroll:true});}
  function resize(value) {const m=update();Theme.set({conversationWidthPx:Math.max(m.min,Math.min(m.max,value))});update();}
  function install() {
    expanded=false;region="conversation";forceExpanded=false;
    const d=$("#navigation-drawer");d.addEventListener("click",e=>{if(e.target===d){const r=d.getBoundingClientRect();if(e.clientX<r.left || e.clientX>r.right || e.clientY<r.top || e.clientY>r.bottom)closeDrawer();}});d.addEventListener("close",()=>{const nav=$("nav.side",d);if(nav)$(".shell").prepend(nav);if(restoreDrawer)$("[data-open-drawer]")?.focus({preventScroll:true});});
    $("#workspace-bar").onclick=e=>{const b=e.target.closest("button");if(!b)return;if(b.matches("[data-open-drawer]"))openDrawer();if(b.dataset.region)choose(b.dataset.region);if(b.matches("[data-expand-work]")){expanded=!expanded;if(expanded)region="work";remember();update();}if(b.matches("[data-close-work]"))closePanel(true);};
    $("[data-nav-toggle]").onclick=()=>{const m=update(),navigation=m.nav==="compact" ? "expanded" : "compact";forceExpanded=navigation==="expanded";Theme.set({navigation});update();};
    $("[data-close-drawer]").onclick=()=>closeDrawer();
    $("[data-rail-search]").onclick=()=>{const dialog=$("#workspace-search");dialog.showModal();$("input",dialog).focus();};
    $("#workspace-search form").onsubmit=e=>{e.preventDefault();const q=$("#workspace-search input").value.trim();$("#workspace-search").close();if(q){if(App.workspace)openPanel(`search:${q}`);else{App.openAfter=`search:${q}`;location.hash="#/home";}}};
    $("[data-close-search]").onclick=()=>$("#workspace-search").close();
    const separator=$("#workspace-splitter");separator.onkeydown=e=>{const m=update();let width=m.conversation;if(e.key==="Home")width=m.min;else if(e.key==="End")width=m.max;else if(["ArrowLeft","ArrowRight"].includes(e.key))width+=(e.key==="ArrowLeft" ? 1 : -1)*(e.shiftKey ? 48 : 16);else return;e.preventDefault();resize(width);};
    separator.onpointerdown=e=>{if(e.button!==0)return;const m=update(),x=e.clientX,width=m.conversation;separator.setPointerCapture(e.pointerId);separator.onpointermove=move=>resize(width+x-move.clientX);separator.onpointerup=()=>{separator.onpointermove=null;separator.onpointerup=null;separator.releasePointerCapture(e.pointerId);};};
    if(!installed){installed=true;
      window.addEventListener("resize",()=>{forceExpanded=false;update();closeTree(false);});window.visualViewport?.addEventListener("resize",update);window.visualViewport?.addEventListener("scroll",()=>$(".shell")?.style.setProperty("--workspace-top",`${window.visualViewport?.offsetTop || 0}px`));window.addEventListener("magnus-theme",update);
      document.addEventListener("click",e=>{
        const tree=e.target.closest("[data-nav-tree]");if(tree){const fly=$("#navigation-flyout");if(fly.hidden){fly.hidden=false;sidebar();tree.setAttribute("aria-expanded","true");$("a,button",fly)?.focus({preventScroll:true});}else closeTree();return;}
        if(e.target.closest("[data-close-tree]"))closeTree();else if(!e.target.closest("#navigation-flyout"))closeTree(false);
        const destination=e.target.closest("nav.side a, #navigation-flyout a");
        if(destination){if(e.button===0 && !e.ctrlKey && !e.metaKey && !destination.dataset.open && destination.hash!==location.hash)App.routeFocus={opener:destination,composer:destination.hash.startsWith("#/chat/new")};closeTree(false);closeDrawer(false);}
        if(e.target.closest("#navigation-flyout [data-act=sign-out]"))$("nav.side [data-act=sign-out]")?.click();
      });
      document.addEventListener("keydown",e=>{if(e.key==="Escape" && !$("#navigation-flyout")?.hidden){e.preventDefault();closeTree();}});
    }
    update();
  }
  return {fit,update,install,opened,closed,choose};
})();

function shell() {
  document.body.innerHTML = `<div class="shell"><nav class="side" aria-label="Sections"><div class="brand"><span class="logo" aria-hidden="true">M</span><span class="brand-name">Magnus</span><button class="icon-btn" data-nav-toggle aria-label="Collapse navigation" title="Collapse navigation">${icon("chev")}</button><button class="icon-btn" data-close-drawer aria-label="Close navigation">${icon("close")}</button></div>
    <button class="icon-btn rail-search" data-rail-search aria-label="Search conversations, tasks and files" title="Search conversations, tasks and files">${icon("search")}</button>
    <form id="find" role="search">${icon("search")}<label class="sr" for="q">Search conversations, tasks and files</label><input id="q" type="search" placeholder="Search conversations, tasks and files" autocomplete="off"></form><div id="side"></div></nav>
    <header id="workspace-bar" class="workspace-bar"><button class="icon-btn" data-open-drawer aria-label="Open navigation">${icon("tasks")}</button><span class="workspace-heading">Conversation</span><div class="workspace-selectors" aria-label="Workspace region"><button data-region="work" aria-pressed="false">Work</button><button data-region="conversation" aria-pressed="true">Conversation</button></div><span class="grow"></span><div class="admin-kill" data-admin-kill role="group" aria-label="Kill switch" hidden></div><button class="icon-btn" data-expand-work aria-label="Expand work">${icon("grid")}</button><button class="icon-btn" data-close-work data-act="close-panel" aria-label="Close work">${icon("close")}</button></header>
    <main id="main" tabindex="-1"></main><div id="workspace-splitter" role="separator" tabindex="0" aria-label="Conversation width" aria-orientation="vertical" hidden></div><aside id="detail" class="detail" aria-label="Work" hidden></aside></div>
    <dialog id="navigation-drawer" class="navigation-drawer" aria-label="Navigation"></dialog><div id="navigation-flyout" class="navigation-flyout" role="dialog" aria-label="Projects and conversations" hidden></div>
    <dialog id="workspace-search" class="workspace-search" aria-label="Search conversations, tasks and files"><form><label for="workspace-query">Search conversations, tasks and files</label><input id="workspace-query" type="search" required><div class="row"><button type="submit">Search</button><button type="button" data-close-search>Close search</button></div></form></dialog>`;
  Workspace.install();
  $("nav.side").addEventListener("click", async e => {
    if (e.target.closest("[data-act=restart-update]")) return restartToUpdate();
    if (e.target.closest("[data-act=sign-out]")) {
      MessageDrafts.clear();
      stop();
      await Magnus.signOut();
      location.hash = "";
      start();
    }
  });
  $("#find").onsubmit = e => { e.preventDefault(); const q = $("#q").value.trim(); if (q) openPanel(`search:${q}`,{history:true,focus:true,opener:$("#q")}); };
  sidebar();
  wirePanel();
  if (!App.wired) { App.wired = true; wireDocument(); }
}

// One open menu and one shared tooltip, for both input families.
const Menus = (() => {
  let current = null, trigger = null, anchorPosition = null;
  const items = () => current ? $$('[role="menuitem"]:not(:disabled), [role="menuitemcheckbox"]:not(:disabled)', current) : [];
  function close(restore = false) {
    if (!current) return;
    current.hidden = true; trigger?.setAttribute("aria-expanded", "false");
    const back = trigger; current = trigger = null; anchorPosition=null;
    if (restore && back?.isConnected) back.focus({ preventScroll: true });
  }
  function open(button, focus = true) {
    const menu = document.getElementById(button.dataset.menu);
    if (!menu) return;
    if (current === menu) return close(true);
    // A transformed entry becomes the containing block of fixed descendants.
    // End its entrance before placing a viewport-relative menu.
    button.closest('.entry.enter')?.classList.remove('enter');
    close(); current = menu; trigger = button; menu.hidden = false;
    button.setAttribute("aria-expanded", "true");
    const anchor = button.getBoundingClientRect(), r = menu.getBoundingClientRect();
    anchorPosition={left:anchor.left,top:anchor.top};
    menu.style.left = `${Math.max(8, Math.min(anchor.left, innerWidth - r.width - 8))}px`;
    menu.style.top = `${anchor.bottom + r.height <= innerHeight - 8 ? anchor.bottom : Math.max(8, anchor.top - r.height)}px`;
    if (focus) items()[0]?.focus({ preventScroll: true });
  }
  function install() {
    document.addEventListener("click", e => {
      const button = e.target.closest("[data-menu]");
      if (button) { e.preventDefault(); open(button); return; }
      if (current && (e.target.closest('[role="menuitem"], [role="menuitemcheckbox"]') || !current.contains(e.target))) close();
    });
    document.addEventListener("keydown", e => {
      if (!current) return;
      if (e.key === "Escape") { e.preventDefault(); close(true); return; }
      if (e.key === "Tab") { trigger?.focus({ preventScroll: true }); close(); return; }
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) return;
      e.preventDefault(); const list = items(), n = list.indexOf(document.activeElement);
      const at = e.key === "Home" ? 0 : e.key === "End" ? list.length - 1 : (n + (e.key === "ArrowDown" ? 1 : -1) + list.length) % list.length;
      list[at]?.focus({ preventScroll: true });
    });
    document.addEventListener("scroll", e => {
      if(!current || current.contains(e.target))return;
      const now=trigger.getBoundingClientRect();
      if(!anchorPosition || Math.abs(now.left-anchorPosition.left)>1 || Math.abs(now.top-anchorPosition.top)>1)close();
    }, true);
    window.addEventListener("resize", () => close(true));
    new MutationObserver(() => { if (current && (!current.isConnected || !trigger?.isConnected)) { close(); $("#conv-head h1")?.focus({ preventScroll: true }); } }).observe(document.body, { childList: true, subtree: true });
  }
  function capture(root) {
    return current && root.contains(current) ? {id:current.id, action:document.activeElement?.dataset?.msgAct, focused:current.contains(document.activeElement)} : null;
  }
  function restore(state) {
    if (!state) return;
    close();
    const button = $$("[data-menu]").find(b => b.dataset.menu === state.id);
    if (button) { open(button,state.focused); if (state.focused && state.action) items().find(b => b.dataset.msgAct === state.action)?.focus({preventScroll:true}); }
  }
  return { install, close, capture, restore };
})();
const Tooltips = (() => {
  let trigger = null, tip = null, showTimer, closeTimer, hovered = false, overTip = false, focused = false, suppressed = null, warm = 0;
  const eligible = node => node instanceof Element ? node.closest("[data-tooltip]") : null;
  const label = el => el.dataset.tooltip || el.getAttribute("aria-label") || el.textContent.trim();
  function hide() {
    clearTimeout(showTimer); clearTimeout(closeTimer); showTimer=null;closeTimer=null;
    if (tip) { const ids = (trigger?.getAttribute("aria-describedby") || "").split(/\s+/).filter(x => x && x !== tip.id); if (ids.length) trigger?.setAttribute("aria-describedby", ids.join(" ")); else trigger?.removeAttribute("aria-describedby"); tip.remove(); tip = null; }
  }
  function place() {
    if (!tip || !trigger?.isConnected) return hide();
    const r = trigger.getBoundingClientRect(), v = window.visualViewport, x = v?.offsetLeft || 0, y = v?.offsetTop || 0, w = v?.width || innerWidth, h = v?.height || innerHeight;
    tip.style.maxWidth = `${Math.min(280, w - 16)}px`;
    const t = tip.getBoundingClientRect();
    tip.style.left = `${Math.max(x + 8, Math.min(r.left + (r.width - t.width) / 2, x + w - t.width - 8))}px`;
    const top = r.bottom + 8 + t.height <= y + h - 8 ? r.bottom + 8 : r.top - t.height - 8;
    tip.style.top = `${Math.max(y + 8, Math.min(top, y + h - t.height - 8))}px`;
  }
  function show() {
    showTimer=null;
    if (!trigger?.isConnected || suppressed === trigger || !(hovered || focused)) return;
    tip = document.createElement("div"); tip.id = "magnus-tooltip"; tip.className = "tooltip"; tip.role = "tooltip"; tip.textContent = label(trigger);
    document.body.append(tip); trigger.setAttribute("aria-describedby", [trigger.getAttribute("aria-describedby"), tip.id].filter(Boolean).join(" "));
    tip.onpointerenter = () => { overTip = true; clearTimeout(closeTimer); };
    tip.onpointerleave = () => { overTip = false; scheduleClose(); };
    place(); warm = Date.now() + 1000;
  }
  function scheduleClose() {
    clearTimeout(closeTimer);
    if (!hovered && !focused && !overTip) closeTimer = setTimeout(() => { hide(); if (suppressed === trigger) suppressed = null; }, 150);
  }
  function enter(el, focus, touch = false) {
    if (!el || touch) return;
    if (el !== trigger) { hide(); trigger = el; hovered = false; overTip = false; focused = false; }
    if (focus) focused = true; else hovered = true;
    clearTimeout(closeTimer); clearTimeout(showTimer);
    if (!tip && suppressed !== el) showTimer = setTimeout(show, focus || warm > Date.now() ? 80 : 350);
  }
  function dismiss() { if (trigger && (hovered || focused)) suppressed = trigger; hide(); }
  function decorate(root) {
    for (const el of [root, ...root.querySelectorAll("[title]")]) {
      if (!el.hasAttribute?.("title")) continue;
      const title = el.getAttribute("title"); if (title) { el.dataset.tooltip = title; if (el.matches("button, a") && !el.getAttribute("aria-label") && !el.textContent.trim()) el.setAttribute("aria-label", title); }
      el.removeAttribute("title");
    }
  }
  function install() {
    decorate(document.body);
    document.addEventListener("pointerover", e => { const el = eligible(e.target); if (el && !el.contains(e.relatedTarget)) enter(el, false, e.pointerType === "touch"); });
    document.addEventListener("pointerout", e => { if (trigger?.contains(e.target) && !trigger.contains(e.relatedTarget)) { hovered = false; if (!focused && suppressed === trigger) suppressed = null; scheduleClose(); } });
    document.addEventListener("focusin", e => enter(eligible(e.target), true));
    document.addEventListener("focusout", e => { if (trigger?.contains(e.target) && !trigger.contains(e.relatedTarget)) { focused = false; if (!hovered && suppressed === trigger) suppressed = null; scheduleClose(); } });
    document.addEventListener("click", dismiss, true);
    document.addEventListener("keydown", e => { if (e.key === "Escape" && (tip || showTimer)) dismiss(); });
    document.addEventListener("scroll", dismiss, true); window.addEventListener("resize", place); window.visualViewport?.addEventListener("resize", place);
    new MutationObserver(records => {
      for (const r of records) { if (r.type === "attributes") decorate(r.target); for (const n of r.addedNodes || []) if (n.nodeType === 1) decorate(n); }
      if (trigger && !trigger.isConnected) { hide(); trigger = null; }
    }).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["title"] });
  }
  return { install, dismiss };
})();
function loadMarkdownImages(root) {
  drawUiBlocks(root);
  for (const img of root.querySelectorAll("img[data-prose-image]")) {
    if (img._loading) continue; img._loading = true;
    const path = img.dataset.proseImage;
    if (!/^\/v1\/files\/[^/?#]+$/.test(path)) continue;
    Magnus.dataUrl(path).then(src => { if (img.isConnected) img.src = src; }).catch(() => { img.alt = `${img.alt || "Image"} (unavailable)`; });
  }
}
// An answer still being written: prose is redrawn, while a block keeps its
// frame, so what is already drawn stays put as more arrives.
function patchLive(el, text) {
  const parts = UiSpec.split(text);
  [...el.children].slice(parts.length).forEach(c => c.remove());
  parts.forEach((p, i) => {
    const html = p.kind === "text";
    let node = el.children[i];
    if (!node || node.classList.contains("ui-block") === html) {
      const made = document.createElement("div");
      if (node) node.replaceWith(made); else el.append(made);
      node = made;
    }
    if (html) { node.innerHTML = Compose.markdown(p.text); return; }
    node.className = "ui-block";
    Object.assign(node.dataset, { uiKind: p.kind, uiBody: p.body, uiClosed: p.closed ? "1" : "0" });
  });
}
// A spec block in the agent's words is drawn in a frame of its own: Compose
// makes the document, and the frame's scripts stay off. HTML the agent wrote
// runs in a sandbox with an opaque origin, under a policy set before it.
function drawUiBlocks(root) {
  if ((root.ownerDocument || root) !== document) return;
  root.querySelectorAll(".ui-block").forEach((block, n) => {
    const body = block.dataset.uiBody, closed = block.dataset.uiClosed === "1", kind = block.dataset.uiKind;
    if (block._drawn === body + closed) return;
    block._drawn = body + closed;
    if (kind === "html") {
      if (!closed) { block.innerHTML = `<div class="small faint">Writing…</div>`; return; }
      block.innerHTML = `<iframe class="composed ui-html" sandbox="allow-scripts" title="Made by Magnus"></iframe>`;
      Magnus.sandboxUrl(`<!doctype html>${body}`).then(url => { if (block.firstChild?.matches?.("iframe")) block.firstChild.src = url; }, e => { block.innerHTML = `<div class="small bad">This could not be drawn: ${esc(e.message)}</div>`; });;
      return;
    }
    let view;
    try { view = closed ? UiSpec.compile(body) : UiSpec.compilePartial(body); }
    catch (e) { block.innerHTML = `<div class="small bad">This could not be drawn: ${esc(e.message)}</div>`; return; }
    if (!view.length) { block.innerHTML = `<div class="small faint">Drawing…</div>`; return; }
    if (closed) view.filter(c => c.type === "app").forEach(c => drawAppView(block, c.view));
    if (view.every(c => c.type === "app")) return;
    if (!block.firstChild?.matches?.("iframe")) block.innerHTML = `<iframe class="composed" sandbox="allow-same-origin" title="What Magnus showed" data-sized=""></iframe>`;
    const frame = block.firstChild;
    const drawn = JSON.stringify(view);
    if (block._view === drawn) return;
    block._view = drawn;
    const files = {};
    const paint = () => { frame.srcdoc = Compose.documentFor(view, files, fontCss(), undefined, ...look()); };
    paint();
    frame.onload = () => { sizeComposedFrame(frame); if (closed) wireBlock(frame, view, block, n); };
    if (closed) view.filter(c => c.placeholder).forEach(c => waitForImage(c.placeholder, files, block, paint));
  });
}
// What is pressed is read again by the server from what the task holds.
function wireBlock(frame, view, block, fence) {
  const task = block.closest("[data-task]")?.dataset.task;
  if (!task) return;
  const at = kind => view.flatMap((c, i) => c.type === kind ? [i] : []);
  const send = (index, values, b) => { b.disabled = true; api("POST", `/v1/tasks/${enc(task)}/actions/${index}`, { fence, values }).then(() => toast("Started as a new request")).catch(e => { b.disabled = false; toast(e.message); }); };
  const doc = frame.contentDocument;
  doc?.querySelectorAll("button.action:not(.form-send)").forEach((b, n) => b.addEventListener("click", () => send(at("action")[n], {}, b)));
  doc?.querySelectorAll(".deck-present").forEach(b => b.addEventListener("click", () => presentDeck(frame)));
  doc?.querySelectorAll(".uiform").forEach((form, n) => form.querySelector(".form-send").addEventListener("click", e => {
    if (![...form.querySelectorAll("input,select,textarea")].every(i => i.reportValidity())) return;
    send(at("form")[n], Object.fromEntries([...form.querySelectorAll("[name]")].map(i => [i.name, i.value])), e.currentTarget);
  }));
}
// Present mode: the same frame full screen, one slide at a time.
function presentDeck(frame) {
  const doc = frame.contentDocument, slides = [...doc.querySelectorAll(".slide")];
  if (!slides.length) return;
  let at = 0;
  const show = () => slides.forEach((s, i) => s.classList.toggle("on", i === at));
  const end = () => { doc.body.classList.remove("presenting"); doc.removeEventListener("keydown", key); document.removeEventListener("keydown", key); document.removeEventListener("fullscreenchange", left); if (document.fullscreenElement === frame) document.exitFullscreen().catch(() => {}); sizeComposedFrame(frame); };
  const left = () => { if (document.fullscreenElement !== frame) end(); };
  const key = e => { if (e.key === "ArrowRight" || e.key === " ") { at = Math.min(at + 1, slides.length - 1); show(); } else if (e.key === "ArrowLeft") { at = Math.max(at - 1, 0); show(); } else if (e.key === "Escape") end(); };
  doc.body.classList.add("presenting");
  show();
  doc.addEventListener("keydown", key); document.addEventListener("keydown", key); document.addEventListener("fullscreenchange", left);
  frame.requestFullscreen?.().catch(() => {});
}
// A connector's view, in the same sandbox as the agent's own HTML. It talks
// over postMessage; a tool it asks for starts a new request.
async function drawAppView(block, id) {
  if (block.querySelector(`iframe[data-view="${CSS.escape(id)}"]`)) return;
  const frame = document.createElement("iframe");
  Object.assign(frame, { className: "composed ui-html", title: "A connector's view" });
  frame.setAttribute("sandbox", "allow-scripts");
  frame.dataset.view = id;
  block.append(frame);
  let v;
  try { v = await api("GET", `/v1/ui/apps/${enc(id)}`); }
  catch { frame.replaceWith(Object.assign(document.createElement("div"), { className: "small bad", textContent: "This view is no longer available." })); return; }
  const send = m => frame.contentWindow?.postMessage({ jsonrpc: "2.0", ...m }, "*");
  const onMessage = e => {
    if (!frame.isConnected) return window.removeEventListener("message", onMessage);
    if (e.source !== frame.contentWindow || !e.data || e.data.jsonrpc !== "2.0") return;
    const { id: rid, method, params } = e.data;
    if (method === "ui/initialize") send({ id: rid, result: { protocolVersion: params?.protocolVersion, hostInfo: { name: "Magnus", version: "1" }, hostCapabilities: {}, hostContext: {} } });
    else if (method === "ui/notifications/initialized") {
      send({ method: "ui/notifications/tool-input", params: { arguments: v.input } });
      send({ method: "ui/notifications/tool-result", params: { content: [{ type: "text", text: v.text }], structuredContent: v.structured ?? undefined } });
    } else if (method === "tools/call") api("POST", `/v1/ui/apps/${enc(id)}/request`, { tool: String(params?.name), arguments: params?.arguments ?? {} })
      .then(() => { toast("Started as a new request."); send({ id: rid, result: { content: [{ type: "text", text: "Started as a new request." }] } }); })
      .catch(err => send({ id: rid, error: { code: -32000, message: err.message || String(err) } }));
    else if (rid !== undefined) send({ id: rid, error: { code: -32601, message: "Not available here." } });
  };
  window.addEventListener("message", onMessage);
  frame.src = await Magnus.sandboxUrl(`<!doctype html>${v.html}`);
}
// An image being made: asked after until it is there, then drawn in place.
async function waitForImage(id, files, block, paint) {
  for (let tries = 0; tries < IMAGE_POLLS && block.isConnected; tries++) {
    try {
      const src = await Magnus.dataUrl(`/v1/ui/images/${enc(id)}`);
      if (src && !/^data:[^,]*,$/.test(src)) { files[id] = { image: src }; paint(); return; }
    } catch { files[id] = { failed: true }; paint(); return; }
    await new Promise(r => setTimeout(r, IMAGE_POLL_MS));
  }
}
const IMAGE_POLLS = 120, IMAGE_POLL_MS = 3000;
// Says what this client can draw, so the agent writes only that.
async function loadUiCatalog() {
  try {
    const c = await api("GET", "/v1/ui/catalog");
    UiSpec.setCatalog(c);
    await api("PUT", "/v1/ui/catalog", { version: c.version, elements: c.elements.map(e => e.name) });
  } catch {/* Drawn as "could not be drawn" until it loads. */}
}
function wireMarkdown(root) {
  if (root._markdownWired) return; root._markdownWired = true;
  loadMarkdownImages(root);
  root.addEventListener("click", async e => {
    const copy = e.target.closest("[data-code-copy]"), link = e.target.closest("[data-prose-link]");
    if (copy) { e.preventDefault(); try { await copyText(copy.closest(".code-block").querySelector("code").textContent); toast("Code copied."); } catch (err) { toast(err.message); } }
    if (link) { e.preventDefault(); const url = Compose.safeUrl(link.getAttribute("href")); if (url) Magnus.openExternal(url).catch(err => toast(err.message)); }
  });
}

// What works wherever it is shown, set once for the page.
function wireDocument() {
  document.addEventListener("toggle",reportOpenedSoon,true);
  document.addEventListener("scroll",reportOpenedSoon,{capture:true,passive:true});
  window.addEventListener("popstate",()=>{
    if (App.currentHash!==location.hash || !App.workspace) return;
    if (history.state?.work) openPanel(history.state.work,{focus:true}); else closePanel(false);
  });
  document.addEventListener("click", e => { const b=e.target.closest("[data-attach-open]"); if (b) b.closest(".attachment-picker").querySelector("input").click(); });
  Menus.install(); Tooltips.install(); wireMarkdown(document);
  new MutationObserver(() => drawUiBlocks(document)).observe(document.body, { childList: true, subtree: true });
  const style = document.createElement("style"); style.textContent = Compose.markdownStyles; document.head.append(style);
  document.addEventListener("load", e => { if (e.target.matches?.("iframe.composed")) { try { wireMarkdown(e.target.contentDocument); sizeComposedFrame(e.target); } catch { /* frame unavailable */ } } }, true);
  new MutationObserver(()=>{for(const [frame,sizing] of frameSizing)if(!frame.isConnected){sizing.observer.disconnect();frame._frameActions?.dispose();delete frame._frameActions;frameSizing.delete(frame);}}).observe(document.body,{childList:true,subtree:true});
  // Anything that names an item opens it beside the conversation, without
  // leaving it.
  document.addEventListener("click", e => {
    const o = e.target.closest("[data-open]");
    if (!o || e.button !== 0 || e.ctrlKey || e.metaKey) return;
    e.preventDefault();
    // From Settings, Trash or a developer view: back to the conversation,
    // with it open beside.
    Panel.opener=o; Panel.focusNext=true;
    if (!App.workspace) { App.openAfter = o.dataset.open; location.hash = "#/home"; return; }
    openPanel(o.dataset.open,{history:true,focus:true,opener:o});
  });
  // A decision is made from its card wherever the card is shown: in its
  // conversation, beside it, or under Needs you.
  document.addEventListener("click", e => {
    const b = e.target.closest("button[data-item]");
    if (b) respond(b, b.closest("article") || document);
  });
  // A library file downloads from wherever it is listed; a move control
  // moves its item into a project or out, wherever it is shown.
  document.addEventListener("click", async e => {
    const b = e.target.closest("[data-file-download]");
    if (!b) return;
    e.preventDefault();
    try { toast(`Saved ${await Magnus.download(`/v1/files/${enc(b.dataset.fileDownload)}`, b.dataset.name)}`); } catch (err) { toast(err.message); }
  });
  document.addEventListener("click", async e => {
    const b = e.target.closest("[data-verdict], [data-verdict-save]");
    if (!b) return;
    const bar = b.closest("[data-verdict-for]"), id = bar.dataset.verdictFor;
    const verdict = b.dataset.verdict || $("[data-verdict].on", bar)?.dataset.verdict;
    if (!verdict) { toast("Pick right, partly or wrong first."); return; }
    const box = $("textarea", bar);
    try {
      await api("POST", `/v1/tasks/${enc(id)}/verdict`, { verdict, note: box.value });
      delete drafts[box.dataset.draft];
      $$("[data-verdict]", bar).forEach(x => { const on = x.dataset.verdict === verdict; x.classList.toggle("on", on); x.setAttribute("aria-pressed", on); });
      toast(b.dataset.verdictSave !== undefined ? "Note saved." : "Recorded.");
      await refresh();
    } catch (err) { toast(err.message); }
  });
  // Delete, wherever an item offers it: to the trash, where it can be
  // restored until it is gone for good.
  document.addEventListener("click", async e => {
    const b = e.target.closest("[data-trash]");
    if (!b) return;
    if (!await ask(`${b.textContent}? It moves to the trash, where you can restore it until it is gone for good.`, b.textContent)) return;
    b.disabled = true;
    try {
      await api("POST", "/v1/trash", { item: b.dataset.trash });
      toast("Moved to the trash.");
      if (b.closest("#detail")) closePanel();
      if (b.dataset.then) location.hash = b.dataset.then; else await refresh();
    } catch (err) { toast(err.message); b.disabled = false; }
  });
  document.addEventListener("change", async e => {
    const sel = e.target.closest("select[data-move]");
    if (!sel) return;
    try {
      await api("POST", "/v1/moves", { item: sel.dataset.move, project: sel.value || null });
      toast(sel.value ? `Moved to ${sel.selectedOptions[0].textContent}.` : "Taken out of the project.");
      await refresh();
    } catch (err) { toast(err.message); }
  });
}

// A view of the workspace (a conversation) keeps the panel beside it; any
// other view (Settings, Trash, the developer's) has the page to itself.
function route(re, view, workspace = false) { App.routes.push([re, view, workspace]); }

// A Delete button for an item: to the trash, then to `then` (or redrawn).
const trashButton = (item, label, then = "") => `<button data-trash="${esc(item)}"${then ? ` data-then="${esc(then)}"` : ""}>${esc(label)}</button>`;

// ---------- Files and projects ----------

const kb = n => `${Math.max(1, Math.round(n / 1024))} KB`;
// A file wherever it is listed: its name opens it, Download saves it.
function fileChip(f) {
  return `<span class="fchip"><a href="#/file/${enc(f.id)}" data-open="file:${esc(f.id)}">${esc(f.name)}</a> <span class="small faint">${esc(kb(f.size))}</span> <button class="link small" data-file-download="${esc(f.id)}" data-name="${esc(f.name)}" aria-label="Download ${esc(f.name)}">Download</button></span>`;
}
// One tap records the person's verdict on an outcome; a note is optional.
const VERDICTS = [["right", "Right"], ["partly", "Partly"], ["wrong", "Wrong"], ["undone", "I had to undo it"]];
const ENDED = ["completed", "failed", "cancelled"];
function verdictBar(taskId, v) {
  return `<div class="verdict" data-verdict-for="${esc(taskId)}"><span class="vq">How did Magnus do?</span>
    ${VERDICTS.map(([k, label]) => `<button class="${v?.verdict === k ? "on" : ""}" data-verdict="${k}" aria-pressed="${v?.verdict === k}">${label}</button>`).join("")}
    <details class="vnote" data-key="note:${esc(taskId)}"${v?.note ? " open" : ""}><summary class="small">${v?.note ? "Note" : "Add a note"}</summary>
    <textarea rows="2" aria-label="Note" data-draft="note:${esc(taskId)}">${esc(v?.note || "")}</textarea> <button data-verdict-save>Save note</button></details></div>`;
}
// Where a library file came from, as a link.
const fileFrom = f => f.origin.kind === "task" ? `<a href="#/task/${enc(f.origin.task_id)}" data-open="task:${esc(f.origin.task_id)}">${esc(f.origin.title)}</a>`
  : f.origin.kind === "chat" ? `<a href="#/chat/${enc(f.origin.thread)}">Chat: ${esc(f.origin.title || "conversation")}</a>` : `<span class="faint">Uploaded to Files</span>`;
function readFile(file) {
  return new Promise((ok, no) => {
    const r = new FileReader();
    r.onload = () => ok({ name: file.name, media_type: file.type || "application/octet-stream", data_b64: String(r.result).slice(String(r.result).indexOf(",") + 1) });
    r.onerror = () => no(new Error(`Could not read ${file.name}`));
    r.readAsDataURL(file);
  });
}
// Upload chosen files to the library (into a project, or in a thread).
async function upload(list, where = {}) {
  const out = [];
  for (const file of list) out.push(await api("POST", "/v1/files", { ...(await readFile(file)), ...where }));
  return out;
}
function projectOptions(projects, current, none = "No project") {
  return `<option value="">${esc(none)}</option>${projects.map(p => `<option value="${esc(p.id)}"${p.id === current ? " selected" : ""}>${esc(p.name)}</option>`).join("")}`;
}
// A control that moves an item into one of the person's projects, or out.
function moveControl(item, current, projects, compact = false) {
  return `<label class="small move"><span class="${compact ? "sr" : "muted"}">Project</span> <select data-move="${esc(item)}" aria-label="Project">${projectOptions(projects, current?.id)}</select></label>`;
}
// A file input behind a button; `onFiles` gets the chosen files.
function attachButton(label = "Add files", iconOnly = false) {
  return `<span class="attachment-picker"><button type="button" class="${iconOnly ? "icon-btn" : "button"} attach" data-attach-open aria-label="${esc(label)}" title="${esc(label)}">${iconOnly ? icon("attach") + `<span class="sr">${esc(label)}</span>` : esc(label)}</button><input type="file" multiple hidden data-attach></span>`;
}

// A file shown where it is mentioned: a glimpse of it (an image, or the
// first lines of a text file), its name to open it beside the conversation,
// and Download. Glimpses are fetched once, by the app itself.
const GLIMPSE_IMAGE = /^image\/(png|jpeg|gif|webp)$/;
const glimpses = {};
function filePreview(f) {
  const can = (GLIMPSE_IMAGE.test(f.media_type) && f.size <= 4 << 20) || (Compose.TEXT.test(f.media_type) && f.size <= 256 << 10);
  const g = !can ? `<span class="small faint">${esc(f.media_type)}</span>` : glimpses[f.id] ?? `<span class="small faint">Loading a preview…</span>`;
  return `<div class="fprev" data-preview="${esc(f.id)}"${can && !(f.id in glimpses) ? ` data-glimpse="${esc(f.media_type)}"` : ""}>
    <a class="glimpse" href="#/file/${enc(f.id)}" data-open="file:${esc(f.id)}" aria-hidden="true" tabindex="-1">${g}</a>${fileChip(f)}</div>`;
}
async function loadGlimpses(root) {
  const want = $$("[data-glimpse]", root).filter(el => !(el.dataset.preview in glimpses));
  if (!want.length) return;
  for (const el of want) glimpses[el.dataset.preview] = undefined;
  await Promise.all(want.map(async el => {
    const id = el.dataset.preview, type = el.dataset.glimpse;
    try {
      const url = await Magnus.dataUrl(`/v1/files/${enc(id)}`, type);
      glimpses[id] = GLIMPSE_IMAGE.test(type) ? `<img alt="" src="${esc(url)}">` : `<pre>${esc(decodeText(url).split("\n").slice(0, 8).join("\n"))}</pre>`;
    } catch { glimpses[id] = `<span class="small faint">No preview</span>`; }
  }));
  await Promise.all([App.redraw?.(), drawPanel()]);
}

async function go() {
  App.timers.forEach(clearInterval); App.timers = [];
  App.leave.forEach(f => { try { f(); } catch { /* ignore */ } }); App.leave = [];
  const h = location.hash || "#/home", routeFocus=App.routeFocus;App.routeFocus=null;
  queueMicrotask(()=>Workspace.update());
  App.currentHash=location.hash;
  // A view still loading when the person moves on stops where it is: only
  // the latest navigation draws. Each view gets a main of its own, so one
  // still loading (and its handlers) only ever touches the main it left,
  // never the next view's.
  const seq = ++App.seq;
  const left = $("#main"), main = left.cloneNode(false);
  left.replaceWith(main);
  for (const [re, view, workspace] of App.routes) {
    const m = h.match(re);
    if (m) {
      App.workspace = workspace;
      if (!workspace) { App.thread = null; closePanel(); }
      sidebar();
      try {
        await view(main, ...m.slice(1).map(x => x === undefined ? x : decodeURIComponent(x)));
        if (seq===App.seq) App.currentHash=location.hash;
        if (seq===App.seq && workspace && history.state && "work" in history.state) {
          if (history.state.work) await openPanel(history.state.work); else closePanel();
        }
        if(seq===App.seq && routeFocus && [document.body,routeFocus.opener,$("[data-open-drawer]")].includes(document.activeElement)) {
          const target=routeFocus.composer ? $("#msg",main) : $("h1,h2",main) || main;
          if(target){if(!target.matches("input,textarea"))target.tabIndex=-1;target.focus({preventScroll:true});}
        }
      }
      catch (e) { if (seq === App.seq) paint(main, `<div class="banner bad">${esc(e.message)}</div>`); }
      return;
    }
  }
  location.hash = "#/home";
}

// ---------- sign-in ----------

async function signInScreen(note) {
  document.body.innerHTML = `<div class="signin"><h1>Magnus</h1><p class="muted">Sign in to your server.</p><div class="card" id="si"></div></div>`;
  const box = $("#si");
  // Signed in. Pair this computer for the public name once, on the private
  // address; a failure is said and sign-in still stands.
  async function enter(person, data) {
    if (data.person) localStorage.setItem("magnus.person", data.person);
    if (Magnus.pairDevice && data.public_url) {
      try { await Magnus.pairDevice(data.public_url); }
      catch (err) { return ask("paired", { ...data, error: err.message || String(err), who: person }); }
    }
    App.me = person;
    App.appliance = data.url || location.origin;
    boot();
  }
  const ask = async (stage, data = {}) => {
    if (stage === "address") {
      paint(box, `${note ? `<p class="small" style="color:var(--bad)">${esc(note)}</p>` : ""}
        <form id="f"><label for="addr">Server address</label>
        <input id="addr" style="width:100%" placeholder="https://magnus.local:7443" value="${esc(data.url || localStorage.getItem("magnus.address") || "")}" required>
        <p class="small muted">The owner gives you this address.</p>
        <div class="row" style="margin-top:14px"><button class="primary">Continue</button></div></form>`);
      $("#f").onsubmit = async e => {
        e.preventDefault();
        const url = $("#addr").value.trim().replace(/\/+$/, "");
        try {
          const c = await Magnus.connect(url);
          localStorage.setItem("magnus.address", url);
          if (c.status === "trusted") ask("method", { url });
          else if (c.status === "needs_device") ask("needs_device", { url });
          else ask("pin", { url, ...c });
        } catch (err) { note = err.message || String(err); ask("address", { url }); }
      };
    } else if (stage === "pin") {
      const changed = data.status === "changed";
      paint(box, `<h3>${changed ? "The server's certificate changed" : "Check the server"}</h3>
        <p class="small">${changed ? "This is not the certificate you trusted before. Continue only if the owner told you it was replaced." : "This is the first connection to this server. Compare this fingerprint with the one the owner sees on the server."}</p>
        <div class="fp" id="fp">${esc(data.fingerprint)}</div>
        <div class="row" style="margin-top:14px"><button class="${changed ? "danger" : "primary"}" id="ok">They match</button><button id="no">Cancel</button></div>`);
      $("#ok").onclick = async () => { await Magnus.trust(data.url, data.fingerprint); ask("method", data); };
      $("#no").onclick = () => ask("address", data);
    } else if (stage === "needs_device") {
      paint(box, `<h3>This computer is not paired</h3>
        <p class="small">magnus.pyrallon.ai accepts only a computer that was paired with the server. Sign in once on the server's private address, the one on your network or Tailscale. This computer is paired then, and this address works after that, from anywhere.</p>
        <div class="row" style="margin-top:14px"><button id="back">Use a private address</button></div>`);
      $("#back").onclick = () => ask("address", {});
    } else if (stage === "paired") {
      paint(box, `<h3>This computer was not paired</h3>
        <p class="small" style="color:var(--bad)" role="alert">${esc(data.error)}</p>
        <p class="small muted">You are signed in on this network. ${esc(data.public_url || "")} will not accept this computer until pairing works.</p>
        <div class="row" style="margin-top:14px"><button class="primary" id="go">Continue</button></div>`);
      $("#go").onclick = () => { App.me = data.who; App.appliance = data.url || location.origin; boot(); };
    } else if (stage === "method") {
      paint(box, `<p class="small muted">${esc(data.url || location.origin)}</p><p class="small muted">Connecting…</p>`);
      try {
        const m = await Magnus.signInMethod();
        ask("password", { ...data, public_url: m.public_url, label: m.method === "oidc" ? m.label : undefined });
      } catch (err) { note = err.message || String(err); ask(Magnus.kind === "app" ? "address" : "password", data); }
    } else if (stage === "forgot") {
      forgotPasswordStep(box, data, () => ask("password", { ...data, error: undefined }));
    } else if (stage === "browser") {
      // The provider's own page in the person's browser: for a passkey, the
      // first time, or when their account needs finishing there.
      const back = () => ask("password", { ...data, message: undefined });
      if (Magnus.providerSignIn) {
        paint(box, `<p class="small muted">${esc(data.url || location.origin)}</p>
          <p>${esc(data.message || `Sign in on the ${data.label} page that just opened in your browser, then come back here.`)}</p>
          <p class="small muted" id="wait" role="status">Waiting for you to sign in…</p>
          <div class="row" style="margin-top:14px"><button class="link" id="cancel">Cancel</button></div>`);
        // A cancelled sign-in stops waiting; one the browser still finishes is not kept.
        let cancelled = false;
        $("#cancel").onclick = () => { cancelled = true; Magnus.providerSignInCancel?.().catch(() => {}); back(); };
        try {
          const r = await Magnus.providerSignIn(data.person);
          if (cancelled) return;
          return enter(r.person, data);
        } catch (err) { if (!cancelled) ask("password", { ...data, message: undefined, error: err.message || String(err) }); return; }
      }
      let s;
      try { s = await Magnus.deviceStart(); } catch (err) { return ask("password", { ...data, error: err.message || String(err) }); }
      const link = s.verification_uri_complete || s.verification_uri;
      let stop = false;
      paint(box, `<p class="small muted">${esc(data.url || location.origin)}</p>
        <p>${esc(data.message || `Sign in on the ${s.label} page that just opened.`)}</p>
        <p class="small">Check that it shows this code:</p><div class="fp" id="code">${esc(s.user_code)}</div>
        <div class="row" style="margin-top:14px"><button data-url="${esc(link)}" id="open">Open the sign-in page again</button><button class="link" id="cancel">Cancel</button></div>
        <p class="small muted" id="wait" role="status">Waiting for you to approve…</p>`);
      $("#open").onclick = () => Magnus.openExternal(link).catch(() => {});
      $("#cancel").onclick = () => { stop = true; back(); };
      Magnus.openExternal(link).catch(() => {});
      const until = Date.now() + s.expires_in * 1000;
      const every = Math.max(1, s.interval) * 1000;
      while (!stop && Date.now() < until) {
        await new Promise(r => setTimeout(r, every));
        if (stop) return;
        try {
          const r = await Magnus.devicePoll(s.flow);
          if (r.pending) continue;
          return enter(r.person, data);
        } catch (err) { return ask("password", { ...data, message: undefined, error: err.message || String(err) }); }
      }
      if (!stop) ask("password", { ...data, message: undefined, error: "The sign-in code expired. Start again." });
    } else {
      // Username and password, then the code from an authenticator app if
      // the person uses one. The password stays in this closure only.
      const coding = stage === "code" || stage === "machine";
      const machine = stage === "machine";
      paint(box, `<form id="f"><p class="small muted">${esc(data.url || location.origin)}</p>
        ${data.error ? `<p class="small" style="color:var(--bad)" role="alert">${esc(data.error)}</p>` : ""}
        ${machine
          ? `<label for="otp">Enter the code \`magnus sign-in-code\` printed on the server</label><input id="otp" style="width:100%" autocomplete="one-time-code" maxlength="16" required autofocus>`
          : coding
          ? `<label for="otp">Enter the 6-digit code from your authenticator app</label><input id="otp" style="width:100%" inputmode="numeric" autocomplete="one-time-code" pattern="[0-9]*" maxlength="6" required autofocus>`
          : `<label for="who">Username or email</label><input id="who" style="width:100%" autocomplete="username" value="${esc(data.person || localStorage.getItem("magnus.person") || "")}" required>
        <label for="pw">Password</label><input id="pw" type="password" style="width:100%" autocomplete="current-password" required>`}
        <div class="row" style="margin-top:16px"><button class="primary" id="go">${coding ? "Continue" : "Sign in"}</button>${coding ? `<button type="button" class="link" id="again">Back</button>` : data.label ? `<button type="button" class="link" id="passkey">Use a passkey instead</button>` : `<button type="button" class="link" id="forgot">Forgot password?</button>`}${Magnus.kind === "app" && !coding ? `<button type="button" class="link" id="back">Other server</button>` : ""}</div></form>`);
      $("#back")?.addEventListener("click", () => ask("address", data));
      $("#again")?.addEventListener("click", () => ask("password", { ...data, error: undefined, password: undefined }));
      $("#passkey")?.addEventListener("click", () => ask("browser", { ...data, person: $("#who").value.trim() || undefined, error: undefined }));
      $("#forgot")?.addEventListener("click", () => ask("forgot", { ...data, person: $("#who").value.trim() || undefined, error: undefined }));
      $("#f").onsubmit = async e => {
        e.preventDefault();
        const person = coding ? data.person : $("#who").value.trim();
        const password = coding ? data.password : $("#pw").value;
        $("#go").disabled = true;
        try {
          const typed = coding ? $("#otp").value.trim() : undefined;
          const r = await Magnus.signIn(person, password, machine ? undefined : typed, machine ? typed : undefined);
          if (r?.machine_code_needed) return ask("machine", { ...data, person, password, error: r.error });
          if (r?.code_needed) return ask("code", { ...data, person, password, error: coding ? r.error : undefined });
          if (r?.browser) return ask("browser", { ...data, person, password: undefined, message: r.error });
          return enter(r, { ...data, person });
        } catch (err) { ask("password", { ...data, person, password: undefined, error: err.message || String(err) }); }
      };
    }
  };
  if (Magnus.kind === "web") ask("method", { url: location.origin });
  else ask("address");
}

// ---------- background: inbox, notifications, versions ----------

function seenKey(k) { return `magnus.${k}.${App.appliance}.${App.me?.id}`; }
function load(k, d) { try { return JSON.parse(localStorage.getItem(seenKey(k))) ?? d; } catch { return d; } }
function save(k, v) { try { localStorage.setItem(seenKey(k), JSON.stringify(v)); } catch { /* private window */ } }

async function pollInbox() {
  try {
    const items = await Resources.read("Needs you", "/v1/inbox", "items");
    if (!items) return;
    App.inbox = items;
    // Interrupt only for decisions a task is waiting on; the rest wait in
    // the inbox until the person looks (U3).
    const told = new Set(load("told", []));
    const wants = i => i.blocking ? App.prefs?.notify_blocking !== false : App.prefs?.notify_batched === true;
    // Each item is considered once, so turning a kind on later does not
    // bring back everything already waiting.
    const fresh = needsMe().filter(i => !told.has(i.id));
    for (const i of fresh.filter(wants)) Notices.show({ key: `inbox:${i.id}`, title: i.blocking ? "A task needs you" : "Something for you", body: i.summary, system: i.summary, href: "#/inbox" });
    if (fresh.length) save("told", [...told, ...fresh.map(i => i.id)].slice(-500));
    sidebar();
  } catch (e) {
    // A dead session must land on sign-in once. Calling start() again
    // restores the same session and boots forever.
    if (e.status === 401 && App.me) { stop(); signInScreen("Your session ended. Sign in again."); }
  }
}

async function pollVersions() {
  try {
    const v = await Resources.read("Agent changes", "/v1/versions");
    if (!v) return;
    App.versions = v;
    const seen = load("version", null);
    if (seen === null) { save("version", v.latest_seq ?? 0); return; }
    const news = v.changes.filter(c => c.seq > seen);
    const told = load("version-told", 0);
    if (news.length && (v.latest_seq ?? 0) > told) {
      const said = news.map(describeChange).join("; ");
      if (App.prefs?.notify_agent_changes !== false) Notices.show({ key: `version:${v.latest_seq}`, title: "The agent changed", body: said, system: said, href: "" });
      save("version-told", v.latest_seq);
    }
  } catch { /* shown above the conversation */ }
}

function describeChange(c) {
  const what = { harness: "harness", instructions: "instructions", model: "model", playbook: "playbook" }[c.what] || c.what.replace("policy:", "policy for ");
  return `${what} is now ${c.value}`;
}

// ---------- the workspace's objects ----------

// Tasks, schedules, conversations and projects, read together with the
// inbox; then every view that shows them is drawn again from what was read.
async function refresh() {
  const get = (path, k) => Resources.read(({tasks:"Tasks",schedules:"Schedules",threads:"Conversations",projects:"Projects"})[k],path,k);
  // The person's computers, for the conversation's head: whether one is in use, and by which agent.
  const computers = api("GET", "/v1/computers").then(r => { App.computers = r.computers; followDesktopLook(); }, () => { /* Without a computer service there is nothing to show. */ });
  const [tasks, schedules, threads, projects, health] = await Promise.all([get("/v1/tasks", "tasks"), get("/v1/schedules", "schedules"), get("/v1/chat", "threads"), get("/v1/projects", "projects"), Resources.read("Server", "/v1/health"), pollInbox(), computers]);
  if (!App.me) return;
  // Magnus's own background reviews are plumbing, not the person's tasks
  // (owner, 2026-11-07): developers see them in the run inspector.
  if (tasks) App.tasks = tasks.filter(t => !t.review);
  if (schedules) App.schedules = schedules;
  if (threads) App.threads = threads;
  if (projects) App.projects = projects;
  if (health) App.health = health;
  sidebar();
  banners();
  await Promise.all([App.redraw?.(), drawPanel()]);
}

// Above the conversation: how the agent changed since the person last
// looked. A kill switch that is on has its own banner everywhere (admin.js).
function banners() {
  const el = $("#banners");
  if (!el) return;
  const v = App.versions, news = v ? v.changes.filter(c => c.seq > load("version", 0)) : [];
  const sessionUpdates = !App.feedDismissed && App.feed && (App.feed.ended.length || App.feed.arrived.length || App.feed.decided.length);
  paint(el, `${sessionUpdates ? '<div class="banner small banner-row" role="status"><span>Updates since your last session. <a href="#/activity" data-open="activity">View Activity</a></span><button type="button" data-act="dismiss-updates">Dismiss</button></div>' : ""}
    ${news.length ? `<div class="banner" role="status"><strong>The agent changed since you last looked.</strong>
      <ul class="small" style="margin:6px 0">${news.map(c => `<li>${esc(describeChange(c))} <span class="faint">(${esc(when(c.at))})</span></li>`).join("")}</ul>
      <button data-act="seen-versions">Got it</button></div>` : ""}`);
}

// What the server's kill switch (B11) did, for Admin › Overview: it refuses
// every action, suspends every task and revokes the vault's sign-ins and
// keys; after it, what was revoked and what to connect again. Pressing and
// releasing are in the admin's top bar (admin.js), not here.
function killSwitch(ks) {
  const revoked = Object.entries(ks.revoked || {});
  const state = r => r.at_provider === null || r.at_provider === undefined ? "revoking…" : r.at_provider ? "revoked by the provider" : `removed from the vault; the provider did not confirm (${r.detail || "no answer"})`;
  return `<div data-kill-switch="${ks.engaged ? "on" : "off"}"><h3>Kill switch</h3>
    ${ks.engaged
      ? `<p><strong>On.</strong> Every action is refused and every task is suspended. Nothing resumes when it is released; each person continues their own tasks.</p>`
      : `<p class="small muted">Off. Stop everything from the top bar: it refuses every action, suspends every task, revokes every sign-in at its provider and seals the vault, at once.</p>`}
    ${(ks.sealed || []).length ? `<h3>Sealed until you turn it off</h3><p class="small muted">API keys and app secrets cannot be revoked from here; if you think one leaked, revoke it in that provider's console.</p>
      <ul class="small" data-sealed>${ks.sealed.map(n => `<li class="mono" data-secret="${esc(n)}">${esc(n)}</li>`).join("")}</ul>` : ""}
    ${revoked.length ? `<h3>Revoked${ks.engaged ? "" : ": connect these again"}</h3><table class="t" data-revoked><tbody>${revoked.map(([n, r]) => `<tr data-secret="${esc(n)}"><td class="mono small">${esc(r.adapter ? `${r.adapter} sign-in` : n)}</td><td class="small">${esc(state(r))}</td></tr>`).join("")}</tbody></table>
      ${ks.engaged ? "" : `<p class="small"><a href="#/admin/models">Models</a>: sign in or enter keys again.</p>`}` : ""}
  </div>`;
}

// Where a task stands for the person looking: waiting on them, working,
// waiting on something else (a set time), done, or failed or cancelled.
const STATE = t => t.needs_you ? "needs" : t.status === "running" ? "running" : t.status === "suspended" ? "waiting" : t.status === "completed" ? "done" : "failed";
const SECTIONS = [["needs", "Needs you"], ["running", "Running"], ["waiting", "Waiting"], ["done", "Done"], ["failed", "Failed"]];

// Tasks grouped by the request that started them: a chat message or a
// schedule; a task started on its own is a group of one. A group sits in the
// section of its most pressing task.
function groupTasks(tasks) {
  const by = new Map();
  for (const t of tasks) {
    const key = t.group ? `${t.group.kind}:${t.group.id}` : `task:${t.task_id}`;
    if (!by.has(key)) by.set(key, { key, group: t.group, tasks: [] });
    by.get(key).tasks.push(t);
  }
  const groups = [...by.values()];
  for (const g of groups) {
    g.tasks.sort((a, b) => (a.created_at || "").localeCompare(b.created_at || ""));
    g.count = Object.fromEntries(SECTIONS.map(([s]) => [s, g.tasks.filter(t => STATE(t) === s).length]));
    g.section = ["needs", "running", "waiting", "failed"].find(s => g.count[s]) || "done";
    g.latest = g.tasks.map(t => t.ended_at || t.created_at || "").sort().pop();
  }
  groups.sort((a, b) => b.latest.localeCompare(a.latest));
  return SECTIONS.map(([state, label]) => ({ state, label, groups: groups.filter(g => g.section === state) })).filter(x => x.groups.length);
}

function progress(g) {
  const c = g.count, cancelled = g.tasks.filter(t => t.status === "cancelled").length;
  const parts = [`${c.done} of ${g.tasks.length} done`];
  if (c.needs) parts.push(`${c.needs} need${c.needs === 1 ? "s" : ""} you`);
  if (c.running) parts.push(`${c.running} running`);
  if (c.waiting) parts.push(`${c.waiting} waiting`);
  if (c.failed - cancelled) parts.push(`${c.failed - cancelled} failed`);
  if (cancelled) parts.push(`${cancelled} cancelled`);
  return parts.join(" · ");
}

function taskRow(t) {
  const why = t.status === "suspended" ? WAITING[t.waiting_on] || (t.needs_you ? "waiting for you" : "") : "";
  return `<li class="trow" data-state="${STATE(t)}" data-task="${esc(t.task_id)}"><a href="#/task/${enc(t.task_id)}" data-open="task:${esc(t.task_id)}"><span class="ttl" title="${esc(t.text)}">${esc(t.title)}</span>${chip(t.status)}<span class="small muted why">${esc(why)}</span><span class="small faint when">${esc(when(t.ended_at || t.created_at))}</span></a></li>`;
}

// A group: its request and progress, the tasks that need the person in
// view, the rest folded away.
function groupBlock(g, section) {
  if (!g.group && g.tasks.length === 1) return `<ul class="tlist">${taskRow(g.tasks[0])}</ul>`;
  const href = g.group.kind === "chat" ? `href="#/chat/${enc(g.group.thread)}"` : `href="#/schedules" data-open="schedule:${esc(g.group.id)}"`;
  const shown = section === "needs" ? g.tasks.filter(t => t.needs_you) : [];
  const rest = g.tasks.filter(t => !shown.includes(t));
  return `<div class="tgroup" data-group="${esc(g.key)}"><div class="ghead"><a class="ttl" ${href} title="${esc(g.group.title)}">${g.group.kind === "schedule" ? "Schedule: " : ""}${esc(g.group.title)}</a><span class="small muted">${esc(progress(g))}</span></div>
    ${shown.length ? `<ul class="tlist">${shown.map(taskRow).join("")}</ul>` : ""}
    ${rest.length ? `<details data-key="${esc(g.key)}"><summary class="small muted">${shown.length ? `The other ${rest.length}` : `${rest.length} task${rest.length === 1 ? "" : "s"}`}</summary><ul class="tlist">${rest.map(taskRow).join("")}</ul></details>` : ""}</div>`;
}

// Tasks, as a view over the same tasks the conversations show: grouped by
// the request that started them, in sections by where they stand.
const MORE = new Set();
function tasksView() {
  const LIMIT = { done: 15, failed: 15 };
  const sections = groupTasks(App.tasks);
  return sections.length ? sections.map(sec => {
    const n = sec.groups.reduce((k, g) => k + g.count[sec.state], 0);
    const cap = LIMIT[sec.state] && !MORE.has(sec.state) ? LIMIT[sec.state] : Infinity;
    const left = sec.groups.length - Math.min(cap, sec.groups.length);
    return `<section class="tsec" data-section="${sec.state}"><h3>${esc(sec.label)} <span class="count">${n}</span></h3>
      ${sec.groups.slice(0, cap).map(g => groupBlock(g, sec.state)).join("")}
      ${left ? `<button class="link small" data-more="${sec.state}">Show ${left} more</button>` : ""}</section>`;
  }).join("") : offered("No tasks yet. Ask for something in a conversation.");
}

// An empty Tasks or Schedules: what Magnus offered to take over when it
// opened a conversation itself (at the first sign-in, or for a newly
// connected app) and the person has not answered, each linking to its
// conversation, in Magnus's voice. With none, the plain line.
function offered(none) {
  const cards = App.threads.filter(t => t.opener).flatMap(t => (t.open_cards || []).map(c => ({ ...c, thread: t.thread })));
  if (!cards.length) return `<p class="empty">${esc(none)}</p>`;
  return `<div class="empty" data-offered><p>Nothing here yet. I had a look around, and these are mine to take off your hands as soon as you say so:</p>
    <ul class="tlist">${cards.map(c => `<li class="trow" data-offer="${esc(c.card)}"><a href="#/chat/${enc(c.thread)}"><span class="ttl">${esc(c.title)}</span></a></li>`).join("")}</ul></div>`;
}

// A schedule's next run and its last result, in one line.
function scheduleState(sc) {
  const last = sc.last;
  return `${(sc.paused || sc.ended) ? "" : `Next run ${esc(until(sc.next))} · `}${sc.last_failed ? `Last firing <span class="chip bad">could not start</span> ${esc(sc.last_failed.error)} <span class="faint">${esc(when(sc.last_failed.at))}</span>`
    : last ? `Last result: ${chip(last.status)} <a href="#/task/${enc(last.task_id)}" data-open="task:${esc(last.task_id)}">${esc(firstLine(last.summary) || last.title)}</a> <span class="faint">${esc(when(last.ended_at || last.created_at))}</span>` : "No results yet"}`;
}

// When a schedule ends, what it may spend a run, and why it is paused or
// skipped a run: the same lines in the list and in the schedule itself.
function scheduleFacts(sc) {
  return `${sc.ended ? `<div class="small" data-ended><span class="chip">Ended</span> ${esc(sc.ended)}.</div>` : ""}
    <div class="small muted" data-ends>${sc.until || sc.max_runs ? `Ends: ${esc(ends(sc))}` : "Runs until you stop it"}${sc.max_runs ? ` · ${esc(sc.started)} of ${esc(sc.max_runs)} runs so far` : ""}${sc.budget ? ` · up to $${esc(sc.budget.max_usd)} a run` : ""}</div>
    ${sc.paused ? `<div class="small" data-paused><span class="chip bad">Paused</span> ${esc(sc.paused)}. ${sc.paused_on ? `It goes on once you answer that run (<a href="#/task/${enc(sc.paused_on)}" data-open="task:${esc(sc.paused_on)}">open it</a>).` : "Nothing runs until you resume it."}</div>` : ""}
    ${sc.skipped ? `<div class="small muted" data-skipped>Skipped a run ${esc(when(sc.skipped.at))}: ${esc(sc.skipped.why)} (<a href="#/task/${enc(sc.skipped.task)}" data-open="task:${esc(sc.skipped.task)}">open it</a>)</div>` : ""}`;
}
// Resume, for a schedule paused until the person says so (one paused on a
// run waiting for them goes on once they answer it), and Stop schedule.
// An ended schedule stays until its person deletes it (#264): Delete
// schedule is its one control (#452).
function scheduleButtons(sc, stop = "Stop schedule") {
  if (sc.ended) return `<button data-act="delete-schedule" data-id="${esc(sc.id)}">Delete schedule</button>`;
  return `${sc.paused && !sc.paused_on ? `<button data-act="resume-schedule" data-id="${esc(sc.id)}">Resume</button>` : ""}<button data-act="stop-schedule" data-id="${esc(sc.id)}">${stop}</button>`;
}

// Schedules, soonest first, ended last: each opens beside the conversation.
function schedulesView() {
  const key = sc => `${sc.ended ? "1" : "0"}${sc.next || ""}`;
  const list = [...App.schedules].sort((a, b) => key(a).localeCompare(key(b)));
  return list.length ? list.map(sc => `<div class="card flat sched" data-schedule="${esc(sc.id)}">
      <div class="spread"><a class="ttl" href="#/schedules" data-open="schedule:${esc(sc.id)}" title="${esc(sc.text)}">${esc(sc.title)}</a><span class="small muted">${esc(rule(sc))} ${scheduleButtons(sc)}</span></div>
      ${scheduleFacts(sc)}
      <div class="small muted" data-last>${scheduleState(sc)}</div>
      ${sc.runs.some(r => r.needs_you) ? `<ul class="tlist">${sc.runs.filter(r => r.needs_you).map(taskRow).join("")}</ul>` : ""}</div>`).join("")
    : offered("No schedules yet. Ask in a conversation for something done every day or every week.");
}

// One schedule: what it does, when, how its runs went, and a way to stop it.
function scheduleView(id) {
  const sc = App.schedules.find(x => x.id === id);
  if (!sc) return `<p class="empty">Schedule details are not available.</p>`;
  return `<h2 class="ptitle">${esc(sc.title)}</h2><p class="small muted">${esc(rule(sc))}</p>
    <div class="card flat sched" data-schedule="${esc(sc.id)}"><p>${esc(sc.text)}</p>${scheduleFacts(sc)}<div class="small" data-last>${scheduleState(sc)}</div></div>
    <div class="row">${moveControl(sc.id, sc.project, App.projects)}${scheduleButtons(sc, "Stop this schedule")}</div>
    <h3>Runs</h3>${sc.runs.length ? `<ul class="tlist">${sc.runs.map(taskRow).join("")}</ul>` : `<p class="empty">None yet.</p>`}
    ${sc.failures?.length ? `<h3>Firings that could not start</h3><ul class="small">${sc.failures.map(f => `<li>${esc(when(f.at))}: ${esc(f.error)}</li>`).join("")}</ul>` : ""}`;
}

// ---------- Model choice ----------

// The models a person may run a task or a message on, as the adapters'
// providers list them now. The appliance's default is used until the viewer
// picks another; a pick holds only while the default it was made under is
// still the default, so changing the default reaches new messages (#540).
async function modelPicker() {
  const { default: dflt, adapters } = await api("GET", "/v1/models").catch(() => ({ default: null, adapters: [] }));
  const key = m => JSON.stringify([m.adapter, m.model]);
  const opts = adapters.flatMap(a => a.models.map(model => ({ adapter: a.adapter, model })));
  if (dflt && !opts.some(o => key(o) === key(dflt))) opts.unshift(dflt);
  const under = dflt ? key(dflt) : "";
  const saved = load("model-choice", null);
  const pick = opts.find(o => saved?.under === under && key(o) === key(saved)) || dflt || opts[0];
  const groups = [...new Set(opts.map(o => o.adapter))];
  return `<label class="model"><span class="sr">Model</span><select data-model data-default="${esc(under)}" aria-label="Model">${groups.map(g => `<optgroup label="${esc(g)}">${opts.filter(o => o.adapter === g).map(o =>
    `<option value="${esc(key(o))}"${pick && key(o) === key(pick) ? " selected" : ""}>${esc(o.model)}${dflt && key(o) === key(dflt) ? " (default)" : ""}</option>`).join("")}</optgroup>`).join("")}</select></label>`;
}
function chosenModel(root) {
  const sel = $("select[data-model]", root);
  if (!sel || !sel.value) return undefined;
  const [adapter, model] = JSON.parse(sel.value);
  // Sending on the default is not a pick: nothing is kept to outlive it.
  save("model-choice", sel.value === sel.dataset.default ? null : { adapter, model, under: sel.dataset.default });
  return { adapter, model };
}

// ---------- Chat ----------

// A message is answered, or becomes a task or a schedule to confirm: the
// model decides. A conversation goes on until the person starts a new one;
// opening Magnus returns to the latest. What it starts shows in the thread
// as it goes: tasks with their status, the decisions they wait on (the same
// cards as under Needs you), schedules, results and their files.
function composer(placeholder, picker) {
  return `<form id="say" class="say"><label for="msg" class="sr">Message</label><textarea id="msg" rows="1" placeholder="${esc(placeholder)}" required></textarea>
    <div class="row small" id="pending"></div>
    <div class="say-bar">${picker}<span class="grow"></span>${attachButton("Attach files", true)}<button class="send" id="send" aria-label="Send message" title="Send message">${icon("send")}<span class="sr">Send message</span></button></div></form>`;
}
// Files added to the next message: uploaded at once, sent with it. What is
// typed and attached stays with this conversation while the person moves
// around the app (#544).
function wireComposer(thread, project, record) {
  const form = $("#say"), box = $("#msg",form), pending = $("#pending",form), input = $("[data-attach]",form), epoch = MessageDrafts.epoch();
  let replacing = null, active = true;
  box.value = record.text;
  const model = $("[data-model]",form);
  if (record.model && record.modelUnder === model.dataset.default && [...model.options].some(o => o.value === record.model)) model.value = record.model;
  const fit = () => { box.style.height = "auto"; box.style.height = `${Math.min(box.scrollHeight, innerHeight / 3)}px`; };
  const selected = () => { record.selection = [box.selectionStart,box.selectionEnd]; };
  const read = (selection=true) => { record.text = box.value; record.model = model?.value || ""; record.modelUnder = model?.dataset.default || ""; if(selection)selected(); };
  const chips = () => record.files.map(f => `<span class="attachment-chip" data-upload="${esc(f.uid)}"><span>${esc(f.name)}</span><span class="small muted">${esc(f.state === "ready" ? "Ready" : f.state === "uploading" ? "Uploading…" : f.state === "unavailable" ? "Unavailable" : "Failed")}</span>${f.error ? `<span class="small">${esc(f.error)}</span>` : ""}${["failed","unavailable"].includes(f.state) ? `<button type="button" data-retry-upload="${esc(f.uid)}">Retry</button>` : ""}<button type="button" class="icon-btn" data-remove-upload="${esc(f.uid)}" aria-label="Remove ${esc(f.name)} from message" title="Remove ${esc(f.name)} from message">${icon("close")}</button></span>`).join("");
  // The record is shared by every composer mounted on this conversation:
  // a send that finishes after the person left and came back updates the
  // one now showing, and a send in flight stays in flight there too.
  const show = () => {
    if (!active || !form.isConnected) return;
    if (box.value !== record.text) { box.value = record.text; fit(); }
    paint(pending,chips());
    $("#send",form).disabled = !!record.sending || record.files.some(f => f.state !== "ready");
  };
  const unsubscribe = MessageDrafts.subscribe(show);
  async function startUpload(slot, file) {
    const currentEpoch = MessageDrafts.epoch(); slot.state = "uploading"; delete slot.error;
    MessageDrafts.originals.set(slot.uid,file); MessageDrafts.changed();
    try {
      const [ref] = await upload([file],record.thread ? {thread:record.thread} : record.project ? {project:record.project} : {});
      if (currentEpoch !== MessageDrafts.epoch() || !record.files.includes(slot)) return;
      Object.assign(slot,{id:ref.id,name:ref.name,media_type:ref.media_type,size:ref.size,state:"ready"});
    } catch (err) {
      if (currentEpoch !== MessageDrafts.epoch() || !record.files.includes(slot)) return;
      slot.state = "failed"; slot.error = err.message || "Upload failed. Retry or remove this file.";
    }
    MessageDrafts.changed();
  }
  input.onchange = e => {
    const files = [...e.target.files]; e.target.value = "";
    for (const [i,file] of files.entries()) {
      const slot = i === 0 && replacing ? record.files.find(f => f.uid === replacing) : null;
      const item = slot || {uid:MessageDrafts.uid(),name:file.name,media_type:file.type,size:file.size,state:"uploading"};
      if (!slot) record.files.push(item);
      item.name = file.name; startUpload(item,file);
    }
    replacing = null;
  };
  pending.onclick = e => {
    const remove = e.target.closest("[data-remove-upload]"), retry = e.target.closest("[data-retry-upload]");
    // Removing a file takes it off this message; the shared file stays in Files.
    if (remove) { const uid=remove.dataset.removeUpload; record.files=record.files.filter(f=>f.uid!==uid); MessageDrafts.originals.delete(uid); MessageDrafts.changed(); }
    if (retry) {
      const uid=retry.dataset.retryUpload, slot=record.files.find(f=>f.uid===uid), original=MessageDrafts.originals.get(uid);
      if (original) startUpload(slot,original); else { replacing=uid; input.click(); }
    }
  };
  box.oninput = () => { read(); fit(); };
  box.onselect = selected;
  model?.addEventListener("change",read);
  // Enter sends; Shift+Enter and an input method's Enter do not.
  box.onkeydown = e => { if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.keyCode !== 229) { e.preventDefault(); form.requestSubmit(); } };
  form.onsubmit = async e => {
    e.preventDefault(); read();
    const text=record.text.trim(); if (!text || record.sending) return;
    if (record.files.some(f=>f.state!=="ready")) { toast("Wait for uploads, or retry/remove unavailable files before sending."); return; }
    const files=[...record.files], body={text, model:chosenModel(form), files:files.map(f=>f.id)};
    if (record.thread) body.thread=record.thread; else if (record.project) body.project=record.project;
    record.sending=true; MessageDrafts.changed();
    try {
      const response=await api("POST","/v1/chat",body);
      if (epoch !== MessageDrafts.epoch()) return;
      // A secret typed into chat was hidden by the server: say so, and where a sign-in belongs.
      if (response?.hidden?.length) toast(`Magnus hid the ${[...new Set(response.hidden)].join(" and ")} in your message: it is not kept, and Magnus never sees it. When Magnus needs a sign-in, it asks for it through a secure form.`, 8000);
      // Clear what was sent; anything typed or attached meanwhile stays.
      if (record.text.trim()===text) { record.text=""; record.selection=[0,0]; }
      record.files=record.files.filter(f=>!files.includes(f));
      if (!record.thread && response?.thread) {
        MessageDrafts.discard(record.id);
        const next=MessageDrafts.forThread(response.thread); next.text=record.text; next.files=record.files; next.model=record.model; next.modelUnder=record.modelUnder;
        if (App.workspace && App.draftRecord === record) { history.replaceState(history.state,"",`#/chat/${enc(response.thread)}`); record.sending=false; await go(); return; }
      }
      await refresh();
    } catch (err) {
      if (epoch !== MessageDrafts.epoch()) return;
      // The message stays in the box. A lost response may still have been
      // accepted, so say so and never send it again by itself (UX-08).
      const rejected=err.status>=400 && err.status<500 && typeof err.body?.error === "string";
      toast(rejected ? err.message : "Send outcome not confirmed. Your message is still here; check the conversation before sending it again.");
      if (!rejected) refresh().catch(()=>{});
    } finally { record.sending=false; MessageDrafts.changed(); }
  };
  onLeave(() => { if (active) { read(false); active=false; unsubscribe(); } });
  // Only a missing/expired reference is marked unavailable. A connection
  // failure does not silently turn a valid attachment into a missing one.
  for (const f of record.files.filter(f=>f.state==="ready")) api("GET",`/v1/files/${enc(f.id)}/info`).catch(err=>{
    if ([404,410].includes(err.status) && epoch===MessageDrafts.epoch() && record.files.includes(f)) { f.state="unavailable"; f.error="Select the file again or remove it."; MessageDrafts.changed(); }
  });
  fit(); show();
  box.setSelectionRange(...(record.selection || [box.value.length,box.value.length]));
}

// Lays out a conversation; false when the person moved on while it loaded.
async function chatLayout(main, thread, project) {
  const seq = App.seq;
  App.thread = thread;
  sidebar();
  paint(main, `<section class="conv"><header class="conv-head" id="conv-head"></header><div id="banners"></div><div id="thread" class="thread" aria-live="off"></div><button type="button" class="jump-latest" data-jump-latest hidden>New activity · Jump to latest</button><div id="composer"></div></section>`);
  convHead(thread, project);
  banners();
  $("#banners", main).onclick = e => {
    if (e.target.closest("[data-act=seen-versions]")) { save("version", App.versions?.latest_seq ?? 0); banners(); }
    // Dismissing the updates notice, or opening Activity from it, closes it until the next session.
    if (e.target.closest("[data-act=dismiss-updates], [data-open=activity]")) { App.feedDismissed = true; banners(); }
  };
  const picker = await modelPicker();
  if (seq !== App.seq) return false;
  paint($("#composer", main), composer(thread ? "Reply, or give Magnus more to do" : "Ask anything, or give Magnus work to do", picker));
  const record = MessageDrafts.forThread(thread,project);
  App.draftRecord = record;
  wireComposer(thread, project,record);
  $("[data-jump-latest]",main).onclick = () => {
    const root=$("#thread",main); root.scrollTop=root.scrollHeight; ReadingAnchors.remember(root);
    record.anchor={follow:true}; $("[data-jump-latest]",main).hidden=true;
  };
  onLeave(ReadingAnchors.watch($("#thread",main),record));
  return true;
}

// Above the conversation: its project (a link to it) and its title, and
// what can be done to the whole conversation.
function convHead(thread, project) {
  const el = $("#conv-head");
  if (!el) return;
  const here = thread && App.threads.find(t => t.thread === thread);
  const p = here ? here.project : project && App.projects.find(x => x.id === project);
  paint(el, `<div class="conv-title"><div class="small muted">${p ? `<a href="#/project/${enc(p.id)}" data-open="project:${esc(p.id)}">${esc(p.name)}</a>` : thread ? "No project" : ""}</div>
    <h1 title="${esc(here?.title || "")}">${esc(here?.title || "New conversation")}</h1></div>
    <div class="row">${computerChip(App.computers)}${thread ? `${moveControl(thread, here?.project, App.projects, true)}<button class="icon-btn" data-trash="${esc(thread)}" data-then="#/home" title="Delete conversation">${icon("trash")}<span class="sr">Delete conversation</span></button>` : ""}</div>`);
}

async function showNewChat(main, project) {
  if (!await chatLayout(main,null,project)) return;
  const record = App.draftRecord;
  paint($("#thread",main), `<p class="muted hello">Ask a question, or say what you want done. Magnus answers, starts a task, or proposes a schedule for you to confirm. What it starts shows here as it goes.</p>`);
  if (App.appSearch) { record.text=App.appSearch;record.selection=[record.text.length,record.text.length];App.appSearch=null;const box=$("#msg",main);if(box){box.value=record.text;box.setSelectionRange(...record.selection);} }
  sidebar();
}

async function showThread(main, thread) {
  if (!await chatLayout(main, thread, null)) return;
  // An answer being written shows as it streams; `live` holds its text so far.
  const ui = { editing: null, sharing: null, declining: null, people: [], live: {} };
  const listening = {};
  const listen = entries => {
    const running = new Set(entries.filter(e => e.kind === "turn" && e.status === "running").map(e => e.task_id));
    for (const id of running) if (!listening[id]) listening[id] = Magnus.live(id, t => {
      ui.live[id] = (ui.live[id] || "") + t;
      const el = $(`[data-live="${CSS.escape(id)}"]`, main);
      if(el){
        const root=$("#thread",main), followed=ReadingAnchors.capture(root)?.follow;
        ReadingAnchors.hold(root,()=>{patchLive(el,ui.live[id]);loadMarkdownImages(el);});
        if(!followed)$("[data-jump-latest]",main).hidden=false;
      }
    });
    for (const id of Object.keys(listening)) if (!running.has(id)) { listening[id](); delete listening[id]; delete ui.live[id]; }
  };
  let shown = [], drawRevision=0;
  onLeave(()=>Resources.forget("Conversation"));
  const draw = async () => {
    const revision=++drawRevision;
    const entries = await Resources.read("Conversation", `/v1/chat/${enc(thread)}`, "entries");
    if (!entries || revision!==drawRevision) return;
    if (App.thread !== thread) return;
    shown = entries;
    // A decision the thread now waits on, not read yet: read the inbox first.
    const waits = entries.flatMap(e => e.kind === "tasks" ? e.tasks.map(t => t.waiting_item) : e.kind === "sub_agents" ? e.agents.map(a => a.waiting_item) : [e.waiting_item]).filter(Boolean);
    if (waits.some(id => !App.inbox.some(i => i.id === id))) await pollInbox();
    convHead(thread);
    ui.composed = [];
    // A result that comes after the person has moved on (a message of theirs
    // since the task started) quotes what they asked.
    ui.movedOn = new Set();
    const startedAt = {};
    let lastMessage = -1;
    entries.forEach((e, i) => {
      if (e.kind === "message") lastMessage = i;
      if (e.kind === "tasks") for (const t of e.tasks) startedAt[t.task_id] = i;
      if (e.kind === "result" && startedAt[e.task_id] !== undefined && lastMessage > startedAt[e.task_id]) ui.movedOn.add(e.task_id);
    });
    const body = entries.map(e => `<div class="entry ${e.kind === "message" ? "me" : "them"}" data-entry="${esc(e.id || "")}" data-kind="${esc(e.kind)}">${entry(e, ui)}${msgActions(e, ui)}</div>`).join("");
    const el = $("#thread", main);
    if (!el) return;
    const anchor = ui.drawn ? ReadingAnchors.capture(el) : App.draftRecord?.anchor || {follow:true};
    const newActivity = ui.entryIds && entries.some(e=>e.id && !ui.entryIds.has(e.id));
    ui.entryIds=new Set(entries.map(e=>e.id)); ui.drawn=true;
    const grew = paint(el, body); liveStatusLines(el);
    ReadingAnchors.restore(el,anchor); ReadingAnchors.remember(el,anchor);
    if (newActivity && !anchor.follow) $("[data-jump-latest]",main).hidden=false;
    bindConnectorActs(el);
    keepHeights(el);
    motion(el, ui);
    for (const e of entries.filter(e => e.view)) fill($(`iframe[data-shown="${CSS.escape(e.id)}"]`, main), e.view, e.task_id);
    for (const [id, view, , task] of ui.composed) fill($(`iframe[data-view="${CSS.escape(id)}"]`, el), view, task);
    listen(entries);
    loadGlimpses(el);

  };
  const path = (id, act) => `/v1/chat/${enc(thread)}/entries/${enc(id)}/${act}`;
  const msgAct = async (b, el) => {
    const id = el.dataset.entry, e = shown.find(x => x.id === id), act = b.dataset.msgAct;
    if (!e) return;
    switch (act) {
      case "copy": await copyText(entryText(e)); return toast("Copied.");
      case "edit": drafts[`edit-${id}`] = e.text; ui.editing = id; return draw();
      case "edit-cancel": ui.editing = null; return draw();
      case "edit-send": {
        const text = $(`#edit-${CSS.escape(id)}`, main).value.trim();
        if (!text) return toast("Write the message first.");
        b.disabled = true; await api("POST", path(id, "resend"), { text }); ui.editing = null; delete drafts[`edit-${id}`]; return draw();
      }
      case "retry": b.disabled = true; await api("POST", path(id, "resend"), {}); return draw();
      case "branch": { const r = await api("POST", path(id, "branch"), {}); toast("Branched into a new chat."); location.hash = `#/chat/${enc(r.thread)}`; return; }
      case "share": ui.people = (await api("GET", "/v1/directory")).people; if (!ui.people.length) return toast("Nobody else is on this server."); ui.sharing = id; return draw();
      case "share-cancel": ui.sharing = null; return draw();
      case "share-send": {
        const to = $("[data-share-to]", el).value;
        b.disabled = true; await api("POST", path(id, "share"), { person: to }); ui.sharing = null;
        toast(`Shared with ${ui.people.find(p => p.id === to)?.name || to}.`); return draw();
      }
      case "export": return toast(`Saved ${await Magnus.download(path(id, "export"), safeName(e))}`);
      case "save": await api("POST", path(id, "save"), {}); return toast("Saved to Files.");
      case "listen": return readAloud(api("POST",path(id,"spoken"),{}).then(r=>r.script),id);
      case "hush": return hush();
      case "feedback": {
        const d = document.createElement("dialog"); d.className = "feedback-dialog"; d.setAttribute("aria-label", "Feedback");
        d.innerHTML = `<h2>Feedback</h2>${e.task_id ? verdictBar(e.task_id, e.verdict) : ""}<div class="row">${["great", "annoying"].map(a => `<button data-msg-act="${a}" aria-pressed="${e.reaction === a}">${a === "great" ? "That was great" : "That was annoying"}</button>`).join("")}</div><button data-close-feedback>Close feedback</button>`;
        d.onclick = async event => { const action=event.target.closest("[data-msg-act]");if(event.target.closest("[data-close-feedback]"))d.close();if(action){const status=document.createElement("p");status.className="small";status.setAttribute("role","status");status.textContent="Saving feedback…";d.append(status);try{await msgAct(action,el);d.close();}catch(err){status.textContent="Feedback was not confirmed. Try again when connected.";toast(err.message);action.disabled=false;}} };
        d.onclose = () => { d.remove(); $(`[data-entry="${CSS.escape(id)}"] .menu-trigger`, main)?.focus({preventScroll:true}); };
        document.body.append(d); d.showModal(); return;
      }
      case "great": case "annoying": b.disabled = true; await api("POST", path(id, "reaction"), { reaction: e.reaction === act ? null : act }); return draw();
      case "task": { const r = await api("POST", path(id, "task"), {}); toast("Started a task."); await refresh(); return r; }
      case "delete":
        if (!await ask("Delete this message? It leaves this chat and what Magnus reads next, and waits in the trash until it is gone for good.", "Delete")) return;
        await api("POST", path(id, "delete"), {}); await draw();
        const heading = $("#conv-head h1"); if (heading) { heading.tabIndex = -1; heading.focus({preventScroll:true}); } return;
    }
  };
  App.redraw = draw;
  onLeave(() => { App.redraw = null; listen([]); hush(); });
  $("#thread", main).onclick = async e => {
    const m = e.target.closest("[data-msg-act]");
    if (m) { try { await msgAct(m, m.closest("[data-entry]")); } catch (err) { toast(err.message); m.disabled = false; } return; }

    // The live cards' buttons act directly, not through Magnus.
    const ta = e.target.closest("[data-task-act]"), sa = e.target.closest("[data-sched-act]");
    if (ta || sa) {
      e.preventDefault();
      const el = ta || sa;
      el.disabled = true;
      try {
        if (ta) {
          if (!await ask("Stop this task? What it already did stays done.", "Stop")) { el.disabled = false; return; }
          await api("POST", `/v1/tasks/${enc(ta.dataset.task)}/cancel`, {}); toast("Stopped.");
        } else {
          const id = enc(sa.dataset.scheduleId), k = sa.dataset.schedAct;
          if (k === "cancel") {
            const ended = App.schedules.find(x => x.id === sa.dataset.scheduleId)?.ended;
            if (!await ask(ended ? "Delete this schedule? It has ended, so nothing stops; its runs stay in Tasks." : "Cancel this schedule? No more of its runs start, and a run still going stops.", ended ? "Delete schedule" : "Cancel schedule")) { el.disabled = false; return; }
            await api("DELETE", `/v1/schedules/${id}`); toast(ended ? "The schedule is deleted." : "The schedule is cancelled.");
          } else { await api("POST", `/v1/schedules/${id}/${k}`, {}); toast(k === "pause" ? "Paused." : "Resumed."); }
        }
        await refresh();
      } catch (err) { toast(err.message); el.disabled = false; }
      return;
    }
    const b = e.target.closest("[data-decide]");
    if (!b) return;
    const card = b.closest("[data-card]").dataset.card, decision = b.dataset.decide;
    // Decline asks for the reason first; Magnus reads it before suggesting again.
    if (decision === "decline-open" || decision === "decline-cancel") { ui.declining = decision === "decline-open" ? card : null; return draw(); }
    const body = { decision };
    // A grant card accepted: the grant as the person set it in the form.
    const box = b.closest("[data-card]");
    if (box.matches(".grant") && decision === "confirm") {
      Object.assign(body, readGrantForm(box));
      if (!body.apps.length) return toast("Choose at least one app.");
    }
    const no = decision === "decline";
    if (no) {
      body.reason = ($(`#why-${CSS.escape(card)}`, main)?.value || "").trim();
      if (!body.reason) return toast("Say why, so Magnus knows.");
    }
    b.disabled = true;
    try {
      const r = await api("POST", `/v1/chat/cards/${enc(card)}`, body);
      toast(no ? "Declined." : decision === "later" ? "Not now." : r.grant ? "Granted." : r.task ? "Started." : "Scheduled.");
      ui.declining = null; delete drafts[`why-${card}`];
      for (const k of Object.keys(drafts)) if (k.startsWith(`g-${card}-`)) delete drafts[k];
      await refresh();
    } catch (err) { toast(err.message); b.disabled = false; }
  };
  await draw();
}

// Open a conversation in place, with `panel` beside it. Replacing the
// window address reloads this app on every entry.
function showChat(main, hash, view, panel) {
  if (location.hash !== hash) history.replaceState({...history.state}, "", hash);
  if (panel) openPanel(panel);
  return view(main);
}

// Home is the latest conversation (a new one when there is none), with
// `panel` open beside it.
async function home(main, panel = App.openAfter) {
  App.openAfter = null;
  const seq = App.seq;
  const threads = (await api("GET", "/v1/chat")).threads;
  App.threads = threads;
  if (seq !== App.seq) return;
  if (threads.length) {
    const id = threads[0].thread;
    return showChat(main, `#/chat/${enc(id)}`, m => showThread(m, id), panel);
  }
  return showChat(main, "#/chat/new", m => showNewChat(m), panel);
}

route(/^#\/(?:home|chat)?$/, main => home(main), true);
// Draft links from the earlier durable-draft build open a new conversation.
route(/^#\/draft\/([^/]+)$/, () => { location.replace("#/chat/new"); }, true);
route(/^#\/chat\/new$/, m => showNewChat(m), true);
route(/^#\/chat\/new\/([^/]+)$/, (m, project) => showNewChat(m, project), true);
// Where the views over tasks, schedules, files and decisions used to be
// pages: each opens beside the latest conversation.
route(/^#\/inbox$/, main => home(main, "needs"), true);
route(/^#\/(activity|tasks|schedules|files|projects)$/, (main, k) => home(main, k), true);
// The Library is the connector catalog, a page of its own (connectors spec,
// Library); a connector's page is under it.
route(/^#\/library(?:\/([^/]+))?$/, (main, name) => showLibrary(main, name));
route(/^#\/new$/, main => home(main), true);
route(/^#\/file\/([^/]+)$/, (main, id) => home(main, `file:${id}`), true);
route(/^#\/project\/([^/]+)$/, (main, id) => home(main, `project:${id}`), true);
// A task opens beside the conversation it came from.
route(/^#\/task\/([^/]+)$/, async (main, id) => {
  const seq = App.seq;
  const t = await api("GET", `/v1/tasks/${enc(id)}`);
  if (seq !== App.seq) return;
  if (!t.thread) return home(main, `task:${id}`);
  return showChat(main, `#/chat/${enc(t.thread)}`, m => showThread(m, t.thread), `task:${id}`);
}, true);

function rule(e) {
  const m = e.every_minutes;
  if (m) {
    for (const [size, one, many] of [[10080, "week", "weeks"], [1440, "day", "days"], [60, "hour", "hours"]]) {
      if (m % size === 0) return m === size ? `Every ${one}` : `Every ${m / size} ${many}`;
    }
    return m === 1 ? "Every minute" : `Every ${m} minutes`;
  }
  if (e.daily_at) return `Every day at ${e.daily_at}`;
  return "";
}
// The decision an entry waits on, as its card, where the person can make it:
// the same card as under Needs you and beside the conversation.
function waitingCard(id, ui) {
  const i = id && App.inbox.find(x => x.id === id && x.status === "open");
  return i ? card(i, ui.composed || []) : "";
}
// When a schedule stops: a time, a number of runs, both, or never.
function ends(e) {
  const parts = [];
  if (e.until) parts.push(`no run after ${new Date(e.until).toLocaleString()}`);
  if (e.max_runs) parts.push(`at most ${e.max_runs} run${e.max_runs === 1 ? "" : "s"}`);
  return parts.length ? parts.join(", ") : "until you stop it";
}
// Where a task stands, as the design draws it: an icon and a pill. A task
// waiting on a decision the person can make needs them.
const needsYou = t => !!t.waiting_item && needsMe().some(i => i.id === t.waiting_item);
function standing(t) {
  if (needsYou(t)) return ["need", `<span class="chip warn">Needs you</span>`];
  return [{ completed: "done", running: "working", failed: "stopped", cancelled: "stopped" }[t.status] || "working", chip(t.status)];
}
// A task in its conversation: one row that opens it beside the thread, its
// status live; what it waits on from the person follows the request's rows.
function taskCard(t) {
  const why = t.status === "suspended" ? esc(WAITING[t.waiting_on] || "") : t.status === "running" ? statusLineSlot(t, "sub why") : "";
  const [ic, pill] = standing(t);
  return `<li class="tcard" data-chat-task="${esc(t.task_id)}" data-status="${esc(t.status)}"><a class="rg-row${Panel.what === `task:${t.task_id}` ? " open" : ""}" href="#/task/${enc(t.task_id)}" data-open="task:${esc(t.task_id)}" title="${esc(t.text)}">
    <span class="st ${ic}">${icon(ic)}</span><span class="rg-main"><span class="ttl">${esc(t.title)}</span>${t.status === "running" ? why : why ? `<span class="sub why">${why}</span>` : `<span class="sub">${esc(firstLine(t.text))}</span>`}</span>${pill}${icon("chev", "chev")}</a>
    ${ENDED.includes(t.status) ? "" : `<div class="card-acts"><button class="link small" data-task-act="cancel" data-task="${esc(t.task_id)}">Stop</button></div>`}</li>`;
}
// A schedule where it was confirmed: a row that opens it, with when it runs
// next and how its last run went.
function scheduleText(sc) {
  const last = sc.last;
  return `${sc.ended ? `Ended: ${sc.ended}` : sc.paused ? `Paused: ${sc.paused}` : `Next run ${until(sc.next)}`} · ${sc.last_failed ? `Last firing could not start: ${sc.last_failed.error}` : last ? `Last result: ${(STATUS[last.status] || ["", last.status])[1]}, ${when(last.ended_at || last.created_at)}` : "No results yet"}`;
}
// A card's answers: accept (one tap, which grants the work), Not now, or
// Decline, which asks for the reason. A grant card is not put off (`later`
// false).
function cardActs(e, ui, accept, later = true) {
  if (ui.declining === e.card) return `<div class="row cc-act decline-why"><label class="sr" for="why-${esc(e.card)}">Why not?</label>
    <input id="why-${esc(e.card)}" data-draft="why-${esc(e.card)}" placeholder="Why not? Magnus reads this before suggesting again">
    <button class="primary" data-decide="decline">Decline</button><button class="link" data-decide="decline-cancel">Cancel</button></div>`;
  return `<div class="row cc-act"><button class="primary" data-decide="confirm">${esc(accept)}</button>${later ? `<button data-decide="later">Not now</button>` : ""}<button class="link" data-decide="decline-open">Decline</button></div>`;
}

// A grant's fields as a form (#235): the kind of work, the apps it covers,
// what may go ahead without asking besides, and its budget of model cost
// per period. `k` keys its drafts; `f` holds the values it starts from;
// `apps` are the person's apps to choose from.
function grantForm(k, f, apps) {
  const id = n => `g-${esc(k)}-${n}`, d = n => `data-draft="${id(n)}"`;
  const lifted = x => (f.allow || []).includes(x) ? "checked" : "";
  const choices = apps?.length ? apps : (f.apps || []).map(name => ({ name, title: name }));
  return `<div class="grant-form" data-grant-form>
    <label for="${id("title")}" class="small muted">What kind of work</label><input id="${id("title")}" ${d("title")} data-g="title" value="${esc(f.title || "")}" style="width:100%">
    <div class="sugg-part"><span class="small muted">In these apps</span>${choices.map(a => `<label class="row toggle small"><input type="checkbox" data-g-app="${esc(a.name)}" ${d(`app-${a.name}`)} ${(f.apps || []).includes(a.name) ? "checked" : ""}><span>${esc(a.title || a.name)}</span></label>`).join("")}</div>
    <div class="sugg-part"><span class="small muted">Without asking, Magnus may also</span>
      <label class="row toggle small"><input type="checkbox" data-g-allow="send" ${d("send")} ${lifted("send")}><span>Send on my behalf</span></label>
      <label class="row toggle small"><input type="checkbox" data-g-allow="irreversible" ${d("irreversible")} ${lifted("irreversible")}><span>Delete or commit me to anything</span></label>
      <p class="small faint">Spending money always asks.</p></div>
    <div class="row"><label for="${id("max")}" class="small muted">Budget</label><span class="small">$</span><input id="${id("max")}" type="number" min="0" step="0.5" data-g="max_usd" ${d("max")} value="${esc(f.max_usd ?? "")}" style="width:6em">
      <select aria-label="Per" data-g="period" ${d("period")}>${["day", "week", "month"].map(p => `<option value="${p}" ${(f.period || "week") === p ? "selected" : ""}>a ${p}</option>`).join("")}</select><span class="small muted">of model cost</span></div>
  </div>`;
}

// The grant a form holds, as the appliance takes it.
function readGrantForm(root) {
  const f = $("[data-grant-form]", root);
  return {
    title: $("[data-g=title]", f).value.trim(), period: $("[data-g=period]", f).value, max_usd: Number($("[data-g=max_usd]", f).value),
    apps: $$("[data-g-app]", f).filter(i => i.checked).map(i => i.dataset.gApp), allow: $$("[data-g-allow]", f).filter(i => i.checked).map(i => i.dataset.gAllow),
  };
}

// What a card rests on: each item, and the tool or task it points at.
function evidenceList(e, label) {
  return `<div class="sugg-part"><span class="small muted">${label}</span><ul class="sugg-list">${(e.evidence || []).map(x => `<li>${esc(x.shows)} <span class="small muted">(${x.call ? `from ${esc(x.tool)}` : `task: ${esc(x.title || "your request")}`})</span></li>`).join("")}</ul></div>`;
}

// What became of a card that is no longer open, other than a schedule it made.
function cardOutcome(e) {
  if (e.state === "declined") return `<p class="small"><span class="chip">Declined</span> ${esc(e.title || "")}${e.reason ? ` <span class="muted">· ${esc(e.reason)}</span>` : ""}</p>`;
  if (e.state === "later") return `<p class="small"><span class="chip">Not now</span> ${esc(e.title || "")}</p>`;
  if (e.started) return `<p class="small"><span class="chip ok">Accepted</span> ${esc(e.title || "")} <a href="#/task/${enc(e.started)}" data-open="task:${esc(e.started)}">See the task</a></p>`;
  if (e.kind === "grant_card" && e.granted) return `<p class="small"><span class="chip ok">Granted</span> ${esc(e.title || "")} <a href="#/settings/data">See it in Settings</a></p>`;
  return "";
}

function scheduleLine(e) {
  if (e.state !== "confirmed") return cardOutcome(e);
  const sc = App.schedules.find(x => x.id === e.schedule);
  if (!sc) return `<p class="small" data-schedule-gone="${esc(e.schedule || "")}"><span class="chip">Details unavailable</span> ${esc(e.title || "This schedule")} · Schedule details are not currently available.</p>`;
  const act = (k, label) => `<button class="link small" data-sched-act="${k}" data-schedule-id="${esc(sc.id)}">${label}</button>`;
  const chip = sc.ended ? `<span class="chip">Ended</span>` : `<span class="chip sch">${sc.paused ? "Paused" : "Scheduled"}</span>`;
  const acts = sc.ended ? act("cancel", "Delete") : `${sc.paused ? act("resume", "Resume") : act("pause", "Pause")}${act("cancel", "Cancel")}`;
  return `<div class="reqgroup sched-state" data-schedule="${esc(sc.id)}"><a class="rg-row" href="#/schedules" data-open="schedule:${esc(sc.id)}" title="Open the schedule">
    <span class="st sch">${icon("calendar")}</span><span class="rg-main"><span class="ttl">${esc(sc.title)}</span><span class="sub" data-last>${esc(scheduleText(sc))}</span></span>${chip}${icon("chev", "chev")}</a>
    <div class="card-acts">${acts}<a class="small" href="#/schedules" data-open="schedule:${esc(sc.id)}">Open</a>${sc.started ? `<a class="small" href="#/chat/${enc(`schedule-${sc.id}`)}" data-schedule-runs>Its runs</a>` : ""}</div></div>`;
}

// Motion (U7): what is new in the thread rises into place, a task whose
// status changed shows its new status gently, and finished work gets a quiet
// moment. A repaint draws every node afresh, so only what changed is marked;
// the stylesheet decides, by the motion setting, how each mark moves, if at
// all.
function motion(el, ui) {
  const first = !ui.seen;
  ui.seen ||= new Set();
  ui.status ||= {};
  for (const n of $$(".entry[data-entry]", el)) {
    const k = n.dataset.entry;
    if (!first && k && !ui.seen.has(k)) {
      n.classList.add("enter");
      n.addEventListener("animationend",()=>n.classList.remove("enter"),{once:true});
    }
    ui.seen.add(k);
  }
  for (const n of $$("[data-chat-task]", el)) {
    const k = n.dataset.chatTask, st = n.dataset.status, was = ui.status[k];
    if (was && was !== st) n.classList.add(st === "completed" ? "finished" : "changed");
    ui.status[k] = st;
  }
}

// The clipboard where the page may use it; the old way where it may not.
async function copyText(t) {
  try { await navigator.clipboard.writeText(t); return; } catch { /* fall back */ }
  const a = document.createElement("textarea");
  a.value = t; a.style.position = "fixed"; a.style.opacity = "0"; document.body.append(a); a.select();
  const ok = document.execCommand("copy"); a.remove();
  if (!ok) throw new Error("This app may not use the clipboard.");
}

// What a message says, as text to copy.
const entryText = e => e.kind === "result" ? `${e.title} (${e.status})\n\n${e.summary || ""}`
  : e.kind === "tasks" ? e.tasks.map(t => `- ${t.title}: ${t.text}`).join("\n") : e.text || "";
const safeName = e => `${(entryText(e).split(/\s+/).slice(0, 6).join(" ").replace(/[^\p{L}\p{N} -]/gu, " ").trim().replace(/\s+/g, "-") || "message")}.md`;

// Icons for what can be done to a chat entry: the shapes inside each 24px
// outline icon from Lucide (lucide-static 0.563.0), under this licence:
// ISC License
//
// Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2026 as part of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2026.
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
//
// ---
//
// The MIT License (MIT) (for portions derived from Feather)
//
// Copyright (c) 2013-2026 Cole Bemis
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
const ICONS = {
  copy: '<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>', // copy
  edit: '<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/><path d="m15 5 4 4"/>', // pencil
  retry: '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>', // rotate-ccw
  listen: '<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z"/><path d="M16 9a5 5 0 0 1 0 6"/><path d="M19.364 18.364a9 9 0 0 0 0-12.728"/>', // volume-2
  hush: '<circle cx="12" cy="12" r="10"/><rect x="9" y="9" width="6" height="6" rx="1"/>', // circle-stop
  branch: '<line x1="6" x2="6" y1="3" y2="15"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/>', // git-branch
  share: '<circle cx="18" cy="5" r="3"/><circle cx="6" cy="12" r="3"/><circle cx="18" cy="19" r="3"/><line x1="8.59" x2="15.42" y1="13.51" y2="17.49"/><line x1="15.41" x2="8.59" y1="6.51" y2="10.49"/>', // share-2
  export: '<path d="M12 15V3"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/>', // download
  save: '<path d="M12 10v6"/><path d="M9 13h6"/><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>', // folder-plus
  task: '<path d="M16 5H3"/><path d="M11 12H3"/><path d="M16 19H3"/><path d="M18 9v6"/><path d="M21 12h-6"/>', // list-plus
  great: '<circle cx="12" cy="12" r="10"/><path d="M8 14s1.5 2 4 2 4-2 4-2"/><line x1="9" x2="9.01" y1="9" y2="9"/><line x1="15" x2="15.01" y1="9" y2="9"/>', // smile
  annoying: '<circle cx="12" cy="12" r="10"/><path d="M16 16s-1.5-2-4-2-4 2-4 2"/><line x1="9" x2="9.01" y1="9" y2="9"/><line x1="15" x2="15.01" y1="9" y2="9"/>', // frown
  delete: '<path d="M10 11v6"/><path d="M14 11v6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M3 6h18"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>', // trash-2
};

// What a person can do with each entry. Edit is for their own messages,
// Retry for Magnus's answers, and Read aloud for an answer, from a script
// written for listening; a reaction (great or annoying, pressed again to
// take it back) for each of Magnus's messages; the rest for every entry. Each is an icon, named
// by its tooltip and, for screen readers, by hidden text.
function msgActions(e, ui) {
  if (!e.id || e.kind === "sub_agents") return "";
  const mine = e.kind === "message", magnus = e.kind === "answer" || e.kind === "turn";
  const said = ["answer", "turn", "result", "tasks", "grant_card", "suggestion_card", "schedule_card", "connector_card", "opener"].includes(e.kind);
  const id = `message-menu-${enc(e.id)}`, reading = voice.entry === e.id;
  const button = (a, label) => `<button class="icon" data-msg-act="${a}"${a==="listen" || a==="hush" ? ` data-voice-entry="${esc(e.id)}"` : ""} aria-label="${label}" data-tooltip="${label}"><svg viewBox="0 0 24 24" aria-hidden="true">${ICONS[a]}</svg></button>`;
  const primary = mine ? button("edit", "Edit") : button("copy", "Copy response") + (e.kind === "answer" && App.canReadAloud ? button(reading ? "hush" : "listen", reading ? "Stop reading" : "Read aloud") : "");
  const actions = [mine && ["copy", "Copy message"], magnus && ["retry", "Retry"], ["branch", "Branch"], ["share", "Share"], ["export", "Export"], ["save", "Save to Files"], ["task", "Make a task"], said && ["feedback", "Feedback"], ["delete", "Delete message"]].filter(Boolean);
  const share = ui.sharing === e.id ? `<div class="row small share-to"><label class="sr" for="share-${esc(e.id)}">Share with</label>
    <select id="share-${esc(e.id)}" data-share-to>${ui.people.map(p => `<option value="${esc(p.id)}">${esc(p.name)}</option>`).join("")}</select>
    <button class="primary" data-msg-act="share-send">Share</button><button class="link" data-msg-act="share-cancel">Cancel</button></div>` : "";
  return `<div class="msg-actions small" role="group" aria-label="Message actions">${primary}<div class="message-menu-wrap"><button class="icon menu-trigger" data-menu="${esc(id)}" aria-label="More message actions" data-tooltip="More message actions" aria-haspopup="menu" aria-expanded="false" aria-controls="${esc(id)}"><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/></svg></button>
    <div id="${esc(id)}" class="action-menu" role="menu" aria-label="More message actions" hidden>${actions.map(([a, label]) => `<button role="menuitem" tabindex="-1" data-msg-act="${a}">${label}</button>`).join("")}</div></div></div>${share}`;
}

function entry(e, ui = {}) {
  switch (e.kind) {
    case "message": return ui.editing === e.id
      ? `<div class="msg me editing"><label class="sr" for="edit-${esc(e.id)}">Edit and resend</label><textarea id="edit-${esc(e.id)}" rows="3" data-draft="edit-${esc(e.id)}"></textarea>
        <div class="row small"><button class="primary" data-msg-act="edit-send">Send</button><button class="link" data-msg-act="edit-cancel">Cancel</button></div></div>`
      : `<div class="msg me"><div class="small faint">You · ${esc(when(e.at))}</div><div>${Compose.maskedText(e.text)}</div>${e.files?.length ? `<div class="previews">${e.files.map(filePreview).join("")}</div>` : ""}</div>`;
    case "opener": return `<div class="msg magnus" data-opener><div class="small faint">Magnus · ${esc(when(e.at))}</div><div class="small muted">${e.app_title ? `You connected ${esc(e.app_title)}, so I had a look at what I could take over there.` : "You are new here, so I had a look at what I could take over for you."}</div></div>`;
    case "answer": return `<div class="msg magnus" data-answer data-task="${esc(e.task_id)}"><div class="small faint">Magnus${e.model ? ` · ${esc(e.model.model)}` : ""} · ${esc(when(e.at))}</div><div class="answer-body">${Compose.markdown(e.text || "")}</div>${e.view ? shownFrame(e.id, "What Magnus showed") : ""}</div>`;
    case "turn": return e.status === "failed" || e.status === "cancelled"
      ? `<div class="msg magnus"><div class="small bad">Could not answer: ${esc(e.text || e.status)}</div></div>`
      : `<div class="msg magnus"><div class="small faint">${e.status === "running" ? "Working on an answer…" : esc(WAITING[e.waiting_on] || "paused")}</div>${e.status === "running" ? `<div class="pre" data-live="${esc(e.task_id)}">${Compose.markdown(ui.live?.[e.task_id] || "")}</div>` : ""}</div>${waitingCard(e.waiting_item, ui)}`;
    case "tasks": {
      const done = e.tasks.filter(t => t.status === "completed").length, need = e.tasks.filter(needsYou).length;
      return `<div class="reqgroup chatcard" data-chat-tasks="${esc(e.from_run)}"><div class="rg-head"><span>From this request</span><span class="small muted num">${done} of ${e.tasks.length} done${need ? ` · ${need} need${need === 1 ? "s" : ""} you` : ""}</span></div>
        <ul class="rg-rows">${e.tasks.map(taskCard).join("")}</ul></div>
        ${e.tasks.map(t => t.waiting_item && waitingCard(t.waiting_item, ui) ? `<div class="decide" data-for-task="${esc(t.task_id)}">${waitingCard(t.waiting_item, ui)}</div>` : "").join("")}`;
    }
    case "sub_agents": return subAgentsCard(e, ui);
    case "result": return `<div class="result" data-result="${esc(e.task_id)}">${ui.movedOn?.has(e.task_id) ? `<blockquote class="asked small"><span class="faint">You asked:</span> ${esc(firstLine(e.text))}</blockquote>` : ""}<details data-key="result:${esc(e.task_id)}"><summary>${chip(e.status)} <span class="ttl">${esc(e.title)}</span> <span class="muted">${esc(firstLine(e.summary))}</span></summary>
      <div class="pre">${esc(e.summary || "")}</div></details>
      ${e.view ? shownFrame(e.id, "The deliverable") : ""}
      ${e.files?.length ? `<div class="previews">${e.files.map(filePreview).join("")}</div>` : ""}${evidenceSection(e)}<a class="small see" href="#/task/${enc(e.task_id)}" data-open="task:${esc(e.task_id)}">See the outcome</a></div>`;
    case "connector_card": return appOfferCard(e);
    case "suggestion_card": {
      if (e.state !== "open") return `<div data-card="${esc(e.card)}">${e.schedule ? scheduleLine(e) : cardOutcome(e)}</div>`;
      const timed = e.every_minutes || e.daily_at;
      return `<div class="confirm chatcard suggestion" data-card="${esc(e.card)}"><div class="cc-head"><span class="st sch">${icon(timed ? "calendar" : "tasks")}</span><strong>${esc(e.title)}</strong></div>
        <p class="cc-text">${esc(e.plan)}</p>
        ${e.changed ? `<p class="small"><strong>What changed:</strong> ${esc(e.changed)}</p>` : ""}
        ${evidenceList(e, "Why")}
        <div class="sugg-part"><span class="small muted">What it may do</span><ul class="sugg-list">${(e.permissions || []).map(x => `<li>${esc(x)}</li>`).join("")}</ul></div>
        ${timed ? `<div class="tiles"><div><span>When</span><strong>${esc(rule(e))}</strong></div><div><span>Ends</span><strong>${esc(ends(e))}</strong></div></div>` : ""}
        <p class="small muted">It acts for you (${esc(e.acts_for)}), with your access; what it needs from you shows under Needs you.</p>${taintNote(e.taint)}
        ${cardActs(e, ui, "Accept")}</div>`;
    }
    case "grant_card": {
      if (e.state !== "open") return `<div data-card="${esc(e.card)}">${cardOutcome(e)}</div>`;
      return `<div class="confirm chatcard suggestion grant" data-card="${esc(e.card)}"><div class="cc-head"><span class="st sch">${icon("tasks")}</span><strong>Let Magnus handle this on its own?</strong></div>
        <p class="cc-text">${esc(e.why)}</p>
        ${grantForm(e.card, e, e.your_apps)}
        <p class="small muted">Tasks Magnus starts for this go ahead without asking, as you (${esc(e.acts_for)}), within these limits and your organisation's policy; it reports every action. Change or revoke it in Settings.</p>${taintNote(e.taint)}
        ${cardActs(e, ui, "Accept", false)}</div>`;
    }
    // A schedule set up at the person's request: shown as set, with no confirmation (spec rev 140).
    case "schedule_card": return `<div data-card="${esc(e.card)}">${scheduleLine(e)}</div>`;
    default: return "";
  }
}

route(/^#\/chat\/([^/]+)$/, async (main, thread) => {
  if (thread === "new") return showNewChat(main);
  return showThread(main, thread);
}, true);

// ---------- Inbox ----------

const KIND = {
  approval: "Approval", question: "Question", review: "Review", notice: "Stopped", message: "Note from the agent", observation: "Observation", interrupt: "Interruption",
  policy_proposal: "Policy proposal", suggestion: "Suggestion", app_tools: "App changed",
  monitor: "Safety monitor", secure_form: "Secure sign-in", takeover_request: "Take over", payment: "Purchase", contract_change: "Contract change", deletion_request: "Deletion request", shared: "Shared with you", endorsement: "Company fact?",
};
const TIER = { read: "reads", sandbox: "workspace only", reversible_write: "can be undone", irreversible: "cannot be undone", financial: "money", prohibited: "prohibited" };
const AUDIENCE = { private: "only you", org: "people in your organization", public: "anyone" };

function fieldValue(v) {
  if (v === null || v === undefined) return `<span class="faint">none</span>`;
  if (typeof v === "string") return `<pre>${esc(v)}</pre>`;
  if (typeof v === "number" || typeof v === "boolean") return `<pre>${esc(String(v))}</pre>`;
  return `<pre>${esc(JSON.stringify(v, null, 2))}</pre>`;
}

function taintNote(t) {
  if (!t || (!t.untrusted && !t.private)) return "";
  const parts = [];
  if (t.untrusted) parts.push("has read content from outside sources, which could be steering it");
  if (t.private) parts.push("has read private data");
  return `<p class="small muted">This task ${parts.join(" and ")}.</p>`;
}

function buttons(i, list) {
  if (!i.can_respond) return `<p class="small faint">Waiting for someone with the right role to decide.</p>`;
  return `<div class="row" style="margin-top:12px">${list.map(([d, label, cls, extra]) => `<button class="${cls || ""}" data-item="${esc(i.id)}" data-decision="${d}" ${extra || ""}>${esc(label)}</button>`).join("")}</div>`;
}

function card(i, composed) {
  const d = i.details || {};
  const tier = i.kind === "approval" ? (i.tier || d.risk) : "";
  const head = `<div class="spread"><span class="kind">${esc(KIND[i.kind] || i.kind)}${tier ? ` · ${esc(TIER[tier] || tier)}` : ""}</span>
    <span class="small faint">${i.blocking ? `<strong style="color:var(--fg)">A task is waiting</strong> · ` : ""}${esc(when(i.created_at))}${i.task_id && i.person === App.me?.id ? ` · <a href="#/task/${enc(i.task_id)}" data-open="task:${esc(i.task_id)}">task</a>` : ""}</span></div>`;
  let body = "";
  if (i.status !== "open") {
    const r = i.resolution || {};
    if (i.kind === "approval") return `<div class="card flat item approval">${head}${askResolved(i)}</div>`;
    return `<div class="card flat item ${esc(i.kind)} ${esc(tier)}">${head}<h4>${esc(i.summary)}</h4><p class="small muted">${esc(r.decision)}${r.scope ? ` (${esc(r.scope)})` : ""} by ${esc(String(r.by || "").replace(/^person:/, ""))}${r.text ? `: ${esc(r.text)}` : ""}</p></div>`;
  }
  switch (i.kind) {
    // An ask: the server's one sentence, the content one tap away, and
    // its answers (asks.js). In the conversation the work came from too.
    case "approval":
      return `<article class="card item approval" data-id="${esc(i.id)}">${head}${i.app ? appOfferCard(i.app, i.person === App.me?.id) : ""}${askCard(i, i.can_respond)}</article>`;
    case "question": {
      const opts = Array.isArray(d.options) ? d.options : [];
      body = `${taintNote(d.taint)}${i.can_respond ? `${opts.length ? `<div class="row">${opts.map(o => `<button data-item="${esc(i.id)}" data-decision="answer" data-text="${esc(o)}">${esc(o)}</button>`).join("")}</div>` : ""}
        <textarea rows="2" data-draft="ans-${esc(i.id)}" placeholder="Your answer" style="margin-top:10px"></textarea>
        <div class="row" style="margin-top:8px"><button class="primary" data-item="${esc(i.id)}" data-decision="answer">Send answer</button></div>` : buttons(i, [])}`;
      break;
    }
    // Asked before rev 140, when checking became internal: the criteria
    // stand whatever is answered, and the task goes on against them.
    case "contract_change":
      body = `<p class="small">This task no longer needs you here: it goes on against the criteria it set.</p>${buttons(i, [["reject", "Continue", "primary"]])}`;
      break;
    case "review": {
      const crit = (d.criteria || []).map(c => `<li>${esc(c.description || c.id || c)}${c.unchecked_reason ? `<div class="small faint">Its check could not run: ${esc(c.unchecked_reason)}</div>` : ""}</li>`).join("");
      body = `<p>${esc(d.summary)}</p>
        <div class="evidence none"><div class="small muted">The server could not check these. Check them yourself:</div><ul>${crit}</ul></div>
        ${d.view ? `<iframe class="composed" sandbox="allow-same-origin" title="The outcome" data-view="${esc(i.id)}"></iframe>` : ""}
        ${taintNote(d.taint)}
        ${i.can_respond ? `<textarea rows="2" data-draft="rev-${esc(i.id)}" placeholder="If you send it back, say what is wrong" style="margin-top:10px"></textarea>` : ""}
        ${buttons(i, [["accept", "Accept", "primary"], ["reject", "Send back"]])}`;
      if (d.view) composed.push([i.id, d.view, i.run_id, i.task_id]);
      break;
    }
    case "notice": {
      const ev = d.evidence;
      const list = [];
      // A schedule's own notice stands for no run to continue: its Resume does that.
      if (typeof d.reason === "string" && !d.reason.startsWith("schedule_") && i.run_id) list.push(["resume", "Continue", "primary"]);
      if (d.reason === "budget") list.push(["extend", "Double the budget and continue"]);
      const askMore = typeof spendNoticeAction === "function" ? spendNoticeAction(d) : null;
      if (askMore) list.push(askMore);
      list.push(["dismiss", "Dismiss"]);
      body = `<p class="small"><span class="muted">Why it stopped:</span> ${esc(d.reason)}</p>
        ${ev ? `<div class="evidence fail"><div class="small muted">Evidence</div>${fieldValue(ev)}</div>` : ""}${buttons(i, list)}`;
      break;
    }
    case "policy_proposal":
      body = `<p class="small"><span class="muted">Layer:</span> <span class="mono">${esc(d.layer)}</span></p>
        ${d.rationale ? `<p>${esc(d.rationale)}</p>` : ""}<div class="what"><pre>${esc(d.cedar)}</pre></div>${taintNote(d.taint)}
        ${buttons(i, [["accept", "Add this rule", "primary"], ["reject", "Reject"]])}`;
      break;
    // Magnus's own idea from a task or a review: one tap starts it (spec rev
    // 140). Proposals from before then read as suggestions.
    case "suggestion": {
      const timed = d.every_minutes || d.daily_at;
      body = `<div class="what"><div class="field"><span>${timed ? "Each run" : "Task"}</span><span>${esc(d.plan || d.text)}</span></div>
        ${timed ? `<div class="field"><span>When</span><span>${esc(rule(d))}</span></div><div class="field"><span>Ends</span><span>${esc(ends(d))}</span></div>` : ""}
        ${(d.permissions || d.needs?.permissions || []).length ? `<div class="field"><span>May</span><span>${esc((d.permissions || d.needs?.permissions).join("; "))}</span></div>` : ""}</div>
        ${(d.evidence || []).length ? `<div class="evidence"><div class="small muted">Why</div><ul class="small">${(d.evidence || []).map(c => `<li>${esc(c.title || c.tool || c.call || "")}: “${esc(c.shows)}”</li>`).join("")}</ul></div>` : ""}${taintNote(d.taint)}
        ${buttons(i, [["accept", timed ? "Schedule it" : "Start it", "primary"], ["reject", "Dismiss"]])}`;
      break;
    }
    case "message":
      body = `${taintNote(d.taint)}${buttons(i, [["dismiss", "Dismiss"]])}`;
      break;
    case "observation":
    case "interrupt":
      body = `${d.why ? `<p>${esc(d.why)}</p>` : ""}${d.evidence ? `<div class="evidence"><div class="small muted">Evidence</div>${fieldValue(d.evidence)}</div>` : ""}${d.notify_at && Date.parse(d.notify_at) > Date.now() ? `<p class="small faint">Pings ${esc(when(d.notify_at))}.</p>` : ""}${taintNote(d.taint)}${buttons(i, [["dismiss", "Dismiss"]])}`;
      break;
    case "shared":
      body = `<div class="msg magnus"><div class="small faint">${esc(d.speaker)} · ${esc(when(d.at))}</div><div class="pre">${esc(d.text)}</div></div>${taintNote(d.taint)}
        ${buttons(i, [["dismiss", "Dismiss"]])}`;
      break;
    case "deletion_request":
      body = `<div class="what"><div class="field"><span>From</span><span>${esc(d.requested_by)}</span></div><div class="field"><span>What</span><span>${esc(d.what)}</span></div>
        <div class="field"><span>Due</span><span>${esc(new Date(d.due).toLocaleDateString())}</span></div></div>
        ${i.can_respond ? `<textarea rows="2" data-draft="ans-${esc(i.id)}" placeholder="A note for them: what was deleted, or why not" style="margin-top:10px"></textarea>` : ""}
        ${buttons(i, [["done", "Deleted", "primary"], ["decline", "Decline"]])}`;
      break;
    // An entry to make a company fact: what it says, where it came from, and
    // what the appliance found when it checked its citations.
    case "endorsement": {
      const ev = d.verification?.evidence || [];
      const how = { corroborated: "found in independent sources", system_of_record: "found in a system of record" }[d.verification?.how];
      body = `<div class="msg magnus"><div class="pre">${esc(d.text)}</div></div>
        <div class="what"><div class="field"><span>Now</span><span>${esc(d.integrity)}${how ? ` (${esc(how)})` : ""}</span></div>
        <div class="field"><span>Readable by</span><span>${esc((d.readers || []).length ? d.readers.map(c => c.join(" or ")).join(", and ") : "everyone")}</span></div>
        <div class="field"><span>Asked by</span><span>${esc(d.requested_by)}</span></div>
        <div class="field"><span>Source</span>${ev.length ? `<ul class="small">${ev.map(c => `<li>${c.found ? "✓" : "✗ not found in"} ${esc(c.source)}: “${esc(c.quote)}”</li>`).join("")}</ul>` : fieldValue(d.source)}</div></div>
        ${i.can_respond ? `<textarea rows="2" data-draft="ans-${esc(i.id)}" placeholder="A note: how you checked it (optional)" style="margin-top:10px"></textarea>` : ""}
        ${buttons(i, [["endorse", "Make it a company fact", "primary"], ["decline", "Decline"]])}`;
      break;
    }
    case "monitor":
      body = `<div class="what"><div class="field"><span>Action</span>${fieldValue(d.tool || d.action)}</div><div class="field"><span>Why it was held</span>${fieldValue(d.reason)}</div></div>
        ${buttons(i, [["allow", "Allow", "primary"], ["reject", "Reject"]])}`;
      break;
    case "secure_form":
      body = secureFormCard(i);
      break;
    case "takeover_request":
      body = takeoverRequestCard(i);
      break;
    case "payment":
      body = paymentCard(i);
      break;
    // One card per app, never per tool, and never a description (spec rev 140).
    case "app_tools":
      body = appToolsCard(i);
      break;
    default:
      body = `<div class="what">${fieldValue(d)}</div>`;
  }
  return `<article class="card item ${esc(i.kind)} ${esc(tier)}" data-id="${esc(i.id)}">${head}<h4>${esc(i.summary)}</h4>${body}</article>`;
}

// Draw composed outcomes into their frames, from trusted code only.
const fontCss = () => {
  const u = f => new URL(`fonts/${f}`, location.href).href;
  return `@font-face{font-family:Inter;font-weight:100 900;src:url(${u("inter-latin-wght-normal.woff2")}) format("woff2")}`;
};
// A task's delivered files, the latest of each name.
const latestOutputs = t => Object.values(Object.fromEntries((t?.outputs || []).map(o => [o.name, o])));
const decodeText = u => new TextDecoder().decode(Uint8Array.from(atob(u.slice(u.indexOf(",") + 1)), c => c.charCodeAt(0)));
// The delivered files a composition shows, fetched by the app itself:
// images as data: URLs, text files decoded for drawing.
async function fileData(view, t) {
  const out = {};
  const names = new Set((view || []).filter(c => c.type === "image" || c.type === "file").map(c => c.file));
  for (const o of latestOutputs(t).filter(o => names.has(o.name))) {
    const path = `/v1/tasks/${enc(t.task_id)}/outputs/${enc(o.id)}`;
    try {
      if (/^image\/(png|jpeg|gif|webp)$/.test(o.media_type)) out[o.name] = { image: await Magnus.dataUrl(path, o.media_type) };
      else if (Compose.TEXT.test(o.media_type)) out[o.name] = { text: decodeText(await Magnus.dataUrl(path, o.media_type)), media_type: o.media_type };
    } catch { /* shown as missing */ }
  }
  return out;
}
// The look in effect, for what a frame draws: its tokens, and light or dark.
const look = () => { const l = window.Theme?.look; return l ? [Theme.tokens(l), l.mode] : []; };

// The height each composed frame in a conversation last had, by its entry:
// a redraw of the thread makes the frame anew, and drawn at its known height
// the thread keeps its layout (and its scroll) while the frame loads again.
const frameHeights = {};
// (Set after the thread is drawn, not in its markup, so the markup stays the
// same and a loaded frame does not make the thread draw itself again.)
const shownFrame = (id, title) => `<iframe class="composed" sandbox="allow-same-origin" title="${title}" data-shown="${esc(id)}"></iframe>`;
const keepHeights = el => $$("iframe[data-shown]", el).forEach(f => { if (frameHeights[f.dataset.shown]) f.style.height = frameHeights[f.dataset.shown] + "px"; });
const frameSizing = new Map();
// Native WebKit blocks frame event handlers when scripts are sandboxed. Keep
// actions in the trusted parent, aligned to inert reading-surface markers.
function alignFrameActions(frame,state) {
  if(!frame.isConnected || frame.contentDocument!==state.doc)return;
  const clips=new Map(), geometry=[];
  for(const control of state.controls) {
    const marker=control._marker;if(!marker.isConnected)continue;
    const r=marker.getBoundingClientRect();
    let left=0,top=0,right=frame.clientWidth,bottom=frame.clientHeight;
    for(let node=marker.parentElement;node;node=node.parentElement) {
      let clip=clips.get(node);
      if(!clip){const style=state.doc.defaultView.getComputedStyle(node),box=node.getBoundingClientRect();clip={x:/auto|scroll|hidden|clip/.test(style.overflowX),y:/auto|scroll|hidden|clip/.test(style.overflowY),left:box.left+node.clientLeft,top:box.top+node.clientTop,right:box.left+node.clientLeft+node.clientWidth,bottom:box.top+node.clientTop+node.clientHeight};clips.set(node,clip);}
      if(clip.x){left=Math.max(left,clip.left);right=Math.min(right,clip.right);}
      if(clip.y){top=Math.max(top,clip.top);bottom=Math.min(bottom,clip.bottom);}
    }
    // Keep the 2px outline and 2px offset visible inside the scrollport.
    const visible=Math.min(right,r.right)>Math.max(left,r.left) && Math.min(bottom,r.bottom)>Math.max(top,r.top);
    geometry.push([control,{left:`${r.left}px`,top:`${r.top}px`,width:`${r.width}px`,height:`${r.height}px`,clipPath:visible?`inset(${Math.max(-4,top-r.top)}px ${Math.max(-4,r.right-right)}px ${Math.max(-4,r.bottom-bottom)}px ${Math.max(-4,left-r.left)}px)`:'inset(50%)'}]);
  }
  // Read frame geometry together before writing parent overlay styles.
  for(const [control,style] of geometry)Object.assign(control.style,style);
  state.scrollports=new Map([...clips].filter(([,clip])=>clip.x || clip.y).map(([node])=>[node,[node.scrollLeft,node.scrollTop]]));
  const root=state.doc.scrollingElement;if(root)state.scrollports.set(root,[root.scrollLeft,root.scrollTop]);
}
function frameActions(frame,doc) {
  let wrapper=frame.parentElement;
  if(!wrapper.classList.contains('composed-wrap')) {
    wrapper=document.createElement('div');wrapper.className='composed-wrap';frame.before(wrapper);wrapper.append(frame);
  }
  let state=frame._frameActions;
  if(state?.doc!==doc || state?.body!==doc.body) {
    state?.dispose();wrapper.querySelector('.frame-actions')?.remove();
    const layer=document.createElement('div');layer.className='frame-actions';wrapper.append(layer);
    let scheduled=0,poll=0,disposed=false,visible=false,visibility;
    state={doc,body:doc.body,controls:[],scrollports:new Map()};
    const align=()=>alignFrameActions(frame,state);
    const schedule=()=>{if(!disposed && !scheduled)scheduled=requestAnimationFrame(()=>{scheduled=0;align();});};
    for(const marker of doc.querySelectorAll('[data-code-copy-anchor], [data-prose-url]')) {
      const copy=marker.hasAttribute('data-code-copy-anchor'),control=document.createElement(copy?'button':'a');
      control.className=copy?'frame-code-copy':'frame-prose-link';
      if(copy){control.type='button';control.dataset.codeCopy='';control.textContent='Copy code';control.setAttribute('aria-label','Copy code');control.dataset.tooltip='Copy code';
        const text=marker.closest('.code-block').querySelector('code').textContent;
        control.onclick=async e=>{e.preventDefault();e.stopPropagation();Tooltips.dismiss();try{await copyText(text);toast('Code copied.');}catch(err){toast(err.message);}};
      }else{control.textContent=marker.textContent;control.setAttribute('aria-label',marker.textContent);control.href=marker.dataset.proseUrl;control.rel='noopener noreferrer';control.onclick=e=>{e.preventDefault();Magnus.openExternal(control.href).catch(err=>toast(err.message));};}
      marker.style.scrollMargin='4px';
      control.onfocus=()=>{marker.scrollIntoView({block:'nearest',inline:'nearest'});align();};
      layer.append(control);control._marker=marker;state.controls.push(control);
    }
    const pollScroll=()=>{
      poll=0;if(disposed || !visible || !frame.isConnected)return;
      // Sandboxed WebKit may omit subdocument scroll events. Checking the
      // few scrollport offsets is cheap; measure links only after movement.
      if(!document.hidden && [...state.scrollports].some(([node,at])=>node.scrollLeft!==at[0] || node.scrollTop!==at[1]))align();
      poll=requestAnimationFrame(pollScroll);
    };
    if(state.controls.length){
      doc.addEventListener('scroll',schedule,true);doc.fonts?.addEventListener('loadingdone',schedule);doc.fonts?.ready.then(schedule);
      if('IntersectionObserver' in window){visibility=new IntersectionObserver(entries=>{if(disposed)return;visible=entries[0].isIntersecting;if(visible){align();if(!poll)poll=requestAnimationFrame(pollScroll);}else{cancelAnimationFrame(poll);poll=0;}});visibility.observe(frame);}
      else{visible=true;poll=requestAnimationFrame(pollScroll);}
    }
    state.dispose=()=>{disposed=true;cancelAnimationFrame(scheduled);cancelAnimationFrame(poll);visibility?.disconnect();doc.removeEventListener('scroll',schedule,true);doc.fonts?.removeEventListener('loadingdone',schedule);};
    frame._frameActions=state;
  }
  alignFrameActions(frame,state);
}
function sizeComposedFrame(frame) {
  try {
    const doc=frame.contentDocument;
    if(!frame.isConnected || !frame.hasAttribute("srcdoc") || !doc?.body || doc.URL==="about:blank")return;
    let sizing=frameSizing.get(frame);
    if(sizing?.doc!==doc || sizing?.body!==doc.body) {
      sizing?.observer.disconnect();
      const measure=()=>{
        if(!frame.isConnected){frameSizing.get(frame)?.observer.disconnect();frame._frameActions?.dispose();delete frame._frameActions;frameSizing.delete(frame);return;}
        if(frame.contentDocument!==doc)return;
        const height=Math.ceil(Math.max(doc.body.scrollHeight,doc.body.getBoundingClientRect().height))+4;
        if(frame.style.height!==`${height}px`)frame.style.height=`${height}px`;
        if(frame.dataset.shown)frameHeights[frame.dataset.shown]=height;
        frameActions(frame,doc);
        frame.dataset.sized="1";
      };
      const observer=new ResizeObserver(measure);observer.observe(doc.body);
      sizing={doc,body:doc.body,observer,measure};frameSizing.set(frame,sizing);
    }
    sizing.measure();
  }catch{/* A frame still loading cannot be measured yet. */}
}

async function fill(frame, view, taskId, task) {
  if (!frame || frame._drawn) return;
  frame._drawn = true;
  const t = task || (view?.some(c => c.type === "image" || c.type === "file") ? await api("GET", `/v1/tasks/${enc(taskId)}`) : null);
  frame.removeAttribute("data-sized");
  frame.srcdoc = Compose.documentFor(view, await fileData(view, t), fontCss(), t?.preview, ...look());
  frame.onload=()=>{wireMarkdown(frame.contentDocument);wireActions(frame,view,taskId);sizeComposedFrame(frame);};
}

// A button in something the agent made starts a new request and approves nothing.
function wireActions(frame, view, taskId) {
  const at = (view || []).flatMap((c, i) => c.type === "action" ? [i] : []);
  frame.contentDocument?.querySelectorAll("button.action").forEach((b, n) => b.addEventListener("click", async () => {
    b.disabled = true;
    try { await api("POST", `/v1/tasks/${enc(taskId)}/actions/${at[n]}`); toast("Started as a new request"); }
    catch (e) { b.disabled = false; toast(e.message); }
  }));
}
async function respond(btn, root) {
  // Sent once, however many handlers see the click.
  if (btn.disabled) return;
  const id = btn.dataset.item;
  let [decision, scope] = btn.dataset.decision.split(":");
  // Asking for more spend is not an answer to the notice: it opens the ask.
  if (decision === "ask-more") { if (typeof askMoreFromNotice === "function") askMoreFromNotice(btn); return; }
  const body = { decision };
  if (scope) body.scope = scope;
  const box = $(`[data-draft="ans-${CSS.escape(id)}"]`, root) || $(`[data-draft="rev-${CSS.escape(id)}"]`, root);
  if (btn.dataset.text) body.text = btn.dataset.text;
  else if (box && box.value.trim()) body.text = box.value.trim();
  if (decision === "answer" && !body.text) { toast("Write an answer first."); return; }
  btn.disabled = true;
  try {
    await api("POST", `/v1/inbox/${enc(id)}`, body);
    delete drafts[`ans-${id}`]; delete drafts[`rev-${id}`]; delete drafts[`cc-${id}`];
    toast({ endorse: "It is now a company fact.", approve: "Allowed.", reject: "Done.", accept: "Accepted.", answer: "Sent.", dismiss: "Dismissed.", resume: "Continuing.", extend: "Budget doubled.", keep: "Kept.", disconnect: "Disconnected." }[decision] || "Done.");
    await refresh();
  } catch (e) { toast(e.message); btn.disabled = false; }
}

// Needs you: every decision waiting on the person (U1), most pressing
// first, each the same card as in its conversation, with a way there.
// Compact rows for the home feed: ended tasks link to their detail,
// items to the inbox below, where the open ones already wait (#367).
function feedRows(feed) {
  const rows = [];
  for (const t of feed.ended) rows.push(`<div class="feed-row"><a href="#/task/${enc(t.task_id)}">${esc(t.title || "A task")}</a> <span class="chip ${t.status === "failed" ? "bad" : "ok"}">${esc(t.status)}</span></div>`);
  for (const i of feed.arrived) rows.push(`<div class="feed-row"><a href="#/item/${enc(i.id)}" data-open="item:${esc(i.id)}">${esc(i.summary || i.kind)}</a> <span class="small muted">new ${esc(i.kind)}</span></div>`);
  for (const i of feed.decided) rows.push(`<div class="feed-row"><a href="#/item/${enc(i.id)}" data-open="item:${esc(i.id)}">${esc(i.summary || i.kind)}</a> <span class="small muted">${esc(i.resolution?.decision || "decided")}</span></div>`);
  return rows.join("");
}

function activityLinks(selected="activity") {
  return `<nav class="activity-views" aria-label="Activity views">${[["activity","Overview"],["tasks","Tasks"],["schedules","Schedules"]].map(([k,l])=>`<a href="#/${k}" data-open="${k}" ${selected===k ? 'aria-current="page"' : ""}>${l}</a>`).join("")}</nav>`;
}
function activityView(composed) {
  const feed=App.feed && (App.feed.ended.length || App.feed.arrived.length || App.feed.decided.length) ? `<section><h3>Since you were last here</h3>${feedRows(App.feed)}</section>` : "";
  const other=App.inbox.filter(i=>i.status==="open" && !needsDecision(i));
  const decided=App.inbox.filter(i=>i.status!=="open").sort((a,b)=>(b.created_at || "").localeCompare(a.created_at || "")).slice(0,15);
  return `${activityLinks()}${feed}<section><h3>Work</h3>${tasksView()}</section><section><h3>Schedules</h3>${schedulesView()}</section>
    ${other.length ? `<details data-key="other"><summary>Updates and work waiting on others (${other.length})</summary>${other.map(i=>card(i,composed)).join("")}</details>` : ""}
    ${decided.length ? `<details data-key="decided"><summary>Resolved decisions (${decided.length})</summary>${decided.map(i=>card(i,composed)).join("")}</details>` : ""}`;
}

function needsView(composed) {
  const open=needsMe();
  const now=open.filter(i=>i.blocking), later=open.filter(i=>!i.blocking);
  const where = i => {
    const t = i.thread && App.threads.find(x => x.thread === i.thread);
    return t ? `<div class="small where">In <a href="#/chat/${enc(t.thread)}">${esc(t.title)}</a></div>` : "";
  };
  const list = items => items.map(i => `<div class="needs-item" data-needs="${esc(i.id)}">${where(i)}${card(i, composed)}</div>`).join("");
  return `<h3>A task is waiting ${now.length ? `<span class="badge">${now.length}</span>` : ""}</h3>
    ${now.length ? list(now) : `<p class="empty">No task is waiting on you.</p>`}
    <h3>When you have a moment</h3>
    ${later.length ? list(later) : `<p class="empty">Nothing else.</p>`}
    `;
}

// ---------- Task detail ----------

function plan(t) {
  const units = t.plan?.units || [];
  if (!units.length) return `<p class="empty">No plan: it is optional, and shows progress when kept.</p>`;
  const mark = { done: "ok", active: "run", dropped: "", pending: "" };
  // Once the task has ended nothing is in progress: a unit the agent left
  // active or pending was simply not marked done.
  const ended = ["completed", "failed", "cancelled"].includes(t.status);
  const status = u => (ended && (!u.status || u.status === "active" || u.status === "pending")) ? ["", "not marked done"] : [mark[u.status] || "", u.status || "pending"];
  return `<p class="small faint">Version ${esc(t.plan_version)}. Statuses are the agent's own account.</p>
    <table class="t"><tbody>${units.map(u => { const [cls, label] = status(u); return `<tr><td class="mono small">${esc(u.id)}</td><td>${esc(u.goal)}${u.done_when ? `<div class="small muted">Done when ${esc(u.done_when)}</div>` : ""}</td><td><span class="chip ${cls}" data-unit-status="${esc(label)}">${esc(label)}</span></td></tr>`; }).join("")}</tbody></table>`;
}

function trace(tl) {
  const steps = tl.steps.map(s => `<div class="step"><div class="small faint">Step ${esc(s.step)}</div>
    ${s.text ? `<p>${esc(s.text)}</p>` : ""}
    ${s.calls.map(c => `<div class="call"><div><span class="mono">${esc(c.tool)}</span> <span class="small faint">issued by ${esc(c.issuer)}</span>
      ${(c.decisions || []).map(d => `<span class="chip ${d.outcome === "allow" || d.outcome === "dedupe" ? "ok" : d.outcome === "ask" ? "warn" : "bad"}">${esc(d.stage)}: ${esc(d.outcome)}</span>`).join(" ")}</div>
      <details><summary class="small">Input${c.result !== null && c.result !== undefined ? " and result" : ""}</summary>${fieldValue(c.input)}${c.result !== null && c.result !== undefined ? `<div class="small muted" style="margin-top:6px">Result${c.is_error ? " (error)" : ""}</div><pre>${esc(c.result)}</pre>` : ""}</details></div>`).join("")}
  </div>`).join("");
  const notices = (tl.notices || []).map(n => `<div class="evidence none small"><span class="muted">Kernel, step ${esc(n.step)}:</span> ${esc(n.text)}</div>`).join("");
  return (steps || `<p class="empty">No steps yet.</p>`) + (notices ? `<h3>Kernel notices</h3>${notices}` : "");
}

// What the outcome page draws first: the deliverable the agent composed
// (the summary when it gave none), then each delivered file it does not
// show, in full.
function deliverable(t) {
  const view = t.outcome_view ? [...t.outcome_view] : t.summary && t.status !== "running" ? [{ type: "text", text: t.summary }] : [];
  const shown = new Set(view.filter(c => c.type === "file" || c.type === "image").map(c => c.file));
  for (const o of latestOutputs(t)) if (!shown.has(o.name)) view.push({ type: "file", file: o.name });
  return view;
}

// ---------- The panel beside the conversation ----------

// Whatever the person opens (a task, a file, a schedule, a decision, or a
// view over them) shows here, beside the conversation, never instead of it.
// `what` is "kind:id" or a view's name.
const Panel = { anchors:{}, layers:{}, unwatch:null, opener:null, focusNext:false, drawn:null, what: null, layer: "outcome", seq: 0, live: null, liveFor: null, liveText: "", lib: { q: "", project: "" } };

function openPanel(what, options={}) {
  if (options.history && Panel.what!==what) {
    history.replaceState({...history.state,work:Panel.what},"",location.href);
    history.pushState({...history.state,work:what},"",location.href);
  }
  if (options.opener) Panel.opener=options.opener;
  if (options.focus) Panel.focusNext=true;
  if (Panel.what !== what) {
    if (Panel.what) Panel.layers[Panel.what]=Panel.layer;
    Panel.unwatch?.(); Panel.unwatch=null;
    Panel.layer=Panel.layers[what] || "outcome"; stopPanelLive();
  }
  Panel.what = what;
  const el = $("#detail");
  if (!el) return;
  // The panel glides in when it opens (as the motion setting allows).
  if (el.hidden) { el.classList.add("opening"); el.addEventListener("animationend", () => el.classList.remove("opening"), { once: true }); }
  el.hidden = false;
  document.body.classList.add("with-detail");
  sidebar();
  markOpen();
  Workspace.opened();
  return drawPanel();
}
// The conversation's row for what the panel shows is marked, without
// redrawing the thread.
function markOpen() {
  for (const r of document.querySelectorAll(".rg-row[data-open]")) r.classList.toggle("open", r.dataset.open === Panel.what);
}
function closePanel(restoreFocus=false) {
  Panel.unwatch?.(); Panel.unwatch=null;
  if (Panel.what) Panel.layers[Panel.what]=Panel.layer;
  if (restoreFocus && Panel.what) history.pushState({...history.state,work:null},"",location.href);
  stopPanelLive();
  hush();
  Panel.what = null; Panel.drawn=null;
  Panel.rendered=null;Resources.forget("Work");
  const el = $("#detail");
  if (!el) return;
  el.hidden = true;
  document.body.classList.remove("with-detail");
  paint(el, "");
  markOpen();
  if ($("#side")) sidebar();
  Workspace.closed();
  if (restoreFocus) {
    const target=Panel.opener?.isConnected ? Panel.opener : $("#conv-head h1");
    if (target) { target.tabIndex=target.tabIndex<0?-1:target.tabIndex; target.focus({preventScroll:true}); }
  }
  Panel.focusNext=false;
}
function stopPanelLive() {
  if (Panel.live) Panel.live();
  Panel.live = Panel.liveFor = null;
  Panel.liveText = "";
}

const PANEL_TITLES = { activity: "Activity", needs: "Needs you", tasks: "Tasks", schedules: "Schedules", files: "Files", projects: "Projects", task: "Task", file: "File", schedule: "Schedule", project: "Project", item: "Item", search: "Search", connector: "Connector" };

async function drawPanel() {
  const el = $("#detail");
  if (!el || !Panel.what) return;
  const n = ++Panel.seq;
  const at = Panel.what.indexOf(":");
  const [kind, id] = at < 0 ? [Panel.what, ""] : [Panel.what.slice(0, at), Panel.what.slice(at + 1)];
  const composed = [];
  let body, after = null, head = null, failed=false;
  try {
    ({ body, after, head } = await PANELS[kind](id, composed));
  } catch (e) {
    if(e.cancelled)return;
    failed=true;
    if(n===Panel.seq){Resources.bad("Work",e);if(Panel.rendered===Panel.what)return;}
    body = `<div class="banner bad">${esc(e.status === 404 ? "This is gone: it was deleted, or it is not yours." : e.message)}</div>`;
  }
  if (n !== Panel.seq || !Panel.what) return;
  if(!failed){Panel.rendered=Panel.what;Resources.good("Work");}
  const title = kind === "search" ? `Search: ${id}` : PANEL_TITLES[kind] || kind;
  // A view names itself in the head (a task: its title and where it
  // stands); the others are named by what they are.
  const entry=Panel.anchors[Panel.what] ||= {anchor:{follow:false,top:0}};
  if (Panel.drawn===Panel.what) { const current=ReadingAnchors.capture($(".detail-body",el)); if(current){entry.anchor=current;entry.anchor.follow=false;} }
  const prior=entry.anchor;
  const changed = paint(el, `<div class="detail-head"><div class="dh-main">${head || `<h2>${esc(title)}</h2>`}</div></div><div class="detail-body" data-panel="${esc(kind)}">${body}</div>`); liveStatusLines(el);
  if (!changed) {
    reportOpenedSoon();
    if (Panel.focusNext) { const heading=$("h2",el); if (heading) { heading.tabIndex=-1; heading.focus({preventScroll:true}); } Panel.focusNext=false; }
    return;
  }
  Panel.unwatch?.(); Panel.drawn=Panel.what;
  ReadingAnchors.restore($(".detail-body",el),prior);
  entry.anchor=prior; Panel.unwatch=ReadingAnchors.watch($(".detail-body",el),entry,false);
  for (const [iid, view, , task] of composed) fill($(`iframe[data-view="${CSS.escape(iid)}"]`, el), view, task);
  if (after) await after(el);
  loadGlimpses(el);
  if (Panel.focusNext && n===Panel.seq) { const heading=$("h2",el); if (heading) { heading.tabIndex=-1; heading.focus({preventScroll:true}); } Panel.focusNext=false; }
}

// A view whose content is already read (App's objects) or read here.
const plain = body => ({ body });
const PANELS = {
  activity: (_, composed) => plain(activityView(composed)),
  needs: (_, composed) => plain(needsView(composed)),
  tasks: () => plain(activityLinks("tasks") + tasksView()),
  schedules: () => plain(activityLinks("schedules") + schedulesView()),
  schedule: id => plain(scheduleView(id)),
  item: (id, composed) => {
    const i = App.inbox.find(x => x.id === id);
    return plain(i ? card(i, composed) : `<p class="empty">This decision is gone.</p>`);
  },
  task: taskView,
  file: fileView,
  files: filesView,
  connector: connectorView,
  projects: projectsView,
  project: projectView,
  search: searchView,
};

// An app Magnus offered, with why: the same card as in the Library, as the
// person stands with it now, in a conversation, a task or an approval
// (connectors spec, Inside work). Only the person whose app it is gets its
// button; anyone else deciding sees where they stand.
function appOfferCard(e, theirs = true) {
  const app = { title: e.title, listing: { letters: e.letters, tint: e.tint } };
  const STATE = { needs_attention: "Needs signing in again", needs_more_access: "Needs more access", available: "Not connected" };
  const act = e.state === "connected" ? `<span class="chip ok">Connected</span>`
    : !theirs ? `<span class="chip warn">${esc(STATE[e.state] || "Not in their Apps")}</span>`
    : e.state === "unavailable" ? `<span class="small muted">No longer in your Apps.</span>`
    : e.ready === false ? `<span class="small muted">Not ready on this server yet.</span>`
    : e.state === "needs_attention" ? `<button class="primary" data-act="connect" data-app="${esc(e.connector)}">Sign in to ${esc(e.title || "")} again</button>`
    : e.state === "needs_more_access" ? `<button class="primary" data-act="connect" data-app="${esc(e.connector)}">Approve more access</button>`
    : `<button class="primary" data-act="connect" data-app="${esc(e.connector)}">Connect ${esc(e.title || "")}</button>`;
  return `<div class="confirm chatcard conncard" data-connector="${esc(e.connector)}" data-state="${esc(e.state)}"><div class="cc-head">${tile(app)}<div><strong>${esc(e.title || e.connector)}</strong>${e.publisher ? ` <span class="small muted">by ${esc(e.publisher)}</span>` : ""}</div></div>
    ${e.why ? `<p class="cc-text">${esc(e.why)}</p>` : ""}${e.summary ? `<p class="small muted">${esc(e.summary)}</p>` : ""}
    <div class="row cc-act">${act}</div></div>`;
}

// A task: its request, the decisions it waits on (the same cards as in its
// conversation), then its outcome, what was asked and its plan, or its
// trace. How the agent checked its work is internal (spec rev 140): it is in
// the developer view and the run inspector, not here.
async function taskView(id, composed) {
  const [t, tl] = await Promise.all([api("GET", `/v1/tasks/${enc(id)}`), Panel.layer === "trace" ? api("GET", `/v1/tasks/${enc(id)}/timeline`) : null]);
  const running = t.status === "running";
  if (running && Panel.liveFor !== id) {
    stopPanelLive();
    Panel.liveFor = id;
    Panel.live = Magnus.live(id, txt => { Panel.liveText += txt; const el = $("#plive"); if (el) el.textContent = Panel.liveText; });
  }
  if (!running && Panel.liveFor === id) stopPanelLive();
  const items = App.inbox.filter(i => i.task_id === id && i.status === "open");
  const view = deliverable(t);
  const files = latestOutputs(t);
  const cost = Number.isFinite(t.usage?.cost_usd) ? `$${t.usage.cost_usd.toFixed(2)}` : null;
  // The result leads with the deliverable, then its files, the person's
  // verdict; what it cost closes it.
  const outcome = `${view.length ? `<iframe class="composed" sandbox="allow-same-origin" title="The deliverable" id="pview"></iframe>` : running ? "" : `<p class="empty">No result yet.</p>`}
    ${t.spoken && App.canReadAloud ? `<div class="row" style="margin:10px 0"><button type="button" class="icon-btn" data-act="${voice.entry===`task:${id}` ? "hush" : "speak"}" data-voice-entry="task:${esc(id)}" aria-label="${voice.entry===`task:${id}` ? "Stop reading" : "Read aloud"}" data-tooltip="${voice.entry===`task:${id}` ? "Stop reading" : "Read aloud"}"><svg class="ic" viewBox="0 0 24 24" aria-hidden="true">${ICONS[voice.entry===`task:${id}` ? "hush" : "listen"]}</svg></button></div>` : ""}
    ${files.length ? `<div class="frows">${files.map(f => fileRow(f, t.project)).join("")}</div>` : ""}
    ${evidenceSection(t)}
    ${t.outcome_view && t.summary ? `<h3>Summary</h3><p data-summary>${esc(t.summary)}</p>` : ""}
    ${ENDED.includes(t.status) ? `<details data-work-feedback data-key="feedback:${esc(id)}"><summary>Feedback</summary>${verdictBar(t.task_id, t.verdict)}</details>` : ""}
    ${cost ? `<p class="foot-note num" data-cost>${cost}</p>` : ""}
    ${t.model?.name || Number.isFinite(t.usage?.model_calls) || Number.isFinite(t.usage?.actions) ? `<details data-key="usage:${esc(id)}"><summary>Execution details</summary><p class="small muted">${[t.model?.name,Number.isFinite(t.usage?.model_calls) ? `${t.usage.model_calls} model calls` : null,Number.isFinite(t.usage?.actions) ? `${t.usage.actions} actions` : null].filter(Boolean).map(esc).join(" · ")}</p></details>` : ""}`;
  const contract = `<h3>Request</h3><p class="request">${esc(t.text)}</p>
    ${t.files?.length ? `<p class="small"><span class="muted">Your files:</span> ${t.files.map(fileChip).join(" ")}</p>` : ""}
    <h3>Plan</h3>${plan(t)}`;
  const layers = { outcome, contract, trace: tl ? trace(tl) : "" };
  const [, pill] = standing({ ...t, waiting_item: items.find(i => i.can_respond)?.id });
  const head = `<h2 class="ptitle" data-task="${esc(t.task_id)}">${esc(t.title || t.text)}</h2>
    <div class="row small dh-sub">${pill}${t.waiting_on ? `<span class="muted">${esc(WAITING[t.waiting_on] || "")}</span>` : running && t.status_line ? statusLineSlot(t, "muted") : ""}</div>`;
  const body = `${items.map(i => card(i, composed)).join("")}
    ${(t.app_offers || []).filter(o => o.state !== "connected").map(appOfferCard).join("")}
    ${running ? `<div class="live" id="plive" aria-live="polite">${esc(Panel.liveText) || `<span class="faint">Working…</span>`}</div>` : ""}
    <div class="seg layers" role="tablist" aria-label="What to show">${[["outcome", "Result"], ["contract", "Progress"], ["trace", "Steps"]].map(([k, l]) => `<button role="tab" aria-selected="${Panel.layer === k}" class="${Panel.layer === k ? "on" : ""}" data-layer="${k}">${l}</button>`).join("")}</div>
    <section class="layer">${layers[Panel.layer]}</section>
    <div class="row pactions">${moveControl(t.task_id, t.project, App.projects)}${!ENDED.includes(t.status) ? `<button data-act="cancel" data-id="${esc(id)}">Stop</button>` : trashButton(t.task_id, "Delete task")}${t.status === "suspended" && !t.waiting_item ? `<button data-act="resume" data-id="${esc(id)}">Resume</button>` : ""}</div>`;
  Panel.task = t;
  return { head, body, after: el => Panel.layer === "outcome" && view.length && fill($("#pview", el), view, id, t) };
}

// A delivered file as a row: its type, its name (opens it), where it is
// kept, and Download.
const FILE_TYPES = { "application/pdf": "PDF", "text/csv": "CSV", "text/markdown": "MD", "text/plain": "TXT", "text/html": "HTML", "application/json": "JSON" };
function fileRow(f, project) {
  const type = FILE_TYPES[f.media_type] || (f.media_type || "").split("/").pop().slice(0, 4).toUpperCase() || "FILE";
  const p = project && (typeof project === "object" ? project : App.projects.find(x => x.id === project));
  return `<div class="frow" data-file-row="${esc(f.id)}"><span class="ftype ${type === "PDF" ? "pdf" : ""}">${esc(type)}</span>
    <div class="rg-main"><a class="ttl" href="#/file/${enc(f.id)}" data-open="file:${esc(f.id)}" title="${esc(f.name)}">${esc(f.name)}</a><span class="sub">Saved to Files${p ? ` · ${esc(p.name)}` : ""} · ${esc(kb(f.size))}</span></div>
    <button class="icon-btn" data-file-download="${esc(f.id)}" data-name="${esc(f.name)}" title="Download ${esc(f.name)}">${icon("download")}<span class="sr">Download</span></button></div>`;
}

// One file, drawn the way an outcome draws it, with where it came from and
// a way to save it.
async function fileView(id) {
  const f = await api("GET", `/v1/files/${enc(id)}/info`);
  const path = `/v1/files/${enc(id)}`;
  const body = `<h2 class="ptitle">${esc(f.name)}</h2>
    <p class="small muted">${f.source === "delivered" ? "Delivered by Magnus" : "Uploaded by you"} · ${fileFrom(f)} · ${esc(kb(f.size))} · ${esc(when(f.at))}</p>
    <div class="row"><button class="primary" data-file-download="${esc(f.id)}" data-name="${esc(f.name)}" aria-label="Download ${esc(f.name)}">Download</button> ${moveControl(f.id, f.project, App.projects)} ${trashButton(f.id, "Delete file")}</div>
    <iframe class="composed" sandbox="allow-same-origin" title="${esc(f.name)}" id="fview"></iframe>`;
  return {
    body,
    after: async el => {
      const files = {};
      try {
        if (GLIMPSE_IMAGE.test(f.media_type)) files[f.name] = { image: await Magnus.dataUrl(path, f.media_type) };
        else if (Compose.TEXT.test(f.media_type)) files[f.name] = { text: decodeText(await Magnus.dataUrl(path, f.media_type)), media_type: f.media_type };
      } catch (e) { toast(e.message); }
      const frame = $("#fview", el);
      frame.removeAttribute("data-sized");
  frame.srcdoc = Compose.documentFor([{ type: "file", file: f.name }], files, fontCss(), f.preview, ...look());
      frame.onload = () => { try { frame.style.height = frame.contentDocument.documentElement.scrollHeight + 4 + "px"; } catch { /* sized by CSS */ } };
    },
  };
}

// ---------- The Library: the connector catalog ----------

// The Library is a catalog of apps, laid out as the owner's catalog mockup
// draws it in every look (connectors spec rev 48, Library): Discover shows
// the whole catalog, Yours what this person has connected or needs to fix;
// one search box and category chips; cards in sections, each with its tile,
// the review mark, one plain sentence, who makes it, and one action at its
// corner. Connecting is a click and the person's own login on the
// provider's page: nothing is typed in Magnus but the search.
const Lib = { tab: "discover", cat: "all", q: "" };
const CATEGORIES = [["all", "All"], ["email", "Email & chat"], ["files", "Files"], ["code", "Code"], ["work", "Work"], ["design", "Design & CAD"]];
const Connecting = {};
const SVG = {
  plus: `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>`,
  check: `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>`,
  again: `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 12a8 8 0 1 1-2.4-5.7"/><path d="M20 4v5h-5"/></svg>`,
  lock: `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/></svg>`,
  search: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/></svg>`,
};
const reviewed = `<svg class="lc-rev" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" role="img" aria-label="Reviewed by Alpha Vector"><path d="M12 2.5l2.3 1.7 2.8-.2 1 2.6 2.4 1.5-.7 2.8 1 2.6-2.3 1.7-.5 2.8-2.8.4-1.9 2.1-2.6-1.1-2.6 1.1-1.9-2.1-2.8-.4-.5-2.8L2.6 15.2l1-2.6-.7-2.8 2.4-1.5 1-2.6 2.8.2z"/><path d="M8.5 12.2l2.3 2.3 4.7-4.8"/></svg>`;
const stateOf = c => (Connecting[c.name] ? "connecting" : c.state);
const tintOf = c => (/^#[0-9a-f]{6}$/i.test(c.listing?.tint || "") ? c.listing.tint : "#3A3F48");
const tile = (c, big) => `<span class="ctile${big ? " big" : ""}" aria-hidden="true" style="background:${tintOf(c)}">${esc(c.listing?.letters || (c.title || "?").slice(0, 2))}</span>`;
const byline = c => (c.listing?.publisher ? `by ${c.listing.publisher}` : "Added by your organisation");
const provider = c => c.listing?.publisher || c.title;

function connectorAction(c) {
  const name = esc(c.title);
  switch (stateOf(c)) {
    case "connecting": return `<span class="lc-act busy" role="status" aria-label="Signing in to ${name}"><span class="spin"></span></span>`;
    case "connected": return c.more_access
      ? `<button class="lc-act warn" data-act="connect" data-app="${esc(c.name)}" aria-label="Approve more access for ${name}">${SVG.again}</button>`
      : `<a class="lc-act ok" href="#/library/${enc(c.name)}" aria-label="${name} is connected. Open details">${SVG.check}</a>`;
    case "needs_attention": return `<button class="lc-act warn" data-act="connect" data-app="${esc(c.name)}" aria-label="Sign in to ${name} again">${SVG.again}</button>`;
    case "local": return `<button class="primary lc-enable" data-act="connect" data-app="${esc(c.name)}">Enable</button>`;
    default: return c.ready === false
      ? `<button class="lc-act off" aria-disabled="true" data-act="connect" data-app="${esc(c.name)}" title="Not ready on this server yet" aria-label="${name} is not ready on this server yet">${SVG.plus}</button>`
      : `<button class="lc-act" data-act="connect" data-app="${esc(c.name)}" aria-label="Connect ${name}">${SVG.plus}</button>`;
  }
}

function connectorCard(c) {
  const st = stateOf(c);
  const line = {
    connecting: `<p class="lc-note">Finish signing in on ${esc(provider(c))}'s page… <button class="link" data-act="connect-page" data-url="${esc(Connecting[c.name] || "")}">Open it again</button></p>`,
    needs_attention: `<p class="lc-note warn">Sign in again to keep using ${esc(c.title)}.</p>`,
    connected: c.account ? `<p class="lc-note ok">Connected as ${esc(c.account)}</p>` : "",
  }[st] || "";
  return `<article class="lcard" data-connector="${esc(c.name)}" data-state="${esc(st)}">${tile(c)}
    <div class="lc-main"><div class="lc-name"><h3><a href="#/library/${enc(c.name)}">${esc(c.title)}</a></h3>${c.listing?.reviewed ? reviewed : ""}</div>
    <p class="lc-sum">${esc(c.summary)}</p>${line}<p class="lc-by">${esc(byline(c))}</p></div>${connectorAction(c)}</article>`;
}

// What a search or a chip leaves: a service's name, or what the person
// wants done, against its name, sentence, publisher and abilities.
function libMatches(c) {
  if (Lib.cat !== "all" && (c.listing?.category || "work") !== Lib.cat) return false;
  const words = Lib.q.toLowerCase().split(/\s+/).filter(Boolean);
  const hay = [c.title, c.summary, c.listing?.publisher, ...(c.listing?.abilities || [])].join(" ").toLowerCase();
  return words.every(w => hay.includes(w));
}

function librarySections(all) {
  const shown = all.filter(libMatches);
  const of = (...states) => shown.filter(c => states.includes(stateOf(c)));
  const sections = Lib.tab === "discover"
    ? [["local", "On this computer", "Programs Magnus found here. Enable one and Magnus can work in it.", of("local")],
       ["attention", "Needs attention", "Sign in again and Magnus picks up where it left off.", of("needs_attention")],
       ["popular", "Popular", "Connect any of these by signing in to your own account.", of("available", "connected", "connecting")]]
    : [["connected", "Connected", "What Magnus can work in for you right now.", of("connected", "connecting")],
       ["attention", "Needs attention", "Sign in again and Magnus picks up where it left off.", of("needs_attention")],
       ["local", "Ready to enable", "Programs on this computer you have not turned on yet.", of("local")]];
  const html = sections.filter(x => x[3].length).map(([k, title, sub, items]) => `<section class="lsect" data-section="${k}">
    <div class="lsect-head"><div><h2>${title}</h2><p>${sub}</p></div><span class="lsect-n">${items.length === 1 ? "1 connector" : `${items.length} connectors`}</span></div>
    <div class="lgrid">${items.map(connectorCard).join("")}</div></section>`).join("");
  if (html) return html;
  return `<div class="lempty">${Lib.q || Lib.cat !== "all" ? "No connectors match. Try another word, or ask Magnus what you want done."
    : Lib.tab === "yours" ? "Nothing connected yet. Discover shows every app Magnus can work in." : "Your organisation's catalog is empty."}</div>`;
}

async function showLibrary(main, name) {
  Lib.main = main;
  if (name) return showConnectorPage(main, name);
  const connectors = await Resources.read("Apps", "/v1/connectors", "connectors",true);
  // An app blocked for your groups is not in your Library; the owner still
  // manages it in Settings.
  Lib.all = (connectors || []).filter(c => c.for_you !== false);
  const seg = (k, label) => `<button data-lib-tab="${k}" aria-pressed="${Lib.tab === k}">${label}</button>`;
  main.innerHTML = `<div class="libpage"><header class="lhead"><div><h1>Apps</h1><p>Connect the apps and programs you want Magnus to work in. You only ever sign in.</p></div>
    <a class="icon-btn lclose" href="#/home" aria-label="Close Apps">${icon("close")}</a></header>
    <div class="lbar"><div class="lseg" role="group" aria-label="Show">${seg("yours", "Yours")}${seg("discover", "Discover")}</div>
    <div class="lsearch">${SVG.search}<label class="sr" for="libq">Search apps and capabilities</label><input id="libq" type="search" value="${esc(Lib.q)}" placeholder="Search apps and capabilities" autocomplete="off"></div></div>
    <div class="lchips" role="group" aria-label="Category">${CATEGORIES.map(([k, l]) => `<button data-lib-cat="${k}" aria-pressed="${Lib.cat === k}">${esc(l)}</button>`).join("")}</div>
    <p><a href="#/chat/new" data-ask-app>Ask Magnus to find an app</a></p><div id="lib-sections">${connectors ? librarySections(Lib.all) : '<p class="empty">Apps are unavailable. Retrying when connected.</p>'}</div></div>`;
  bindConnectorActs(main);
  const update=async()=>{
    const connectors=await Resources.read("Apps","/v1/connectors","connectors",true);
    if(!connectors || !main.querySelector('#lib-sections'))return;
    Lib.all=connectors.filter(c=>c.for_you!==false);
    paint($('#lib-sections',main),librarySections(Lib.all));
  };
  App.redraw=update;onLeave(()=>{if(App.redraw===update)App.redraw=null;});
  $("[data-ask-app]",main).onclick=()=>{const query=$("#libq",main).value.trim();App.appSearch=query ? `Find an app for: ${query}` : "Help me find an app for ";};
  let t;
  $("#libq", main).addEventListener("input", e => { clearTimeout(t); t = setTimeout(() => { Lib.q = e.target.value.trim(); $("#lib-sections", main).innerHTML = librarySections(Lib.all); }, 150); });
}

// One connector's page, in the two states the mockup draws: before
// connecting, what Magnus can do with it and what it asks for; connected,
// as whom, what it can and cannot do, when, what Magnus did, Disconnect.
function connectorBody(c) {
  const st = stateOf(c), L = c.listing || {};
  const head = `<div class="cp-head">${tile(c, true)}<div><div class="cp-name"><h2 class="ptitle">${esc(c.title)}</h2>${st === "connected" ? `<span class="chip ok">${SVG.check}Connected</span>` : ""}</div>
    <p class="muted">${esc(byline(c))}${L.reviewed ? " · Reviewed by Alpha Vector" : ""}</p></div></div>`;
  const recent = `<h3>Recent actions</h3>${c.recent?.length ? `<ol class="cp-recent">${c.recent.map(u => `<li><a href="#/task/${enc(u.task)}" data-open="task:${esc(u.task)}">${esc(u.action.charAt(0).toUpperCase() + u.action.slice(1))}</a><span class="small faint">${esc(when(u.at))}${u.worked === false ? " · did not work" : ""}</span></li>`).join("")}</ol>` : `<p class="empty">Magnus has not used it for you yet.</p>`}`;
  if (st === "connected" || (st === "needs_attention" && c.connected_at)) {
    return `${head}${st === "needs_attention" ? `<p class="lc-note warn">Your sign-in to ${esc(c.title)} stopped working. Sign in again to keep using it.</p>` : ""}
      <dl class="kv cp-kv"><dt>Account</dt><dd>${esc(c.account || "Your own account")}</dd><dt>It can</dt><dd>${esc(L.can || c.summary)}</dd>${L.cannot ? `<dt>It cannot</dt><dd>${esc(L.cannot)}</dd>` : ""}
      ${c.connected_at ? `<dt>Connected</dt><dd>${esc(when(c.connected_at))}</dd>` : ""}<dt>Last used</dt><dd>${c.last_used ? esc(when(c.last_used)) : "Not yet"}</dd></dl>
      ${!c.watched ? "" : c.instant === "automatic" ? `<h3>Instant updates</h3><p class="small muted">Once you connect ${esc(c.title)}, Magnus signs up for its notifications for you and hears about new items as they arrive. It also checks on a schedule.</p>`
        : c.instant === "needs_public_url" ? `<h3>Instant updates</h3><p class="small muted">Magnus checks ${esc(c.title)} for new items on a schedule. Instant updates need a public address for this server, which your admin sets.</p>`
        : `<h3>Instant updates</h3><p class="small muted">Magnus checks ${esc(c.title)} for new items on a schedule. If ${esc(c.title)} can send notifications to an address, give it your push address and Magnus hears about new items as they arrive.</p><div class="row"><button data-act="push-copy" data-app="${esc(c.name)}">Copy push address</button></div>`}
      ${recent}<div class="row cp-actions">${st === "needs_attention" ? `<button class="primary" data-act="connect" data-app="${esc(c.name)}">Sign in to ${esc(c.title)} again</button>` : ""}${c.signs_in ? `<button data-act="disconnect" data-app="${esc(c.name)}">Disconnect ${esc(c.title)}</button>` : ""}${c.generated ? `<button data-act="gen-download" data-app="${esc(c.name)}">Download definition</button><button data-act="gen-delete" data-app="${esc(c.name)}">Delete</button>` : ""}</div>${c.needs_token ? `<div class="row"><input type="password" autocomplete="off" aria-label="Token for ${esc(c.title)}" data-gen-token><button data-act="gen-token" data-app="${esc(c.name)}">Save token</button></div>` : ""}`;
  }
  const can = L.abilities?.length ? L.abilities : c.allows || [];
  const go = st === "connecting"
    ? `<p class="lc-note">Finish signing in on ${esc(provider(c))}'s page, then come back here. <button class="link" data-act="connect-page" data-url="${esc(Connecting[c.name] || "")}">Open it again</button></p>`
    : c.ready === false ? `<p class="small muted">${esc(c.title)} is not ready on this server yet.</p>`
    : `<button class="primary cp-connect" data-act="connect" data-app="${esc(c.name)}">${st === "needs_attention" ? `Sign in to ${esc(c.title)} again` : `Connect ${esc(c.title)}`}</button>`;
  return `${head}<h3>What Magnus can do with ${esc(c.title)}</h3>${can.length ? `<ul class="cp-list">${can.map(l => `<li>${esc(l)}</li>`).join("")}</ul>` : `<p class="empty">Connecting shows what Magnus can read, change and send in it.</p>`}
    ${L.asks?.length ? `<h3>What it asks for</h3><ul class="cp-list">${L.asks.map(l => `<li>${esc(l)}</li>`).join("")}</ul>` : ""}
    <p class="cp-lock">${SVG.lock}<span>You sign in on ${esc(provider(c))}'s own page. Magnus never sees your password, and you can disconnect at any time.</span></p>${go}`;
}

async function showConnectorPage(main, name) {
  const c = await api("GET", `/v1/connectors/${enc(name)}`);
  main.innerHTML = `<div class="libpage cpage"><p class="small"><a href="#/library">Apps</a></p><section class="cp" aria-label="${esc(c.title)}">${connectorBody(c)}</section></div>`;
  bindConnectorActs(main);
}

// The same connector in the panel, from a card in chat or a task.
async function connectorView(name) {
  return plain(`<section class="cp">${connectorBody(await api("GET", `/v1/connectors/${enc(name)}`))}</section>`);
}

// What Disconnect did: ended at the app, or, where the app could not end it
// itself, ended here, with the app's own page where the person removes
// Magnus (connectors spec, Lifecycle).
function disconnected(r) {
  if (!r || r.at_provider !== false) { toast("Disconnected. Your sign-in is ended at the app too."); return; }
  toast(r.remove_at ? `Disconnected here. The app could not end your sign-in itself: remove Magnus at ${r.remove_at}` : "Disconnected here. The app could not end your sign-in itself: remove Magnus in its own settings.", 10000);
}

// Connect, again or for the first time, and Disconnect, wherever a card is.
function bindConnectorActs(root) {
  if (root.dataset.connActs) return;
  root.dataset.connActs = "1";
  root.addEventListener("click", async e => {
    const tab = e.target.closest("[data-lib-tab]"), cat = e.target.closest("[data-lib-cat]");
    if (tab || cat) {
      if (tab) Lib.tab = tab.dataset.libTab; else Lib.cat = cat.dataset.libCat;
      return showLibrary(root);
    }
    const a = e.target.closest("[data-act]");
    if (!a || !root.contains(a)) return;
    if (a.dataset.act === "connect") { e.preventDefault(); connectApp(a.dataset.app).catch(err => toast(err.message)); }
    if (a.dataset.act === "connect-page") { e.preventDefault(); Magnus.openExternal(a.dataset.url).catch(() => {}); }
    // The address is this server's, as the app reaches it; it is a credential, so it is copied, not shown.
    if (a.dataset.act === "push-copy") { e.preventDefault(); try { const { url } = await api("GET", `/v1/connectors/${enc(a.dataset.app)}/push`); if (!url) { toast("Instant updates need a public address for this server, which your admin sets."); return; } await copyText(url); toast("Push address copied. Keep it private: anyone with it can make Magnus check this app."); } catch (err) { toast(err.message); } }
    if (a.dataset.act === "gen-download") { e.preventDefault(); try { toast(`Saved ${await Magnus.download(`/v1/connectors/${enc(a.dataset.app)}/definition`, `${a.dataset.app}.json`)}`); } catch (err) { toast(err.message); } }
    if (a.dataset.act === "gen-token") { e.preventDefault(); const input = $("[data-gen-token]", root); try { await api("PUT", `/v1/connectors/${enc(a.dataset.app)}/token`, { token: input.value }); input.value = ""; toast("Saved."); connectorsChanged(); } catch (err) { toast(err.message); } }
    if (a.dataset.act === "gen-delete") {
      e.preventDefault();
      if (!await ask("Delete this connector? Its token and grants are revoked and its approval rules are removed.", "Delete")) return;
      try { await api("DELETE", `/v1/connectors/${enc(a.dataset.app)}/generated`); location.hash = "#/library"; connectorsChanged(); } catch (err) { toast(err.message); }
    }
    if (a.dataset.act === "disconnect") {
      e.preventDefault();
      if (!await ask("Disconnect? Your sign-in is ended at the app, and Magnus can no longer use it for you until you connect again.", "Disconnect")) return;
      try { disconnected(await api("DELETE", `/v1/connectors/${enc(a.dataset.app)}/connection`)); connectorsChanged(); } catch (err) { toast(err.message); }
    }
  });
}

// Wherever connectors are shown, they are drawn again.
function connectorsChanged() {
  const m = /^#\/library(?:\/([^/]+))?$/.exec(location.hash);
  if (m && Lib.main?.isConnected) showLibrary(Lib.main, m[1]);
  drawPanel();
}

// Connect: the system browser opens at the app's own sign-in page; the
// answer comes back to this computer, and the card turns Connected.
function onThisComputer(u) {
  try {
    const x = new URL(u);
    return x.protocol === "http:" && x.hostname === "127.0.0.1" && x.pathname === "/callback" && x.port;
  } catch { return false; }
}

async function connectApp(name) {
  // Slack's registered address is https. The desktop listens on the handoff,
  // and the sign-in tells Slack the https address, which the exchange repeats.
  let card = (Lib.all || []).find(c => c.name === name);
  if (!card) { try { card = await api("GET", `/v1/connectors/${enc(name)}`); } catch { card = null; } }
  // A click on a connector this server cannot sign anyone in to yet used to
  // do nothing at all (2026-11-09): say why instead.
  if (card && card.ready === false) {
    toast(`${card.title || name} can't be connected on this server yet: Alpha Vector has not set up its sign-in here. Ask your organisation's owner.`);
    return;
  }
  // The one screen connecting shows; accepting it is all the approval Magnus
  // needs to use the app (spec rev 140). Signing in again needs no new one.
  const again = card && card.state === "needs_attention", signsIn = !card || card.signs_in !== false;
  if (signsIn && !Magnus.loopbackOpen) { toast("Connect apps from the Magnus desktop app."); return; }
  if (!again && !await consentToConnect(name)) return;
  if (!signsIn) { toast("Connected."); connectorsChanged(); return; }
  const listenAt = card && (card.handoff || (onThisComputer(card.redirect_uri) ? card.redirect_uri : null));
  const caught = await Magnus.loopbackOpen(listenAt || null);
  const redirect = card && card.redirect_uri && !onThisComputer(card.redirect_uri) ? card.redirect_uri : caught;
  const r = await api("POST", `/v1/connectors/${enc(name)}/sign-in`, { redirect_uri: redirect });
  Connecting[name] = r.authorize_url;
  connectorsChanged();
  Magnus.openExternal(r.authorize_url).catch(() => {});
  try {
    const got = await Magnus.loopbackWait();
    if (got.state !== r.state) throw new Error("That sign-in came back for another request. Try again.");
    await api("POST", `/v1/connectors/${enc(name)}/sign-in/complete`, { state: got.state, code: got.code, iss: got.iss });
    toast("Connected.");
  } catch (e) { toast(e.message); }
  finally { delete Connecting[name]; connectorsChanged(); }
}

// Files: every file uploaded or delivered, as a view over the same files
// the conversations show.
async function filesView() {
  const { q, project } = Panel.lib;
  const qs = new URLSearchParams();
  if (q) qs.set("q", q);
  if (project) qs.set("project", project);
  const { files } = await api("GET", `/v1/files${qs.size ? `?${qs}` : ""}`);
  return plain(`<div class="row lib-bar"><label class="sr" for="lq">Search files</label><input id="lq" type="search" data-draft="lib-q" placeholder="Files, tasks, projects or text" autocomplete="off">
    <label class="sr" for="lp">Project</label><select id="lp" aria-label="Filter by project"><option value="">All files</option><option value="none"${project === "none" ? " selected" : ""}>In no project</option>${App.projects.map(p => `<option value="${esc(p.id)}"${p.id === project ? " selected" : ""}>${esc(p.name)}</option>`).join("")}</select>
    ${attachButton("Upload")}</div>
    ${files.length ? `<ul class="flist">${files.map(f => `<li data-lib-file="${esc(f.id)}"><div class="rg-main">${fileChip(f)}<span class="small muted">${f.source === "delivered" ? "Delivered by Magnus" : "Uploaded by you"} · ${esc(when(f.at))} · ${fileFrom(f)}</span></div>${moveControl(f.id, f.project, App.projects, true)}</li>`).join("")}</ul>`
      : `<p class="empty">${q || project ? "No file matches." : "No files yet. Upload one, or ask Magnus for a document."}</p>`}`);
}

function projectsView() {
  return plain(`${App.projects.length ? `<ul class="plist">${App.projects.map(p => `<li><a href="#/project/${enc(p.id)}" data-open="project:${esc(p.id)}"><strong>${esc(p.name)}</strong><span class="small muted">${p.counts.tasks} tasks · ${p.counts.files} files · ${p.counts.schedules} schedules</span></a></li>`).join("")}</ul>` : `<p class="empty">No projects yet.</p>`}
    <form id="np" class="card flat"><h3>New project</h3><p class="small muted">A project groups conversations, tasks, schedules and files. Its instructions and files apply to everything in it.</p><label>Name <input id="pn" required></label>
    <label>Instructions for everything in it <span class="faint small">(optional)</span><textarea id="pi" rows="3" data-draft="new-project" placeholder="For example: Prices in euros. Keep every document to one page."></textarea></label>
    <div class="row"><button class="primary">Create</button></div></form>`);
}

async function projectView(id) {
  const p = await api("GET", `/v1/projects/${enc(id)}`);
  return plain(`<div class="spread"><h2 class="ptitle">${esc(p.name)}</h2>${trashButton(id, "Delete project")}</div>
    <div class="row"><a class="button primary" href="#/chat/new/${enc(id)}">New conversation in this project</a></div>
    <h3>Instructions</h3><p class="small muted">Applied to every task and conversation in this project. Only you change them; Magnus reads them.</p>
      <textarea id="pin" rows="4" data-draft="pin-${esc(id)}">${esc(p.instructions || "")}</textarea><div class="row"><button data-act="save-ins" data-id="${esc(id)}">Save instructions</button></div>
    <div class="spread"><h3>Files</h3>${attachButton("Upload to this project")}</div>
      ${p.files.length ? `<ul class="flist">${p.files.map(f => `<li data-project-file="${esc(f.id)}">${fileChip(f)} <span class="small faint">${f.source === "delivered" ? "delivered" : "uploaded"} ${esc(when(f.at))}</span></li>`).join("")}</ul>` : `<p class="empty">No files. Tasks here read the project's files when they need them.</p>`}
    <h3>Conversations</h3>${p.threads.length ? `<ul class="tlist">${p.threads.map(t => `<li class="trow"><a href="#/chat/${enc(t.thread)}"><span class="ttl">${esc(t.title)}</span><span class="small faint when">${esc(when(t.updated_at))}</span></a></li>`).join("")}</ul>` : `<p class="empty">No conversations.</p>`}
    <h3>Tasks</h3>${p.tasks.length ? `<ul class="tlist">${p.tasks.map(taskRow).join("")}</ul>` : `<p class="empty">No tasks.</p>`}
    <h3>Schedules</h3>${p.schedules.length ? `<ul class="tlist">${p.schedules.map(x => `<li class="trow"><a href="#/schedules" data-open="schedule:${esc(x.id)}"><span class="ttl">${esc(x.title)}</span><span class="small faint when">Next ${esc(until(x.next))}</span></a></li>`).join("")}</ul>` : `<p class="empty">No schedules.</p>`}`);
}

// One search across conversations, tasks and files.
async function searchView(q) {
  const r = await api("GET", `/v1/search?q=${enc(q)}`);
  const none = !r.conversations.length && !r.tasks.length && !r.files.length;
  return plain(none ? `<p class="empty">Nothing matches “${esc(q)}”.</p>` : `
    ${r.conversations.length ? `<h3>Conversations</h3><ul class="tlist">${r.conversations.map(c => `<li class="trow found" data-found-thread="${esc(c.thread)}"><a href="#/chat/${enc(c.thread)}"><span class="ttl">${esc(c.title)}</span><span class="small faint when">${esc(when(c.updated_at))}</span></a>${c.snippet ? `<div class="small muted snippet">${esc(c.snippet)}</div>` : ""}</li>`).join("")}</ul>` : ""}
    ${r.tasks.length ? `<h3>Tasks</h3><ul class="tlist">${r.tasks.map(taskRow).join("")}</ul>` : ""}
    ${r.files.length ? `<h3>Files</h3><ul class="flist">${r.files.map(f => `<li data-lib-file="${esc(f.id)}">${fileChip(f)}<span class="small">${fileFrom(f)}</span></li>`).join("")}</ul>` : ""}`);
}

// What can be done in the panel, whichever view it shows.
function wirePanel() {
  const el = $("#detail");
  el.addEventListener("click", async e => {
    if (e.target.closest("[data-act=close-panel]")) return closePanel(true);
    const l = e.target.closest("[data-layer]");
    if (l) { Panel.layer = l.dataset.layer; return drawPanel(); }
    const m = e.target.closest("[data-more]");
    if (m) { MORE.add(m.dataset.more); return drawPanel(); }
    const a = e.target.closest("button[data-act]");
    if (!a) return;
    const id = a.dataset.id;
    try {
      switch (a.dataset.act) {
        case "speak": await readAloud(Panel.task.spoken,`task:${Panel.task.task_id}`); break;
        case "hush": await hush(); break;
        case "cancel":
          if (!await ask("Stop this task? What it already did stays done.", "Stop")) return;
          await api("POST", `/v1/tasks/${enc(id)}/cancel`, {}); break;
        case "resume": await api("POST", `/v1/tasks/${enc(id)}/resume`, {}); break;
        case "stop-schedule":
          if (!await ask("Stop this schedule? No more of its runs start. Active runs stop too. Work already done stays done, and the runs stay in Tasks.", "Stop schedule")) return;
          await api("DELETE", `/v1/schedules/${enc(id)}`); toast("The schedule is stopped."); break;
        case "delete-schedule":
          if (!await ask("Delete this schedule? It has ended, so nothing stops; its runs stay in Tasks.", "Delete schedule")) return;
          await api("DELETE", `/v1/schedules/${enc(id)}`); toast("The schedule is deleted."); break;
        case "resume-schedule": await api("POST", `/v1/schedules/${enc(id)}/resume`, {}); toast("Schedule resumed."); break;
        case "connect": connectApp(a.dataset.app).catch(err => toast(err.message)); return;
        case "connect-page": await Magnus.openExternal(a.dataset.url); return;
        case "disconnect":
          if (!await ask("Disconnect? Your sign-in is ended at the app, and Magnus can no longer use it for you until you connect again.", "Disconnect")) return;
          disconnected(await api("DELETE", `/v1/connectors/${enc(a.dataset.app)}/connection`)); break;
        case "save-ins":
          await api("PUT", `/v1/projects/${enc(id)}`, { instructions: $("#pin", el).value }); delete drafts[`pin-${id}`];
          toast("Instructions saved. New tasks here use them."); break;
        default: return;
      }
      await refresh();
    } catch (err) { toast(err.message); }
  });
  let t;
  el.addEventListener("input", e => {
    if (e.target.id !== "lq") return;
    clearTimeout(t);
    t = setTimeout(() => { Panel.lib.q = e.target.value.trim(); drawPanel(); }, 200);
  });
  el.addEventListener("change", async e => {
    if (e.target.id === "lp") { Panel.lib.project = e.target.value; return drawPanel(); }
    if (!e.target.matches("[data-attach]")) return;
    const project = Panel.what.startsWith("project:") ? Panel.what.slice(8) : Panel.lib.project && Panel.lib.project !== "none" ? Panel.lib.project : null;
    try { const up = await upload(e.target.files, project ? { project } : {}); toast(`Uploaded ${up.map(f => f.name).join(", ")}.`); } catch (err) { toast(err.message); }
    e.target.value = "";
    refresh();
  });
  el.addEventListener("submit", async e => {
    if (e.target.id !== "np") return;
    e.preventDefault();
    try {
      const r = await api("POST", "/v1/projects", { name: $("#pn", el).value, instructions: $("#pi", el).value });
      delete drafts["new-project"];
      await refresh();
      openPanel(`project:${r.id}`);
    } catch (err) { toast(err.message); }
  });
}

// ---------- Trash ----------

const TRASHED = { task: "Task", conversation: "Conversation", message: "Chat message", file: "File", project: "Project", memory: "Memory", knowledge: "Knowledge entry" };
route(/^#\/trash$/, async main => {
  const draw = async () => {
    const { items, keep_days } = await api("GET", "/v1/trash");
    paint(main, `<h1>Trash</h1><p class="muted">What you delete waits here for ${esc(keep_days)} days, then it is gone for good. Restore puts it back where it was.</p>
      ${items.length ? `<div class="row"><button data-act="empty">Empty trash now</button></div>
      <table class="t trash"><thead><tr><th>Deleted</th><th>When</th><th>Gone for good</th><th></th></tr></thead><tbody>${items.map(i => `<tr data-trashed="${esc(i.item)}">
        <td><div class="small faint">${esc(TRASHED[i.kind] || i.kind)}</div><div data-title>${esc(i.title) || `<span class="faint">Untitled</span>`}</div></td>
        <td class="small faint n">${esc(when(i.trashed_at))}</td><td class="small faint n">${esc(new Date(i.removed_at).toLocaleDateString())}</td>
        <td><div class="row"><button data-act="restore">Restore</button><button class="link" data-act="remove">Delete now</button></div></td></tr>`).join("")}</tbody></table>`
      : `<p class="empty">The trash is empty.</p>`}`);
  };
  main.onclick = async e => {
    const b = e.target.closest("button[data-act]"); if (!b) return;
    const item = b.closest("[data-trashed]")?.dataset.trashed;
    try {
      if (b.dataset.act === "restore") { b.disabled = true; await api("POST", `/v1/trash/${enc(item)}/restore`, {}); toast("Restored."); }
      if (b.dataset.act === "remove") {
        if (!await ask("Delete this for good? It cannot be restored.", "Delete for good")) return;
        b.disabled = true; await api("DELETE", `/v1/trash/${enc(item)}`); toast("Deleted for good.");
      }
      if (b.dataset.act === "empty") {
        if (!await ask("Delete everything in the trash for good? None of it can be restored.", "Delete everything")) return;
        b.disabled = true; const r = await api("POST", "/v1/trash/empty", {}); toast(`${r.removed} deleted for good.`);
      }
      await draw();
    } catch (err) { toast(err.message); b.disabled = false; }
  };
  App.redraw = draw;
  onLeave(() => { App.redraw = null; });
  await draw();
});

// ---------- Models ----------

const signins = {};
/** Connecting the server's model providers: keys and sign-ins to the vault,
 * and the conformance suite. Admin › Models draws it (admin-spend.js) with
 * `connectOnly`, below its own default and rules; every change it makes is
 * an admin change, so the server asks for the password again when due.
 * @param {HTMLElement} main @param {{connectOnly?: boolean}} [opts] */
async function modelsSection(main, opts = {}) {
  let pick = null;
  const catalogs = {};
  const catalog = async a => {
    const c=await Resources.read(`Models for ${a}`,`/v1/adapters/${enc(a)}/catalog`,undefined,true);
    if(c)catalogs[a]=c.models ? {ids:c.models.map(m=>m.id)} : {error:c.message || "the adapter sent no list"};
    return catalogs[a] || {error:"The provider is unavailable. Retrying when connected."};
  };
  const draw = async () => {
    const [providers, cur] = await Promise.all([Resources.read("Providers", "/v1/adapters",undefined,true), Resources.read("Model", "/v1/model",undefined,true)]);
    if(!providers || !cur){if(!main._html)paint(main,'<h2>Models</h2><p class="empty">Models are unavailable. Retrying when connected.</p>');return;}
    const {adapters}=providers;
    const p = pick || cur.adapter, models = await catalog(p);
    const chosen = p === cur.adapter ? cur.model : null;
    paint(main, `${opts.connectOnly ? `<h3>Connect providers</h3>` : `<h2>Models</h2>`}<p class="muted">Connect providers here. Keys and sign-ins go to the server's vault; this app keeps none.</p>
      ${opts.connectOnly ? "" : `<div class="card"><div class="small muted">New tasks run on</div><div class="mono" style="font-size:16px;margin:2px 0 10px">${esc(cur.adapter)} · ${esc(cur.model)}</div>
        <div class="row"><select id="madapter" aria-label="Provider" data-draft="model-provider">${adapters.map(a => `<option ${a.id === p ? "selected" : ""}>${esc(a.id)}</option>`).join("")}</select>
        ${models.ids ? `<select id="mname" aria-label="Model" data-draft="model-choice-${esc(p)}">${chosen && !models.ids.includes(chosen) ? `<option selected>${esc(chosen)}</option>` : ""}${models.ids.map(m => `<option ${m === chosen ? "selected" : ""}>${esc(m)}</option>`).join("")}</select><button class="primary" data-act="use">Use this model</button>` : ""}</div>
        ${models.error ? `<p class="small muted" style="margin-top:8px">No model list from ${esc(p)} yet: ${esc(models.error)}</p>` : ""}</div>`}
      ${adapters.map(a => `<div class="card" data-adapter="${esc(a.id)}"><div class="spread"><strong>${esc(a.id)}</strong>
          <span class="row">${a.conformance ? `<span class="chip ${a.conformance.passed ? "ok" : "bad"}">suite ${a.conformance.passed ? "passed" : "failed"} · ${esc(a.conformance.model)}</span>` : `<span class="chip warn">suite not run</span>`}</span></div>
        <div class="row" style="margin-top:10px"><button data-act="signin" data-adapter="${esc(a.id)}">Connect</button><button data-act="suite" data-adapter="${esc(a.id)}">Run the conformance suite</button></div>
        <div id="si-${esc(a.id)}">${signins[a.id] || ""}</div><div id="suite-${esc(a.id)}"></div></div>`).join("")}`);
    const ma = $("#madapter", main);
    if (ma) ma.onchange = () => { pick = ma.value; draw(); };
  };
  const show = (a, html) => { signins[a] = html; const el = $(`#si-${CSS.escape(a)}`, main); if (el) el.innerHTML = html; };
  const finish = async (a, step, input) => {
    try {
      await api("POST", `/v1/adapters/${enc(a)}/sign-in/complete`, { flow: step.flow, input });
      delete catalogs[a];
      show(a, `<p class="small" style="color:var(--ok)">Connected.</p>`);
      draw();
    } catch (e) { show(a, `<p class="small" style="color:var(--bad)">${esc(e.message)}</p>`); }
  };
  main.onclick = async e => {
    const b = e.target.closest("button[data-act]"); if (!b) return;
    const a = b.dataset.adapter;
    try {
      if (b.dataset.act === "use") { await api("PUT", "/v1/model", { adapter: $("#madapter").value, model: $("#mname").value }); toast("New tasks will use this model."); draw(); }
      if (b.dataset.act === "signin") {
        b.disabled = true;
        const step = await api("POST", `/v1/adapters/${enc(a)}/sign-in`, {});
        b.disabled = false;
        if (step.event === "error") { show(a, `<p class="small" style="color:var(--bad)">${esc(step.message)}</p>`); return; }
        const head = `<div style="margin-top:12px"><p class="small">${esc(step.instructions)}</p>`;
        if (step.flow?.kind === "api_key") {
          show(a, `${head}<label for="key-${esc(a)}">API key</label><input type="password" id="key-${esc(a)}" data-ephemeral style="width:100%" autocomplete="off">
            <div class="row" style="margin-top:8px"><button class="primary" data-act="key" data-adapter="${esc(a)}">Save to the vault</button></div></div>`);
          signins[`${a}:step`] = step;
        } else {
          show(a, `${head}${step.user_code ? `<p>Code: <span class="mono" style="font-size:18px">${esc(step.user_code)}</span></p>` : ""}
            ${step.url ? `<div class="row"><button class="primary" data-act="open" data-url="${esc(step.url)}">Open the sign-in page</button></div>` : ""}
            <p class="small muted">Waiting for you to approve on the provider's site…</p></div>`);
          finish(a, step, {});
        }
      }
      if (b.dataset.act === "open") await Magnus.openExternal(b.dataset.url);
      if (b.dataset.act === "key") {
        const v = $(`#key-${CSS.escape(a)}`, main).value.trim();
        if (!v) return toast("Paste the key first.");
        b.disabled = true;
        await finish(a, signins[`${a}:step`], { api_key: v });
      }
      if (b.dataset.act === "suite") {
        const el = $(`#suite-${CSS.escape(a)}`, main);
        b.disabled = true; el.innerHTML = `<p class="small muted">Running the suite against the live provider…</p>`;
        const r = await api("POST", `/v1/adapters/${enc(a)}/conformance`, {});
        el.innerHTML = `<table class="t small" style="margin-top:8px"><tbody>${r.checks.map(c => `<tr><td>${c.passed ? `<span class="chip ok">pass</span>` : `<span class="chip bad">fail</span>`}</td><td>${esc(c.name)}${c.simulated ? " <span class='faint'>(simulated)</span>" : ""}</td><td class="muted">${esc(c.detail)}</td></tr>`).join("")}</tbody></table>`;
        b.disabled = false; main._html = null;
      }
    } catch (err) { toast(err.message); b.disabled = false; }
  };
  App.redraw=draw;onLeave(()=>{if(App.redraw===draw)App.redraw=null;});
  await draw();
}

// The organisation's apps are Admin › Connectors' (admin-connectors.js); an
// added server's card for new tools that change or send is in the inbox.

// ---------- Settings ----------

// Everything a person sets for themselves, in one place; developer views
// stay in their own group. Administration lives in Admin, not here (spec
// rev 141): the server's models, the organisation's connectors, people's
// roles, the server's updates and the kill switch moved there.
const SETTINGS = [
  ["account", "Account", accountSection], ["rules", "Rules", (el) => rulesSection(el)], ["payments", "Payments", (el) => paymentsSection(el)],
  ["appearance", "Appearance", appearanceSection], ["notifications", "Notifications", notificationsSection], ["updates", "Updates", updatesSection], ["data", "Data and privacy", dataSection],
  ["agreements", "Agreements", (el) => agreementsSection(el)],
];
/** Where Settings' administration went: its old addresses open Admin. */
const MOVED_TO_ADMIN = { models: "#/admin/models", connectors: "#/admin/connectors", safety: "#/admin/overview" };
/** Whether the signed-in person administers here (admin.js knows). */
const administers = () => typeof AdminState !== "undefined" && !!AdminState.standing?.admin;
route(/^#\/settings(?:\/([a-z]+))?$/, async (main, which) => {
  if (MOVED_TO_ADMIN[which]) { location.hash = administers() ? MOVED_TO_ADMIN[which] : "#/settings"; return; }
  const sec = SETTINGS.find(x => x[0] === which) || SETTINGS[0];
  main.innerHTML = `<h1>Settings</h1><nav class="tabs" aria-label="Settings">${SETTINGS.map(([k, label]) =>
    `<a href="#/settings/${k}" class="${k === sec[0] ? "on" : ""}"${k === sec[0] ? ` aria-current="page"` : ""}>${esc(label)}</a>`).join("")}</nav>${administers() ? `<p class="small muted" data-admin-pointer>The server's models, the organisation's connectors, people's roles, server updates and the kill switch are in <a href="#/admin">Admin</a>.</p>` : ""}<section id="setting" data-section="${sec[0]}"></section>`;
  await sec[2]($("#setting", main));
});
// Where these lived before Settings.
route(/^#\/models$/, () => { location.hash = administers() ? "#/admin/models" : "#/settings"; });
route(/^#\/apps$/, () => { location.hash = administers() ? "#/admin/connectors" : "#/library"; });

// How Magnus looks on this device (U7, amended): light, dark or the
// computer's; solid or glass; an accent; and how much it moves. The choice
// takes effect at once; the computer's own settings for less motion and less
// transparency still apply on top.
function appearanceSection(el) {
  const radio = (look, value, label, inner = label, cls = "", style = "") => {
    const on = Theme.prefs()[look] === value;
    return `<button class="${cls}${on ? " on" : ""}" role="radio" aria-checked="${on}" data-look="${look}" data-value="${value}" title="${esc(label)}"${style ? ` style="${style}"` : ""}>${inner}</button>`;
  };
  const draw = () => {
    const p = Theme.prefs(), os = Theme.os(), look = Theme.look;
    paint(el, `<h2>Appearance</h2><div class="appearance">
      ${Theme.saved === false ? `<p class="small" role="status">Appearance changed for this session but was not saved on this device.</p>` : ""}
      <h3 id="mode-h">Mode</h3><div class="seg" role="radiogroup" aria-labelledby="mode-h">${Theme.MODES.map(([k, l]) => radio("mode", k, l)).join("")}</div>
      <h3 id="mat-h">Material</h3><div class="materials" role="radiogroup" aria-labelledby="mat-h">${Theme.MATERIALS.map(([k, l, hint]) => radio("material", k, l,
        `<span class="mprev ${k}" aria-hidden="true"><span class="ms"></span><span class="mm"></span></span><span class="mt"><span class="ml">${esc(l)}</span><span class="small muted">${esc(hint)}</span></span>`, "material")).join("")}</div>
      ${p.material === "glass" && look.material !== "glass" ? `<p class="small muted" data-look-note>Your computer asks for less transparency, so Magnus stays solid.</p>` : ""}
      <h3 id="acc-h">Accent color</h3><div class="swatches" role="radiogroup" aria-labelledby="acc-h">${Theme.ACCENTS.map(a => radio("accent", a.id, a.label, `<span class="sr">${esc(a.label)}</span>`, "swatch", `--swatch:${a.swatch};background:${a.swatch}`)).join("")}</div>
      <h3 id="mot-h">Motion</h3><div class="seg" role="radiogroup" aria-labelledby="mot-h">${Theme.MOTIONS.map(([k, l]) => radio("motion", k, l)).join("")}</div>
      <p class="small muted" data-motion-hint>${esc(Theme.MOTION_HINTS[p.motion])}${look.motion !== p.motion ? " Your computer asks for reduced motion, so Magnus uses quick fades." : ""}</p>
      <p class="small faint">Magnus also follows your computer's settings for reduced motion and reduced transparency.</p></div>`);
  };
  // Redraw keeps keyboard focus on the choice that had it.
  const redraw = () => {
    const a = document.activeElement?.dataset;
    draw();
    if (a?.look) $(`[data-look="${a.look}"][data-value="${a.value}"]`, el)?.focus({ preventScroll: true });
  };
  el.onclick = e => {
    const b = e.target.closest("[data-look]");
    if (!b) return;
    b.focus({ preventScroll: true });
    Theme.set({ [b.dataset.look]: b.dataset.value });
  };
  const changed = () => { if (el.isConnected) redraw(); };
  window.addEventListener("magnus-theme", changed);
  onLeave(() => window.removeEventListener("magnus-theme", changed));
  draw();
}

// The owner's kill switch, and what it revoked.
async function accountSection(el) {
  const draw = async () => {
    const { sessions } = await api("GET", "/v1/sessions");
    paint(el, `<h2>Account</h2>
      <div class="card"><div class="spread"><div><strong>${esc(App.me.name)}</strong> <span class="faint">(${esc(App.me.id)})</span>
        <div class="small muted">Signed in to ${esc(App.appliance)}${App.me.roles?.length ? ` · roles: ${esc(App.me.roles.join(", "))}` : ""}${App.me.groups?.length ? ` · groups: ${esc(App.me.groups.join(", "))}` : ""}</div></div>
        <button data-act="sign-out">Sign out</button></div></div>
      <div data-password></div><div data-mail></div>
      <h3>Devices</h3>
      ${sessions.length ? `<table class="t" data-devices><tbody>${sessions.map(x => `<tr data-session="${esc(x.id)}"><td>${x.client === "app" ? "Magnus app" : x.client === "browser" ? "Browser" : "Device"}${x.current ? ` <span class="chip ok">this device</span>` : ""}</td>
        <td class="small muted">signed in ${esc(when(x.created))} · until ${esc(new Date(x.expires).toLocaleDateString())}</td>
        <td>${x.current ? "" : `<button class="link" data-act="end" data-id="${esc(x.id)}">Sign out</button>`}</td></tr>`).join("")}</tbody></table>`
        : `<p class="empty">Signed in with an access key; no devices to list.</p>`}
      ${administers() ? `<p class="small muted" data-roles-moved>People's roles are set in <a href="#/admin/people">Admin › People</a>.</p>` : ""}`);
    const pw = $("[data-password]", el);
    if (pw && !pw.childElementCount) { await passwordCard(pw); await mailCard($("[data-mail]", el)); }
  };
  el.onclick = async e => {
    const b = e.target.closest("button[data-act]"); if (!b) return;
    try {
      if (b.dataset.act === "sign-out") return $("nav.side [data-act=sign-out]")?.click();
      if (b.dataset.act === "end") { b.disabled = true; await api("DELETE", `/v1/sessions/${enc(b.dataset.id)}`); toast("That device is signed out."); }
      el._html = null; await draw();
    } catch (err) { toast(err.message); b.disabled = false; }
  };
  await draw();
}

// Notifications follow the person to every device: the choices live on the appliance.
const NOTIFY = [["notify_blocking", "A task needs me now", "An approval, a question, or a review a task is waiting on."],
  ["notify_batched", "Something can wait for me", "Results to look at and proposals: decisions no task is waiting on."],
  ["notify_agent_changes", "The agent changes", "A new model, instructions, harness or policy."]];
/** This device's own choice: whether what pops up in the app also goes to
 * the system's notifications while Magnus is in the background. */
function systemNotificationsCard() {
  const allowed = Magnus.notifyPermission?.() || "unsupported";
  const why = allowed === "denied" ? "Your browser blocks notifications from this site. Allow them in its site settings, then turn this on."
    : allowed === "unsupported" ? "This browser cannot show notifications." : "Who wrote and where, never what a message says. Messages, mentions and what needs you also pop up in the app's lower corner.";
  return `<h3>On this device</h3><div class="card"><label class="row toggle"><input type="checkbox" data-system-notifications ${Notices.systemOn() ? "checked" : ""}${allowed === "unsupported" ? " disabled" : ""}>
    <span><span>Show them in ${Magnus.kind === "web" ? "this computer's" : "the system's"} notifications while Magnus is in the background</span><span class="small muted" style="display:block">${esc(why)}</span></span></label></div>`;
}

async function notificationsSection(el) {
  const draw = async () => {
    App.prefs = await api("GET", "/v1/preferences");
    paint(el, `<h2>Notifications</h2><p class="muted">Magnus tells this computer when…</p>
      <div class="card">${NOTIFY.map(([k, label, hint]) => `<label class="row toggle"><input type="checkbox" data-pref="${k}" ${App.prefs[k] ? "checked" : ""}>
        <span><span>${esc(label)}</span><span class="small muted" style="display:block">${esc(hint)}</span></span></label>`).join("")}</div>
      ${systemNotificationsCard()}`);
  };
  el.onchange = async e => {
    if (e.target.closest("input[data-system-notifications]")) {
      if (e.target.checked && Magnus.notifyPermission?.() !== "granted") await Notices.turnOn(); else Notices.setSystem(e.target.checked);
      return draw();
    }
    const c = e.target.closest("input[data-pref]"); if (!c) return;
    try { App.prefs = await api("PUT", "/v1/preferences", { [c.dataset.pref]: c.checked }); toast("Saved."); } catch (err) { toast(err.message); c.checked = !c.checked; }
  };
  await draw();
}

async function updatesSection(el) {
  const draw = async () => {
    // The server's own updates (install, roll back, channel) are Admin ›
    // Operations'; here, only what it runs and what changed.
    const [st, notes, ver] = await Promise.all([Magnus.updateState().catch(() => null), Magnus.whatsNew(true).catch(() => null), api("GET", "/v1/version")]);
    const para = t => esc(t || "Fixes and improvements.").split(/\n+/).map(l => `<p>${l}</p>`).join("");
    paint(el, `<h2>Updates</h2>
      ${st ? `<div class="card" data-app-version><div class="spread"><div><strong>This app</strong> <span class="mono">${esc(st.current)}</span></div>
          ${st.ready ? `<button class="primary" data-act="restart-update">Restart to update</button>` : `<button data-act="look">Check now</button>`}</div>
        <div class="small muted">${st.ready ? `Magnus ${esc(st.ready.version)} is downloaded and checked.` : st.error ? `The last look failed: ${esc(st.error)}` : st.can_update ? esc(st.can_update) : "Up to date."}</div>
        ${st.ready ? `<h3>What's new in ${esc(st.ready.version)}</h3><div class="notes">${para(st.ready.notes)}</div>` : ""}
        ${notes ? `<h3>What's new in ${esc(notes.version)}</h3><div class="notes">${para(notes.notes)}</div>` : ""}</div>` : ""}
      <div class="card" data-appliance-version><div class="spread"><div><strong>The server</strong> <span class="mono">${esc(ver.version)}</span></div></div>
        ${administers() ? `<div class="small muted">Install, roll back and the update channel are in <a href="#/admin/operations">Admin › Operations</a>.</div>` : ""}</div>`);
  };
  // A look that ends while this is showing (a download finishing) shows here
  // too, not only in the sidebar.
  App.redrawUpdates = () => el.isConnected ? draw() : (App.redrawUpdates = null);
  el.onclick = async e => {
    const b = e.target.closest("button[data-act]"); if (!b) return;
    try {
      if (b.dataset.act === "restart-update") return restartToUpdate();
      if (b.dataset.act === "look") { b.disabled = true; await Magnus.updateLook(); await refreshUpdate(); }
      el._html = null; await draw();
    } catch (err) { toast(err.message); b.disabled = false; }
  };
  await draw();
}

// One grant in Settings: what it covers, what still asks, its spend this
// period, the actions taken under it and its policy, with Edit and Revoke.
function grantRow(g, apps, editing) {
  const acts = g.actions || [];
  return `<div class="grant-row" data-grant="${esc(g.id)}" style="margin-top:10px">
    <div class="spread"><strong>${esc(g.title)}</strong>${g.revoked ? `<span class="chip">Revoked ${esc(when(g.revoked))}</span>` : `<span class="row"><button data-act="grant-edit">Edit</button><button data-act="grant-revoke">Revoke</button></span>`}</div>
    <p class="small muted">In ${esc(g.apps.join(", "))} · still asks before ${esc(g.still_asks.join(", "))} · <span class="num">$${esc(Number(g.spent).toFixed(2))} of $${esc(g.max_usd)} this ${esc(g.period)}</span></p>
    ${editing === g.id ? `${grantForm(g.id, g, apps)}<div class="row"><button class="primary" data-act="grant-save">Save</button><button class="link" data-act="grant-cancel">Cancel</button></div>` : ""}
    ${acts.length ? `<details data-key="grant-actions:${esc(g.id)}"><summary class="small">${acts.length} action${acts.length === 1 ? "" : "s"} taken under it</summary><ul class="small">${acts.slice(0, 20).map(a => `<li><span class="faint">${esc(when(a.at))}</span> ${esc(a.tool)} in <a href="#/task/${enc(a.task)}" data-open="task:${esc(a.task)}">${esc(a.task_title || "a task")}</a></li>`).join("")}</ul></details>` : `<p class="small faint">Nothing done under it yet.</p>`}
    <details data-key="grant-policy:${esc(g.id)}"><summary class="small">Its policy</summary><pre class="mono small">${esc(g.cedar)}</pre></details>
  </div>`;
}

// One commitment in Settings (#236): who said they would do what, for
// whom, by when, overdue shown, where it was said; Done, Release, Edit and
// Remove.
function commitmentRow(c, editing) {
  const STATE = { done: ["ok", "Done"], released: ["", "Released"] };
  const who = c.who === "magnus" ? "Magnus" : "You";
  const from = c.source?.kind === "app" ? `read in ${esc(c.source.app)}` : "said in a conversation";
  const status = c.status === "open" ? (c.overdue ? `<span class="chip bad">Overdue</span>` : "") : `<span class="chip ${STATE[c.status]?.[0] || ""}">${esc(STATE[c.status]?.[1] || c.status)}</span>`;
  const acts = c.status === "open" ? `<button data-act="cmt-done">Done</button><button data-act="cmt-release">Release</button>` : `<button data-act="cmt-reopen">Reopen</button>`;
  return `<div class="cmt-row" data-commitment="${esc(c.id)}" style="margin-top:10px">
    <div class="spread"><span><strong>${who}:</strong> ${esc(c.what)} ${status}</span><span class="row">${acts}<button data-act="cmt-edit">Edit</button><button data-act="cmt-remove">Remove</button></span></div>
    <p class="small muted">${c.to ? `For ${esc(c.to)} · ` : ""}${c.by ? `By ${esc(c.by)} · ` : ""}${from}, ${esc(when(c.at))}${c.quote ? ` · <span class="faint">“${esc(c.quote)}”</span>` : ""}</p>
    ${editing === c.id ? `<div class="row" style="margin-top:6px"><input data-f="what" aria-label="What" value="${esc(c.what)}" style="flex:2"><input data-f="to" aria-label="For whom" placeholder="For whom" value="${esc(c.to || "")}" style="flex:1"><input data-f="by" aria-label="By when" placeholder="YYYY-MM-DD" value="${esc(c.by || "")}" style="flex:1"></div>
      <div class="row"><button class="primary" data-act="cmt-save">Save</button><button class="link" data-act="cmt-cancel">Cancel</button></div>` : ""}
  </div>`;
}

async function dataSection(el) {
  // The person's relationship notes as last drawn: each row names its note by them.
  let notes = [];
  // The grant being edited, if any.
  let editing = null;
  // The commitment being edited, if any.
  let editingC = null;
  const draw = async () => {
    const [prefs, { requests }, rel, { disagreements }, granted, { commitments }] = await Promise.all([api("GET", "/v1/preferences"), api("GET", "/v1/deletion-requests"), api("GET", "/v1/relationship"), api("GET", "/v1/disagreements"), api("GET", "/v1/grants"), api("GET", "/v1/commitments")]);
    App.prefs = prefs;
    notes = rel.notes;
    const STATE = { done: ["ok", "done"], decline: ["bad", "declined"] };
    paint(el, `<h2>Data and privacy</h2>
      <div class="card"><label class="row toggle"><input type="checkbox" data-pref="feedback_consent" ${prefs.feedback_consent ? "checked" : ""}>
        <span><span>Send problem reports</span><span class="small muted" style="display:block">When you report a problem, what you wrote and the task's record go to the people who build Magnus. Off: nothing leaves the server.</span></span></label></div>
      <h3>How Magnus works with you</h3>
      <div class="card" data-relationship><p class="small muted">What you told Magnus about working with you: tone, what you delegated, standing decisions. Every task and chat of yours carries it; no one else sees it.</p>
        ${notes.map((n, i) => `<div class="row" data-note="${i}" style="margin-top:6px"><input aria-label="Note" value="${esc(n)}" style="flex:1"><button data-act="note-save">Save</button><button data-act="note-remove">Remove</button></div>`).join("") || `<p class="small faint">Nothing yet.</p>`}
        <div class="row" style="margin-top:8px"><input id="newnote" aria-label="New note" style="flex:1" placeholder="For example: fewer jokes"><button data-act="note-add">Add</button></div></div>
      <h3>What Magnus may do on its own</h3>
      <div class="card" data-grants><p class="small muted">Kinds of your work you let Magnus handle without asking, within your organisation's policy, each with what was done under it. They are yours alone; spending money always asks.</p>
        ${granted.grants.map(g => grantRow(g, granted.apps, editing)).join("") || `<p class="small faint">None yet. Choose "Handle things like this" on a task Magnus proposes, or ask Magnus.</p>`}</div>
      <h3>What you and Magnus said you'd do</h3>
      <div class="card" data-commitments><p class="small muted">What you or Magnus said you would do, recognised from your conversations and what Magnus read for you, with for whom and by when. Open ones are in every task and chat of yours; no one else sees them.</p>
        ${commitments.map(c => commitmentRow(c, editingC)).join("") || `<p class="small faint">None yet.</p>`}</div>
      <h3>Where you decided otherwise</h3>
      <div class="card" data-disagreements><p class="small muted">When Magnus advised one thing and you decided another, it notes both, and later how it turned out. Only your own tasks and chats see these.</p>
        ${disagreements.map(d => `<p class="small"><span class="faint">${esc(when(d.at))}</span> Magnus advised: ${esc(d.position)} You decided: ${esc(d.decision)} ${d.outcome ? `Outcome: ${esc(d.outcome)}` : `<span class="faint">Outcome not known yet.</span>`}</p>`).join("") || `<p class="small faint">None.</p>`}</div>
      <h3>Delete my data</h3>
      <div class="card"><p class="small muted">The owner is asked, and has 30 days to act. What is deleted leaves every backup within that time.</p>
        <label for="delwhat">What to delete</label><input id="delwhat" data-draft="delwhat" style="width:100%" placeholder="all of my data">
        <div class="row" style="margin-top:8px"><button data-act="delete-request">Ask for deletion</button></div>
        ${requests.length ? `<table class="t small" data-deletions style="margin-top:10px"><tbody>${requests.map(q => `<tr><td>${esc(q.what)}${isOwner() && q.person !== App.me.id ? ` <span class="faint">(${esc(q.person)})</span>` : ""}</td>
          <td>${q.decision ? `<span class="chip ${STATE[q.decision]?.[0] || ""}">${esc(STATE[q.decision]?.[1] || q.decision)}</span>${q.note ? ` <span class="muted">${esc(q.note)}</span>` : ""}` : `<span class="chip warn">asked</span> due ${esc(new Date(q.due).toLocaleDateString())}`}</td></tr>`).join("")}</tbody></table>` : ""}</div>
      ${administers() ? `<p class="small muted" data-backups-moved>Backups and restore checks are in <a href="#/admin/operations">Admin › Operations</a>.</p>` : ""}`);
  };
  el.onchange = async e => {
    const c = e.target.closest("input[data-pref]"); if (!c) return;
    try { App.prefs = await api("PUT", "/v1/preferences", { [c.dataset.pref]: c.checked }); toast("Saved."); } catch (err) { toast(err.message); c.checked = !c.checked; }
  };
  el.onclick = async e => {
    const b = e.target.closest("button[data-act]"); if (!b) return;
    try {
      if (b.dataset.act === "delete-request") {
        const what = $("#delwhat", el).value.trim();
        if (!await ask(`Ask the owner to delete ${what || "all of your data"}?`, "Ask the owner")) return;
        b.disabled = true; await api("POST", "/v1/deletion-requests", { what }); delete drafts.delwhat; toast("Asked. The owner has 30 days to act.");
      }
      const row = b.closest("[data-note]"), note = row && notes[+row.dataset.note];
      if (b.dataset.act === "note-add") { b.disabled = true; await api("POST", "/v1/relationship", { action: "add", text: $("#newnote", el).value }); toast("Saved."); }
      if (b.dataset.act === "note-save") { b.disabled = true; await api("POST", "/v1/relationship", { action: "change", note, text: $("input", row).value }); toast("Saved."); }
      if (b.dataset.act === "note-remove") { b.disabled = true; await api("POST", "/v1/relationship", { action: "remove", note }); toast("Removed."); }
      const g = b.closest("[data-grant]")?.dataset.grant;
      if (b.dataset.act === "grant-edit") editing = g;
      if (b.dataset.act === "grant-cancel") editing = null;
      if (b.dataset.act === "grant-save") {
        const f = readGrantForm(b.closest("[data-grant]"));
        if (!f.apps.length) return toast("Choose at least one app.");
        b.disabled = true; await api("PUT", `/v1/grants/${enc(g)}`, f); editing = null; toast("Saved.");
        for (const k of Object.keys(drafts)) if (k.startsWith(`g-${g}-`)) delete drafts[k];
      }
      if (b.dataset.act === "grant-revoke") {
        if (!await ask("Revoke this? From now on, what it covered asks you again.", "Revoke")) return;
        b.disabled = true; await api("DELETE", `/v1/grants/${enc(g)}`); toast("Revoked.");
      }
      const cr = b.closest("[data-commitment]"), c = cr?.dataset.commitment;
      if (b.dataset.act === "cmt-edit") editingC = c;
      if (b.dataset.act === "cmt-cancel") editingC = null;
      const status = { "cmt-done": "done", "cmt-release": "released", "cmt-reopen": "open" }[b.dataset.act];
      if (status) { b.disabled = true; await api("PUT", `/v1/commitments/${enc(c)}`, { status }); toast(status === "done" ? "Marked done." : status === "released" ? "Released." : "Reopened."); }
      if (b.dataset.act === "cmt-save") {
        const f = k => $(`[data-f="${k}"]`, cr).value.trim();
        if (!f("what")) return toast("Say what will be done.");
        b.disabled = true; await api("PUT", `/v1/commitments/${enc(c)}`, { what: f("what"), to: f("to"), by: f("by") }); editingC = null; toast("Saved.");
      }
      if (b.dataset.act === "cmt-remove") {
        if (!await ask("Remove this? It leaves your list and Magnus's context, and its words are deleted.", "Remove")) return;
        b.disabled = true; await api("DELETE", `/v1/commitments/${enc(c)}`); toast("Removed.");
      }
      el._html = null; await draw();
    } catch (err) { toast(err.message); b.disabled = false; }
  };
  await draw();
}

// ---------- start ----------

let background = [];
// ---------- the app's own updates ----------

// `look`: ask the appliance now, as when a session begins (the app's hourly
// look may have run before it was connected).
async function refreshUpdate(look = false) {
  try { App.update = await (look ? Magnus.updateLook() : Magnus.updateState()); } catch { App.update = await Magnus.updateState().catch(() => null); }
  if ($("#side")) sidebar();
  await App.redrawUpdates?.();
}

async function restartToUpdate() {
  toast("Installing the update; Magnus restarts in a moment.", 10000);
  try { await Magnus.updateInstall(); } catch (e) { toast(`The update did not install: ${e.message}`); refreshUpdate(); }
}

// What changed, in the release's own plain words, once after an update.
async function showWhatsNew() {
  let n = null;
  try { n = await Magnus.whatsNew(); } catch { return; }
  if (!n) return;
  const box = document.createElement("div");
  box.className = "whats-new card"; box.setAttribute("role", "dialog"); box.setAttribute("aria-label", "What's new");
  box.innerHTML = `<h3>What's new in Magnus ${esc(n.version)}</h3>
    <div class="notes">${esc(n.notes || "Fixes and improvements.").split(/\n+/).map(l => `<p>${l}</p>`).join("")}</div>
    <div class="row"><button class="primary" data-act="close">Got it</button></div>`;
  box.querySelector("[data-act=close]").onclick = () => box.remove();
  document.body.append(box);
}

// The appliance no longer accepts this version of the app: only updating helps.
let updateScreen = null;
async function updateRequiredScreen(info) {
  if (updateScreen) return;
  stop();
  document.body.innerHTML = `<div class="signin"><h1>Magnus</h1><h3>Update required</h3>
    <p class="muted">${esc(info?.error || "The server needs a newer version of this app.")}</p><div class="card" id="ur"></div></div>`;
  const draw = async () => {
    let st = null;
    try { st = await Magnus.updateState(); } catch { /* shown below */ }
    const box = $("#ur");
    if (!box) return;
    if (!st) paint(box, `<p>Open the server's address in a browser to use it until the app is updated.</p>`);
    else if (st.ready) paint(box, `<p>Magnus ${esc(st.ready.version)} is downloaded and checked.</p><div class="row"><button class="primary" data-act="restart-update">Restart to update</button></div>`);
    else if (st.can_update) paint(box, `<p>${esc(st.can_update)}</p>`);
    else paint(box, `<p>${st.error ? `The update could not be fetched: ${esc(st.error)}` : "Downloading the update…"}</p><div class="row"><button data-act="look">Try again</button></div>`);
  };
  document.body.addEventListener("click", async e => {
    if (e.target.closest("[data-act=restart-update]")) restartToUpdate();
    if (e.target.closest("[data-act=look]")) { try { await Magnus.updateLook(); } catch { /* drawn */ } draw(); }
  });
  Magnus.onUpdateReady(draw);
  updateScreen = setInterval(draw, 5000);
  draw();
}
window.addEventListener("magnus-update-required", e => updateRequiredScreen(e.detail));

async function boot() {
  await loadUiCatalog();
  App.sessionGeneration++;
  Resources.reset();
  App.me = await api("GET", "/v1/me");
  // The Terms of Use first, then the usage-data question (agreements.js).
  if (!(await agreementsGate())) {
    stop();
    await Magnus.signOut();
    location.hash = "";
    return start();
  }
  Theme.scope(App.appliance, App.me.id);
  MessageDrafts.open(App.appliance, App.me.id);
  const host = await Magnus.systemLook().catch(() => ({}));
  const canRead = () => { App.canReadAloud = !!App.me?.read_aloud || !!host.native_speech || ("speechSynthesis" in window && speechSynthesis.getVoices().length > 0); };
  canRead();
  if ("speechSynthesis" in window) speechSynthesis.addEventListener("voiceschanged", () => { canRead(); App.redraw?.(); });
  shell();
  adminStart();
  window.onhashchange = go;
  Magnus.onUpdateReady(() => refreshUpdate());
  refreshUpdate(true);
  showWhatsNew();
  App.prefs = await api("GET", "/v1/preferences").catch(() => null);
  await Promise.all([refresh(), pollVersions()]);
  // Opening the app shows what changed since the last session: with
  // news, and landing on home, the needs panel opens beside it (#367).
  App.reported = new Set();
  App.reporting = new Set();
  App.feed = await api("POST", "/v1/home/opened", {}).catch(() => null);
  App.feedDismissed = false;
  // Return-session details stay in Activity; informational arrivals do
  // not switch the selected region or hide the conversation's composer.
  banners();
  await Notices.poll();
  background = [setInterval(refresh, 3000), setInterval(pollVersions, 30000), setInterval(() => Notices.poll(), 3000)];
  go();
}

function stop() {
  App.sessionGeneration++;
  Resources.reset();
  background.forEach(clearInterval); background = [];
  Notices.reset();
  App.timers.forEach(clearInterval); App.timers = [];
  App.leave.forEach(f => { try { f(); } catch { /* ignore */ } }); App.leave = [];
  window.onhashchange = null;
  Panel.what = null;
  Panel.rendered=null;Resources.forget("Work");
  Theme.scope(null, null);
  adminStop();
  Panel.unwatch?.(); Panel.unwatch=null; Panel.anchors={}; Panel.layers={}; Panel.opener=null; Panel.drawn=null; Panel.what=null; App.draftRecord=null;
  App.me = null; App.computers = null; App.inbox = []; App.tasks = []; App.schedules = []; App.threads = []; App.projects = []; App.thread = null; App.feed = null; App.reported = new Set(); App.reporting = new Set();
  // What one person was typing or filtering by is not the next one's here.
  Panel.lib = { q: "", project: "" };
  delete Lib.all; Lib.q=""; Lib.cat="all"; Lib.tab="discover";
  for (const k of Object.keys(drafts)) delete drafts[k];
}

async function start() {
  try {
    const r = await Magnus.restore();
    if (r && r.update_required) return updateRequiredScreen(r.update_required);
    if (r && r.person) { App.me = r.person; App.appliance = r.appliance; return boot(); }
    signInScreen(r && r.note);
  } catch (e) { signInScreen(e.message || String(e)); }
}

document.addEventListener("DOMContentLoaded", start);
