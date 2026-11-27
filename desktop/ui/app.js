// The operator view: Home, Inbox, New Task, Task Detail and Models, all from
// derived views of the log (K45). The developer view is in dev.js.
"use strict";

const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
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
      try{return await Server.speak(s);}catch(e){
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
  workspaces: [],
  connectors: null,
  health: null,
  thread: null,
  /** Active shared conversation context (`{ ws, channel, home, missions, panel }`) when viewing a shared conversation. */
  shared: null,
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

// Capture an entry and its relative offset before content/layout changes.
const ReadingAnchors = (() => {
  const state = new WeakMap();
  const programmatic = new WeakMap();
  const lastTop = new WeakMap();
  const nearBottom = root => root.scrollHeight - root.scrollTop - root.clientHeight <= 80;
  function capture(root) {
    if (!root) return null;
    if (!root.clientHeight) return state.get(root) || null;
    const prev = state.get(root);
    const follow = nearBottom(root) || (!!prev?.follow && root.scrollTop >= (lastTop.get(root) ?? 0) - 2);
    const bounds = root.getBoundingClientRect();
    const frames=[...root.querySelectorAll("iframe.composed")];
    for (const [frameIndex,frame] of frames.entries()) {
      const rect=frame.getBoundingClientRect();
      if (rect.top>bounds.top || rect.bottom<=bounds.top) continue;
      try {
        const nodes=[...frame.contentDocument.querySelectorAll("h1,h2,h3,h4,p,li,tr,pre")];
        const index=nodes.findIndex(n=>rect.top+n.getBoundingClientRect().bottom>bounds.top);
        if (index>=0) return {follow,top:root.scrollTop,frameIndex,nodeIndex:index,offset:rect.top+nodes[index].getBoundingClientRect().top-bounds.top};
      } catch { /* frame not loaded */ }
    }
    const node = [...root.querySelectorAll("[data-entry], [data-shown], [data-key], h2, h3, p")].find(n => n.getBoundingClientRect().bottom > bounds.top);
    return {follow, top:root.scrollTop,
      entry:node?.dataset.entry, shown:node?.dataset.shown, key:node?.dataset.key,
      index:node ? [...root.querySelectorAll("h2,h3,p")].indexOf(node) : -1,
      offset:node ? node.getBoundingClientRect().top-bounds.top : 0};
  }
  function setTop(root, top) {
    root.scrollTop = top;
    const actual = root.scrollTop;
    programmatic.set(root, actual);
    lastTop.set(root, actual);
  }
  function restore(root, anchor) {
    if (!root || !anchor || !root.clientHeight) return;
    if (anchor.follow) setTop(root, root.scrollHeight);
    else if (anchor.frameIndex!==undefined) {
      try {
        const frame=root.querySelectorAll("iframe.composed")[anchor.frameIndex];
        const node=frame?.contentDocument.querySelectorAll("h1,h2,h3,h4,p,li,tr,pre")[anchor.nodeIndex];
        setTop(root, node ? root.scrollTop+frame.getBoundingClientRect().top+node.getBoundingClientRect().top-root.getBoundingClientRect().top-anchor.offset : anchor.top);
      } catch { setTop(root, anchor.top); }
    } else {
      const node = anchor.entry ? root.querySelector(`[data-entry="${CSS.escape(anchor.entry)}"]`) : anchor.shown ? root.querySelector(`[data-shown="${CSS.escape(anchor.shown)}"]`) : anchor.key ? root.querySelector(`[data-key="${CSS.escape(anchor.key)}"]`) : root.querySelectorAll("h2,h3,p")[anchor.index];
      setTop(root, node ? root.scrollTop + node.getBoundingClientRect().top-root.getBoundingClientRect().top-anchor.offset : anchor.top);
    }
  }
  function remember(root, preferred) { const a = preferred || capture(root); if (root) { state.set(root,a); lastTop.set(root, root.scrollTop); } return a; }
  function hold(root, change) { const a = capture(root); const result = change(); restore(root,a); if (root) state.set(root,a); return result; }
  function watch(root, record, persist=true) {
    if (record?.anchor) restore(root,record.anchor); else restore(root,{follow:true});
    state.set(root,record?.anchor || (persist ? { follow: true } : capture(root)));
    lastTop.set(root, root.scrollTop);
    const onScroll = () => {
      if (!root.clientHeight) return;
      const prev = state.get(root);
      // Ignore programmatic restores; if content grew after restoring bottom, re-pin instead of detaching.
      const prog = programmatic.get(root);
      if (prog !== undefined && Math.abs(root.scrollTop - prog) < 2) {
        programmatic.delete(root);
        if (persist && prev?.follow && !nearBottom(root)) setTop(root, root.scrollHeight);
        return;
      }
      programmatic.delete(root);
      const prevTop = lastTop.get(root) ?? root.scrollTop;
      const movedUp = root.scrollTop < prevTop - 2;
      if (persist && prev?.follow && !movedUp && !nearBottom(root)) {
        // Content grew or viewport shrank (e.g. iframe load / bento open) without upward scroll: stay pinned to bottom.
        setTop(root, root.scrollHeight);
        return;
      }
      const a = capture(root);
      if (!persist) a.follow = false;
      else if (movedUp && !nearBottom(root)) a.follow = false;
      remember(root, a);
      if (a.follow) root.parentElement?.querySelector("[data-jump-latest]")?.setAttribute("hidden", "");
      if (record) record.anchor = a;
    };
    root.addEventListener("scroll",onScroll,{passive:true});
    const resize = new ResizeObserver(() => {
      restore(root,state.get(root));
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
 function bad(name,error){if(error?.cancelled)return;const s=states.get(name)||{};states.set(name,{...s,failed:true});render();if(error?.status===401 && App.me)endSession("Your session ended. Sign in again.");}
 async function read(name,path,key,cached=false){const scope=epoch,token=++serial,prior=states.get(name)||{};states.set(name,{...prior,token});
  try{const response=await Server.call("GET",path);if(scope!==epoch || states.get(name)?.token!==token)return;const value=key ? response[key] : response;states.set(name,{...states.get(name),path,value});good(name);return value;}
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
    Server.inbox.opened([...ids]).then(response=>{
      for(const id of response.opened || [])reported.add(id);
    },()=>{}).finally(()=>{for(const id of ids)reporting.delete(id);});
  });
}

// A question before an action, asked in the app itself: a webview may
// ignore window.confirm and answer no without showing it, so a button
// behind one did nothing at all (the Windows app, owner 2026-11-03:
// Disconnect in the Library). Escape, or Back, is no.
function ask(msg, yes = "Approve", danger = false) {
  return new Promise(done => {
    const d = document.createElement("dialog");
    const msgId = `ask-msg-${Math.random().toString(36).slice(2, 8)}`;
    d.className = "ask";
    d.setAttribute("aria-describedby", msgId);
    d.innerHTML = `<p id="${msgId}"></p><div class="row"><button data-ask="no">Back</button><button class="${danger ? "danger" : "primary"}" data-ask="yes"></button></div>`;
    $("p", d).textContent = msg;
    $("[data-ask=yes]", d).textContent = yes;
    d.addEventListener("click", e => {
      if (e.target === d) { d.close("no"); return; }
      const b = e.target.closest("[data-ask]");
      if (b) d.close(b.dataset.ask);
    });
    d.addEventListener("close", () => { d.remove(); done(d.returnValue === "yes"); });
    document.body.append(d);
    d.showModal();
    $("[data-ask=no]", d).focus();
  });
}

function askText(msg, placeholder = "", yes = "Submit", danger = false) {
  const opts = typeof msg === "object" && msg !== null
    ? { title: msg.title || "", body: msg.body || "", placeholder: msg.placeholder || "", yes: msg.okLabel || msg.yes || "Submit", danger: !!msg.danger }
    : { title: "", body: String(msg ?? ""), placeholder, yes, danger };
  return new Promise(done => {
    const d = document.createElement("dialog");
    const uid = Math.random().toString(36).slice(2, 8);
    d.className = "ask";
    if (opts.title) d.setAttribute("aria-labelledby", `ask-t-${uid}`);
    d.setAttribute("aria-describedby", `ask-b-${uid}`);
    d.innerHTML = `<form>${opts.title ? `<h3 id="ask-t-${uid}"></h3>` : ""}<p id="ask-b-${uid}"></p><input type="text" data-ask-input required style="width:100%;margin-bottom:14px"><div class="row"><button type="button" data-ask="no">Back</button><button type="submit" class="${opts.danger ? "danger" : "primary"}" data-ask="yes"></button></div></form>`;
    if (opts.title) $("h3", d).textContent = opts.title;
    $("p", d).textContent = opts.body;
    const inp = $("[data-ask-input]", d);
    inp.placeholder = opts.placeholder;
    $("[data-ask=yes]", d).textContent = opts.yes;
    d.addEventListener("click", e => { if (e.target === d || e.target.closest("[data-ask=no]")) d.close("no"); });
    $("form", d).addEventListener("submit", e => { e.preventDefault(); d.close("yes"); });
    d.addEventListener("close", () => { const v = d.returnValue === "yes" ? inp.value.trim() : null; d.remove(); done(v || null); });
    document.body.append(d);
    d.showModal();
    inp.focus();
  });
}

function toast(msg, ms = 3200, action = null) {
  // Aurora says it on its stage, in the line above the composer (unless an action button is needed).
  if (!action && typeof AuroraReasoning !== "undefined" && AuroraReasoning.tell(String(msg), ms)) return;
  const t = document.createElement("div");
  t.className = "toast"; t.setAttribute("role", "status");
  const text = document.createElement("span");
  text.textContent = msg;
  t.append(text);
  let timer = 0, remaining = ms, started = Date.now();
  const clear = () => { if (timer) { clearTimeout(timer); timer = 0; } };
  const arm = delay => { clear(); started = Date.now(); timer = setTimeout(() => t.remove(), Math.max(0, delay)); };
  const pause = () => { if (!timer) return; remaining = Math.max(1200, remaining - (Date.now() - started)); clear(); };
  const resume = () => { if (!timer && t.isConnected) arm(remaining); };
  if (action && action.label && typeof action.onClick === "function") {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "toast-act";
    btn.textContent = action.label;
    btn.onclick = () => { clear(); t.remove(); action.onClick(); };
    t.append(btn);
    t.addEventListener("mouseenter", pause);
    t.addEventListener("mouseleave", resume);
    t.addEventListener("focusin", pause);
    t.addEventListener("focusout", resume);
  }
  // Toasts stack, newest at the bottom, rather than covering each other.
  let box = $("#toasts");
  if (!box) { box = document.createElement("div"); box.id = "toasts"; document.body.append(box); }
  box.append(t);
  arm(ms);
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
  const closed = new Set($$("details[data-key]", el).filter(d => !d.open).map(d => d.dataset.key));
  const focused = el.contains(active) ? active?.dataset?.draft : undefined;
  const control = el.contains(active) && !focused ? {
    id:active.id, entry:active.closest("[data-entry]")?.dataset.entry, msg:active.closest("[data-msg]")?.dataset.msg,
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
  $$("details[data-key]", el).forEach(d => { if (opened.has(d.dataset.key)) d.open = true; else if (closed.has(d.dataset.key)) d.open = false; });
  $$("[data-draft]", el).forEach(i => { const v = drafts[i.dataset.draft]; if (v === undefined) return; if (i.type === "checkbox") i.checked = !!v; else i.value = v; });
  if (focused) $(`[data-draft="${CSS.escape(focused)}"]`, el)?.focus({preventScroll:true});
  if (control) {
    const within=control.entry ? $(`[data-entry="${CSS.escape(control.entry)}"]`,el) : control.msg ? $(`[data-msg="${CSS.escape(control.msg)}"]`,el) : el;
    const target=control.id ? $(`#${CSS.escape(control.id)}`,el) : within && (control.act ? $(`[data-msg-act="${CSS.escape(control.act)}"]`,within) : control.open ? $(`[data-open="${CSS.escape(control.open)}"]`,within) : control.label ? $(`${control.tag}[aria-label="${CSS.escape(control.label)}"]`,within) : null);
    target?.focus({preventScroll:true});
  }
  Menus.restore(menuState);
  reportOpenedSoon();
  if (typeof window !== "undefined" && typeof CustomEvent === "function" && !paint._queued) {
    paint._queued = true;
    queueMicrotask(() => { paint._queued = false; window.dispatchEvent(new CustomEvent("magnus-render")); });
  }
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
  spend_limit: "paused: a monthly spend limit is reached", paused_by_user: "paused by you",
};
const chip = st => { const [c, l] = STATUS[st] || ["", st]; return `<span class="chip ${c}">${esc(l)}</span>`; };

// ---------- shell (lives in app-shell.js) ----------

// ---------- Files and projects ----------

const kb = n => `${Math.max(1, Math.round(n / 1024))} KB`;
const ENDED = ["completed", "failed", "cancelled"];
// File chips, verdictBar, upload, project move controls, previews, and glimpses live in app-files.js.

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
  main.className = "";
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

// ---------- background: inbox, notifications, versions (lives in app-polling.js) ----------

function describeChange(c) {
  const what = { harness: "harness", instructions: "instructions", model: "model", playbook: "playbook" }[c.what] || c.what.replace("policy:", "policy for ");
  return `${what} is now ${c.value}`;
}

// ---------- the workspace's objects ----------

const REFRESH_VISIBLE_MS = 3000, REFRESH_HIDDEN_MS = 30_000, VERSION_POLL_MS = 30_000, NOTICES_VISIBLE_MS = 3000, NOTICES_HIDDEN_MS = 15_000;
let refreshInFlight = null, lastVersionPollMs = 0, lastRefreshMs = 0, lastNoticesPollMs = 0, visibilityHandler = null;

// Tasks, schedules, conversations and projects, read together with the
// inbox; then every view that shows them is drawn again from what was read.
async function refresh() {
  if (refreshInFlight) return refreshInFlight;
  refreshInFlight = (async () => {
    const get = (path, k) => Resources.read(({tasks:"Tasks",schedules:"Schedules",threads:"Conversations",projects:"Projects",connectors:"Apps"})[k],path,k);
    // The person's computers, for the conversation's head: whether one is in use, and by which agent.
    const computers = Server.computers.list().then(r => { App.computers = r.computers; followDesktopLook(); }, () => { /* Without a computer service there is nothing to show. */ });
    const workspaces = Server.workspaces.list().then(async r => {
      if (!Array.isArray(r)) return;
      App.workspaces = r;
      if (typeof loadHome === "function" && r.length) {
        await Promise.all(r.map(w => loadHome(w.id).catch(() => null)));
      }
    }, () => {});
    const now = Date.now();
    const version = (!lastVersionPollMs || now - lastVersionPollMs >= VERSION_POLL_MS)
      ? Server.version({ background: true }).then(v => {
          lastVersionPollMs = Date.now();
          if (!v?.version) return;
          if (App.serverVersion && App.serverVersion !== v.version) refreshUpdate(true);
          App.serverVersion = v.version;
        }, () => {})
      : Promise.resolve();
    const staged = typeof Server !== "undefined" && Server.staged?.list
      ? Server.staged.list().then(r => { App.staged = r?.items || []; }, () => {})
      : Promise.resolve();
    const briefing = typeof Server !== "undefined" && Server.briefing
      ? Server.briefing().then(b => { App.briefing = b; }, () => {})
      : Promise.resolve();
    const [tasks, schedules, threads, projects, connectors, health] = await Promise.all([get("/v1/tasks", "tasks"), get("/v1/schedules", "schedules"), get("/v1/chat", "threads"), get("/v1/projects", "projects"), get("/v1/connectors", "connectors"), Resources.read("Server", "/v1/health"), pollInbox(), computers, workspaces, version, staged, briefing]);
    if (!App.me) return;
    // Magnus's own background reviews are plumbing, not the person's tasks
    // (owner, 2026-11-07): developers see them in the run inspector.
    if (tasks) App.tasks = tasks.filter(t => !t.review);
    if (schedules) App.schedules = schedules;
    if (threads) App.threads = threads;
    if (projects) App.projects = projects;
    if (Array.isArray(connectors)) { App.connectors = connectors; Lib.all = connectors.filter(c => c.for_you !== false); }
    if (health) App.health = health;
    if (typeof UIStore !== "undefined") {
      if (App.tasks) UIStore.setTasks(App.tasks);
      UIStore.patch({
        route: location.hash || "",
        asks: typeof needsMe === "function" ? needsMe() : [],
        connectedApps: (App.connectors || []).filter(c => c.connected).map(c => c.name || c.id).filter(Boolean),
      });
    }
    lastRefreshMs = Date.now();
    sidebar();
    banners();
    await Promise.all([App.redraw?.(), drawPanel(), typeof AuroraStage !== "undefined" && AuroraStage.refresh()]);
  })();
  try { return await refreshInFlight; }
  finally { refreshInFlight = null; }
}

async function refreshDomains(domains) {
  if (!App.me || !Array.isArray(domains) || !domains.length) return;
  const want = new Set(domains);
  const get = (path, k) => Resources.read(({ tasks: "Tasks", schedules: "Schedules", threads: "Conversations", connectors: "Apps" })[k], path, k);
  const [tasks, schedules, threads, connectors] = await Promise.all([
    want.has("tasks") ? get("/v1/tasks", "tasks") : null,
    want.has("schedules") ? get("/v1/schedules", "schedules") : null,
    want.has("chat") ? get("/v1/chat", "threads") : null,
    want.has("connectors") ? get("/v1/connectors", "connectors") : null,
    want.has("inbox") ? pollInbox() : null,
  ]);
  if (!App.me) return;
  if (tasks) App.tasks = tasks.filter(t => !t.review);
  if (schedules) App.schedules = schedules;
  if (threads) App.threads = threads;
  if (Array.isArray(connectors)) { App.connectors = connectors; Lib.all = connectors.filter(c => c.for_you !== false); }
  if (typeof UIStore !== "undefined") {
    if (tasks) UIStore.setTasks(App.tasks);
    UIStore.patch({
      route: location.hash || "",
      asks: typeof needsMe === "function" ? needsMe() : [],
      connectedApps: (App.connectors || []).filter(c => c.connected).map(c => c.name || c.id).filter(Boolean),
    });
  }
  lastRefreshMs = Date.now();
  sidebar();
  banners();
  await Promise.all([App.redraw?.(), drawPanel(), typeof AuroraStage !== "undefined" && AuroraStage.refresh()]);
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
  return `<div data-kill-switch="${ks.engaged ? "on" : "off"}"><div class="spread"><h3>Kill switch</h3>${ks.engaged ? `<button type="button" data-overview-kill="release">Release</button>` : `<button type="button" class="danger" data-overview-kill="press">Stop everything</button>`}</div>
    ${ks.engaged
      ? `<p><strong>On.</strong> Every action is refused and every task is suspended. Nothing resumes when it is released; each person continues their own tasks.</p>`
      : `<p class="small muted">Off. Pressing stops every task, revokes every sign-in at its provider, and seals the vault at once.</p>`}
    ${(ks.sealed || []).length ? `<h3>Sealed until you turn it off</h3><p class="small muted">API keys and app secrets cannot be revoked from here; if you think one leaked, revoke it in that provider's console.</p>
      <ul class="small" data-sealed>${ks.sealed.map(n => `<li class="mono" data-secret="${esc(n)}">${esc(n)}</li>`).join("")}</ul>` : ""}
    ${revoked.length ? `<h3>Revoked${ks.engaged ? "" : ": connect these again"}</h3><table class="t" data-revoked><tbody>${revoked.map(([n, r]) => `<tr data-secret="${esc(n)}"><td class="mono small">${esc(r.adapter ? `${r.adapter} sign-in` : n)}</td><td class="small">${esc(state(r))}</td></tr>`).join("")}</tbody></table>
      ${ks.engaged ? "" : `<p class="small"><a href="#/admin/models">Models</a>: sign in or enter keys again.</p>`}` : ""}
  </div>`;
}

// ---------- Model choice ----------

// The models a person may run a task or a message on, as the adapters'
// providers list them now. The appliance's default is used until the viewer
// picks another; a pick holds only while the default it was made under is
// still the default, so changing the default reaches new messages (#540).
const REASONING_LEVELS = [["", "Reasoning: Auto"], ["off", "Reasoning: Off"], ["low", "Reasoning: Low"], ["medium", "Reasoning: Medium"], ["high", "Reasoning: High"]];
const AUTONOMY_MODES = [
  ["standard", "Ask on external & money"],
  ["cautious", "Ask on every write"],
  ["draft_only", "Draft & read only"],
  ["plan_first", "Review plan first"],
];

async function modelPicker() {
  const { default: dflt, adapters } = await Server.models.list().catch(() => ({ default: null, adapters: [] }));
  const key = m => JSON.stringify([m.adapter, m.model]);
  const opts = adapters.flatMap(a => a.models.map(model => ({ adapter: a.adapter, model })));
  if (dflt && !opts.some(o => key(o) === key(dflt))) opts.unshift(dflt);
  const under = dflt ? key(dflt) : "";
  const saved = load("model-choice", null);
  const pick = opts.find(o => saved?.under === under && key(o) === key(saved)) || dflt || opts[0];
  const groups = [...new Set(opts.map(o => o.adapter))];
  const savedReasoning = load("reasoning-choice", "") || "";
  const savedAutonomy = load("autonomy-mode", "standard") || "standard";
  return `<label class="model"><span class="sr">Model</span><select data-model data-default="${esc(under)}" aria-label="Model">${groups.map(g => `<optgroup label="${esc(g)}">${opts.filter(o => o.adapter === g).map(o =>
    `<option value="${esc(key(o))}"${pick && key(o) === key(pick) ? " selected" : ""}>${esc(o.model)}${dflt && key(o) === key(dflt) ? " (default)" : ""}</option>`).join("")}</optgroup>`).join("")}</select></label><label class="model"><span class="sr">Reasoning level</span><select data-reasoning aria-label="Reasoning level">${REASONING_LEVELS.map(([val, label]) =>
    `<option value="${esc(val)}"${val === savedReasoning ? " selected" : ""}>${esc(label)}</option>`).join("")}</select></label><label class="small"><span class="sr">Autonomy mode</span><select data-autonomy aria-label="Autonomy mode" title="Choose how Magnus asks before acting">${AUTONOMY_MODES.map(([val, label]) =>
    `<option value="${esc(val)}"${val === savedAutonomy ? " selected" : ""}>${esc(label)}</option>`).join("")}</select></label>`;
}
function chosenAutonomy(root) {
  const sel = $("select[data-autonomy]", root);
  const mode = sel && AUTONOMY_MODES.some(([v]) => v === sel.value) ? sel.value : "standard";
  if (sel) save("autonomy-mode", mode);
  return mode;
}
function chosenModel(root) {
  const sel = $("select[data-model]", root);
  if (!sel || !sel.value) return undefined;
  const [adapter, model] = JSON.parse(sel.value);
  // Sending on the default is not a pick: nothing is kept to outlive it.
  save("model-choice", sel.value === sel.dataset.default ? null : { adapter, model, under: sel.dataset.default });
  const rsel = $("select[data-reasoning]", root);
  const reasoning = rsel && rsel.value ? rsel.value : undefined;
  if (rsel) save("reasoning-choice", rsel.value || "");
  return reasoning ? { adapter, model, reasoning } : { adapter, model };
}

// ---------- Chat (lives in app-chat.js) ----------
// ---------- Inbox (lives in app-inbox.js) ----------
// ---------- The panel beside the conversation (lives in app-panel.js) ----------
// ---------- The Library (lives in app-library.js) ----------
// ---------- Trash & Settings (lives in app-settings.js) ----------

// ---------- start & updates (lives in app-boot.js) ----------
