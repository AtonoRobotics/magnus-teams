// The admin console's page (admin console spec, Web UI). The shell: the
// top bar (organisation, status, kill switch, the admin's menu), the five
// sections, and the patterns every screen uses (`UI`): lists that open a
// detail panel, a toast with Undo for what can be undone, a confirm sheet
// for what cannot, the step-up sheet, live data over server-sent events,
// and designed empty, loading and error states.
"use strict";

const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const Admin = { me: null, live: null, status: null };
const when = t => (t ? new Date(t).toLocaleString() : "–");
const pct = x => (typeof x === "number" ? `${Math.round(x * 100)}%` : "–");
// Who did something, by name: an admin is named by their local id in
// the log, and the console knows the names from Users.
const who = by => {
  const b = String(by || "");
  const sub = b.startsWith("admin:") ? b.slice(6) : null;
  if (sub) return Admin.names?.[sub] ? `${Admin.names[sub]} (admin)` : "an admin";
  return b.replace(/^person:/, "");
};
async function loadNames() {
  try { Admin.names = Object.fromEntries((await api("GET", "/users")).users.map(u => [u.id, u.name])); } catch { /* names stay ids until Users answers */ }
}
const org = () => document.title.split(" · ")[0];
// A person's status as the app's pills draw it.
const pill = st => `<span class="chip ${({ active: "ok", disabled: "bad", invited: "warn" })[st] || ""}">${esc(st)}</span>`;

// ---- The patterns every screen uses ----------------------------------

const UI = {
  loading(el, what) {
    el.innerHTML = `<div class="ad-state" aria-busy="true"><span class="ad-spin" aria-hidden="true"></span><span>Loading ${esc(what)}…</span></div>`;
  },
  empty(text, hint) {
    return `<div class="ad-state ad-empty"><p>${esc(text)}</p>${hint ? `<p class="small muted">${esc(hint)}</p>` : ""}</div>`;
  },
  // An error names what could not be done and why, with Try again.
  error(el, what, err, retry) {
    el.innerHTML = `<div class="ad-state ad-error" role="alert"><p><strong>${esc(what)}</strong></p><p class="small">${esc(err?.message || err)}</p>${retry ? `<button class="ad-retry">Try again</button>` : ""}</div>`;
    if (retry) $(".ad-retry", el).onclick = retry;
  },

  // A toast says what was done; with `undo`, it offers Undo for ten
  // seconds.
  toast(text, undo) {
    let host = $("#toasts");
    if (!host) { host = document.createElement("div"); host.id = "toasts"; host.setAttribute("role", "status"); host.setAttribute("aria-live", "polite"); document.body.append(host); }
    const t = document.createElement("div");
    t.className = "toast ad-toast";
    t.innerHTML = `<span>${esc(text)}</span>${undo ? `<button class="link ad-undo">Undo</button>` : ""}`;
    host.append(t);
    const gone = setTimeout(() => t.remove(), undo ? 10000 : 5000);
    if (undo) $(".ad-undo", t).onclick = async () => { clearTimeout(gone); t.remove(); try { await undo(); } catch (e) { UI.toast(`Could not undo: ${e.message}`); } };
  },

  // The confirm sheet, for what cannot be undone. `typed` asks for the
  // target's name typed before the action is offered.
  confirm({ title, body, verb = "Confirm", danger = false, typed = null }) {
    return new Promise(done => {
      const d = document.createElement("dialog");
      d.className = "ad-sheet";
      d.setAttribute("aria-labelledby", "sheet-title");
      d.innerHTML = `<form method="dialog">
        <h2 id="sheet-title">${esc(title)}</h2>
        ${body ? `<p>${esc(body)}</p>` : ""}
        ${typed ? `<label>Type <strong class="mono">${esc(typed)}</strong> to confirm <input name="typed" autocomplete="off" spellcheck="false"></label>` : ""}
        <div class="row ad-sheet-actions"><button value="no" formnovalidate>Cancel</button><button value="yes" class="${danger ? "danger" : "primary"}" ${typed ? "disabled" : ""}>${esc(verb)}</button></div>
      </form>`;
      document.body.append(d);
      const yes = $('button[value="yes"]', d);
      if (typed) $("input", d).oninput = e => { yes.disabled = e.target.value.trim() !== typed; };
      d.addEventListener("close", () => { done(d.returnValue === "yes"); d.remove(); });
      d.showModal();
    });
  },

  // The detail panel: on the right of its list, full screen at phone
  // width. It closes with its button or Escape, and goes back to `back`.
  panel(title, html, back) {
    UI.closePanel();
    const main = $("#main");
    const p = document.createElement("aside");
    p.id = "panel";
    p.className = "ad-panel";
    p.setAttribute("aria-label", title);
    p.innerHTML = `<div class="ad-panel-head"><h2>${esc(title)}</h2><button class="ad-close" aria-label="Close">×</button></div><div class="ad-panel-body">${html}</div>`;
    main.classList.add("with-panel");
    main.append(p);
    const close = () => { if (back) location.hash = back; else UI.closePanel(); };
    $(".ad-close", p).onclick = close;
    p.addEventListener("keydown", e => { if (e.key === "Escape") close(); });
    $(".ad-close", p).focus();
    return $(".ad-panel-body", p);
  },
  closePanel() {
    $("#panel")?.remove();
    $("#main").classList.remove("with-panel");
  },

  // The step-up sheet: the console's own sign-in in its own small window;
  // the action goes on once the console has the fresh password. Where
  // windows are blocked, the whole page goes and comes back.
  stepUp() {
    return new Promise(done => {
      const d = document.createElement("dialog");
      d.className = "ad-sheet";
      d.setAttribute("aria-labelledby", "step-title");
      d.innerHTML = `<form method="dialog"><h2 id="step-title">Confirm it's you</h2>
        <p>This needs your password again, typed in the last five minutes. It opens ${esc(org())}'s sign-in in a small window and comes back here.</p>
        <p class="small muted" id="step-wait" hidden>Waiting for you to sign in in the other window. Closed it by mistake? Choose Open it again.</p>
        <div class="row ad-sheet-actions"><button value="no" formnovalidate>Cancel</button><button value="go" class="primary" id="step-go">Continue</button></div></form>`;
      document.body.append(d);
      // The window is followed through the session, not its handle.
      let w = null, timer = null, ok = false;
      const finish = v => { ok = v; clearInterval(timer); d.close(); };
      $("#step-go", d).onclick = e => {
        e.preventDefault();
        w = window.open("/auth/local/step-up?then=/auth/done", "magnus-step-up", "popup,width=520,height=680");
        if (!w) { location.href = `/auth/local/step-up?then=${encodeURIComponent(location.pathname + location.hash)}`; return; }
        $("#step-wait", d).hidden = false;
        $("#step-go", d).textContent = "Open it again";
        clearInterval(timer);
        timer = setInterval(async () => {
          const r = await fetch("/api/v1/session", { credentials: "same-origin" }).catch(() => null);
          const v = r?.ok ? await r.json() : null;
          if (v?.stepped_up_until && new Date(v.stepped_up_until) > new Date()) { Admin.me = v; try { w.close(); } catch { /* it closes itself */ } finish(true); }
        }, 1000);
      };
      d.addEventListener("close", () => { clearInterval(timer); d.remove(); done(ok); });
      d.showModal();
    });
  },
};

// Every change carries the session's CSRF token. A step-up asked for opens
// the sheet, and the same request is sent once more after it.
async function api(method, path, body, again = true) {
  const r = await fetch(`/api/v1${path}`, {
    method, credentials: "same-origin",
    headers: { ...(body ? { "content-type": "application/json" } : {}), ...(Admin.me ? { "x-csrf-token": Admin.me.csrf } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  if (r.status === 401) {
    const v = await r.json().catch(() => ({}));
    if (v.error === "step_up_required" && again) {
      if (!(await UI.stepUp())) throw new Error("Not confirmed: nothing was changed.");
      return api(method, path, body, false);
    }
    if (v.error !== "step_up_required") signedOut();
    throw new Error(v.message || "Sign in again.");
  }
  if (!r.ok) throw new Error((await r.json().catch(() => ({}))).message || r.statusText);
  return r.status === 204 ? null : r.json();
}

// An action from a button: its error as a toast, its success as `then`.
async function act(method, path, body, then) {
  try { const v = await api(method, path, body); await then?.(v); return v; } catch (e) { UI.toast(e.message); return undefined; }
}

function signedOut() {
  Admin.me = null;
  Admin.live?.close();
  Admin.live = null;
  document.body.classList.remove("signed-in");
  $("#nav").hidden = true;
  $("#kill").hidden = true;
  $("#kill-banner")?.remove();
  $("#me").innerHTML = "";
  $("#view")?.remove();
  UI.closePanel();
  $("#sign-in").hidden = false;
}

// ---- Status, live ------------------------------------------------------

function drawStatus(ok, text) {
  const el = $("#status");
  el.dataset.state = ok ? "ok" : "down";
  $(".small", el).textContent = text;
}

async function status() {
  if (Admin.live) return;
  try {
    const v = await (await fetch("/status", { credentials: "same-origin" })).json();
    drawStatus(v.ok, v.ok ? "Running" : "Not answering");
  } catch { drawStatus(false, "Not answering"); }
}

// The live stream: the status dot, the kill switch and whatever screen
// listens (Overview), as they change.
function live() {
  Admin.live?.close();
  const es = new EventSource("/api/v1/events", { withCredentials: true });
  Admin.live = es;
  es.addEventListener("status", e => {
    const v = JSON.parse(e.data);
    Admin.status = v;
    const ok = v.kernel_up && v.log_up;
    drawStatus(ok, ok ? "Running" : !v.kernel_up ? "Kernel not answering" : "Event log not answering");
    if (v.kill_switch) drawKill(v.kill_switch);
    document.dispatchEvent(new CustomEvent("ad-status", { detail: v }));
  });
  es.onerror = () => {
    if (es.readyState !== EventSource.CLOSED) return;
    // Refused (the session ended) or the stream's time is up: back
    // through the guard, or to the sign-in.
    Admin.live = null;
    setTimeout(async () => {
      const r = await fetch("/api/v1/session", { credentials: "same-origin" }).catch(() => null);
      if (r?.ok) live(); else signedOut();
    }, 2000);
  };
}

// ---- The kill switch, on every screen ---------------------------------
//
// "Stop all agents", pressed and held for a second; while it is on, a red
// banner says who engaged it and when, with Release (which needs a fresh
// password).

function drawKill(v) {
  const el = $("#kill");
  el.hidden = false;
  let banner = $("#kill-banner");
  if (v.engaged) {
    if (!banner) { banner = document.createElement("div"); banner.id = "kill-banner"; banner.setAttribute("role", "alert"); document.body.prepend(banner); }
    banner.innerHTML = `<strong>All agents are stopped.</strong> <span>Engaged by ${esc(who(v.by))}${v.at ? ` at ${esc(when(v.at))}` : ""}.</span> <button id="kill-release">Release</button>`;
    $("#kill-release").onclick = () => act("POST", "/kill-switch/release", null, async () => { UI.toast("Agents may run again."); drawKill(await api("GET", "/kill-switch")); });
    el.innerHTML = `<span class="small ad-kill-on">Stopped</span>`;
    return;
  }
  banner?.remove();
  if ($("#kill-hold", el)) return;
  el.innerHTML = `<button id="kill-hold" class="ad-kill-btn" aria-describedby="kill-how">Stop all agents</button><span id="kill-how" class="vh">Press and hold for one second</span>`;
  const b = $("#kill-hold");
  let t = null;
  const start = e => {
    if (t) return;
    e.preventDefault();
    b.classList.add("holding");
    t = setTimeout(async () => {
      t = null;
      b.classList.remove("holding");
      const v = await act("POST", "/kill-switch/engage");
      el.innerHTML = "";
      drawKill(v?.engaged !== undefined ? v : await api("GET", "/kill-switch"));
    }, 1000);
  };
  const stop = () => { clearTimeout(t); t = null; b.classList.remove("holding"); };
  b.addEventListener("pointerdown", start);
  for (const ev of ["pointerup", "pointerleave", "pointercancel"]) b.addEventListener(ev, stop);
  b.addEventListener("keydown", e => { if ((e.key === " " || e.key === "Enter") && !e.repeat) start(e); });
  b.addEventListener("keyup", e => { if (e.key === " " || e.key === "Enter") stop(); });
}

async function killSwitch() {
  try { const v = await api("GET", "/kill-switch"); if (v) drawKill(v); } catch { /* the live stream redraws it */ }
}

// ---- The admin's menu: theme and sign out -----------------------------

function menu() {
  const m = $("#me");
  m.innerHTML = `<button class="ad-me-btn" id="me-btn" aria-haspopup="true" aria-expanded="false">${esc(Admin.me.name)}</button>
    <div class="ad-menu card" id="me-menu" role="menu" hidden>
      <div class="small muted">Theme</div>
      <div class="ad-seg" role="group" aria-label="Light or dark">${Theme.MODES.map(([k, l]) => `<button role="menuitemradio" data-mode="${k}">${esc(l)}</button>`).join("")}</div>
      <div class="ad-seg" role="group" aria-label="Material">${Theme.MATERIALS.map(([k, l]) => `<button role="menuitemradio" data-material="${k}">${esc(l)}</button>`).join("")}</div>
      <hr><button class="link" role="menuitem" id="sign-out">Sign out</button>
    </div>`;
  const btn = $("#me-btn"), box = $("#me-menu");
  const mark = () => {
    const p = Theme.prefs();
    for (const b of $$("[data-mode]", box)) b.setAttribute("aria-checked", String(b.dataset.mode === p.mode));
    for (const b of $$("[data-material]", box)) b.setAttribute("aria-checked", String(b.dataset.material === p.material));
  };
  const open = v => { box.hidden = !v; btn.setAttribute("aria-expanded", String(v)); if (v) mark(); };
  btn.onclick = e => { e.stopPropagation(); open(box.hidden); };
  document.addEventListener("click", e => { if (!m.contains(e.target)) open(false); });
  document.addEventListener("keydown", e => { if (e.key === "Escape" && !box.hidden) { open(false); btn.focus(); } });
  for (const b of $$("[data-mode]", box)) b.onclick = () => { Theme.set({ mode: b.dataset.mode }); mark(); };
  for (const b of $$("[data-material]", box)) b.onclick = () => { Theme.set({ material: b.dataset.material }); mark(); };
  $("#sign-out").onclick = async () => { try { await api("DELETE", "/session"); } catch { /* signed out either way */ } signedOut(); };
}

// ---- Overview ----------------------------------------------------------

async function overviewView(view) {
  view.innerHTML = `<h1>Overview</h1><div id="ov"></div>`;
  const box = $("#ov", view);
  UI.loading(box, "the overview");
  let v;
  try { v = await api("GET", "/overview"); } catch (e) { return UI.error(box, "The overview could not be read.", e, () => overviewView(view)); }
  const strip = s => `<p class="small ad-strip"><span><span class="ad-dot ${s.healthy ? "ok" : "down"}"></span> ${s.healthy ? "Everything is answering" : "Something needs you"}</span>
      <span>Kill switch ${s.kill_switch?.engaged ?? s.kill_switch ? "<strong>on</strong>" : "off"}</span><span>Magnus ${esc(s.version || "")}</span></p>`;
  const cards = c => `
      <a class="card ad-card" href="#/operations/health"><div class="small muted">Tasks running</div><div class="ad-big">${esc(c.running_tasks ?? "–")}</div><div class="small muted">${esc(c.waiting_tasks ?? 0)} waiting</div></a>
      <a class="card ad-card" href="#/users"><div class="small muted">People signed in</div><div class="ad-big">${esc(c.signed_in ?? "–")}</div></a>
      <a class="card ad-card" href="#/knowledge/activity"><div class="small muted">Agent knowledge changes, last 24 hours</div><div class="ad-big">${esc(c.knowledge_changes_24h ?? "–")}</div><div class="small muted">Open Activity</div></a>`;
  box.innerHTML = `<div id="ov-strip">${strip(v.status)}</div>
    <div class="ad-cards" id="ov-cards">${cards(v.cards)}</div>
    <h2>Alerts</h2>${v.alerts.length ? `<ul class="ad-alerts">${v.alerts.map(a => `<li class="card flat"><span>${esc(a.what)}</span> <a href="${esc(a.fix)}">Fix</a></li>`).join("")}</ul>` : UI.empty("No alerts.", "Services, disk and certificates are fine.")}`;
  // Live: the strip and the cards as they change.
  const on = e => {
    if (!document.body.contains(box)) return document.removeEventListener("ad-status", on);
    const s = e.detail;
    $("#ov-cards", box).innerHTML = cards(s);
    $("#ov-strip", box).innerHTML = strip({ healthy: v.status.healthy && s.kernel_up && s.log_up, kill_switch: s.kill_switch, version: s.version });
  };
  document.addEventListener("ad-status", on);
}

// ---- Operations --------------------------------------------------------

const OPS_TABS = [["kill-switch", "Kill switch"], ["modules", "Modules"], ["providers", "Model providers"], ["health", "Health"], ["updates", "Updates"], ["network", "Network"]];

function operationsView(view, rest) {
  const tab = OPS_TABS.some(([k]) => k === rest[0]) ? rest[0] : "kill-switch";
  if (!$("#ops-tabs", view)) view.innerHTML = `<h1>Operations</h1><nav class="ad-tabs" id="ops-tabs" aria-label="Operations">${OPS_TABS.map(([k, l]) => `<a href="#/operations/${k}" data-tab="${k}">${l}</a>`).join("")}</nav><div id="ops"></div>`;
  for (const a of $$("#ops-tabs a", view)) { if (a.dataset.tab === tab) a.setAttribute("aria-current", "page"); else a.removeAttribute("aria-current"); }
  const box = $("#ops", view);
  box.innerHTML = "";
  UI.closePanel();
  return ({ "kill-switch": opsKill, modules: opsModules, providers: opsProviders, health: opsHealth, updates: opsUpdates, network: opsNetwork })[tab](box);
}

// Operations › Kill switch: its state, who moved it and when, and exactly
// what it stopped. Release needs a fresh password.
async function opsKill(box) {
  UI.loading(box, "the kill switch");
  let k;
  try { k = await api("GET", "/kill-switch"); } catch (e) { return UI.error(box, "The kill switch could not be read.", e, () => opsKill(box)); }
  box.innerHTML = `<p>${k.engaged ? `<span class="chip bad">On</span> Engaged by ${esc(who(k.by))}${k.at ? ` at ${esc(when(k.at))}` : ""}.` : `<span class="chip ok">Off</span>${k.by ? ` Last moved by ${esc(who(k.by))}${k.at ? ` at ${esc(when(k.at))}` : ""}.` : ""}`}</p>
    <dl class="kv small"><dt>Tasks halted</dt><dd>${esc(k.tasks_halted)}</dd><dt>Schedules held</dt><dd>${esc(k.schedules_held)}</dd><dt>MicroVMs frozen</dt><dd>${esc(k.microvms_frozen ?? 0)} <span class="muted">(stopped, their disks kept)</span></dd><dt>MicroVMs running</dt><dd>${esc(k.microvms_running ?? 0)}</dd><dt>Sign-ins revoked</dt><dd>${esc(k.sign_ins_revoked)}</dd></dl>
    ${k.engaged ? `<div class="row ad-bar"><button class="primary" id="ops-release">Release</button></div><p class="small muted">Release asks for your password again. Nothing stopped goes on by itself: each person is told, and continues their own.</p>`
      : `<p class="small muted">"Stop all agents" in the top bar engages it at once, from any screen. It halts every task, holds every schedule, freezes every microVM, seals the vault and revokes every sign-in to apps.</p>`}`;
  const r = $("#ops-release", box);
  if (r) r.onclick = () => act("POST", "/kill-switch/release", null, async () => { UI.toast("Agents may run again."); drawKill(await api("GET", "/kill-switch")); opsKill(box); });
}

// Operations › Modules: Magnus's run modules, what each provides, on or off
// for the organisation.
async function opsModules(box) {
  UI.loading(box, "the modules");
  let v;
  try { v = await api("GET", "/modules"); } catch (e) { return UI.error(box, "The modules could not be read.", e, () => opsModules(box)); }
  box.innerHTML = `<p class="small muted">Modules help a model where it is weak. They come with Magnus's release, so a new one arrives with an update; here each is turned on or off for every task that starts from now. Tasks already running keep theirs.</p>
    <div class="ad-scroll"><table class="t small"><thead><tr><th>Module</th><th>Version</th><th>Status</th><th>What it provides</th><th class="n">Acted</th><th></th></tr></thead><tbody>${v.modules.map(m => `<tr>
      <td class="mono">${esc(m.name)}</td><td class="mono">${esc(m.version)}</td>
      <td><span class="chip ${m.on ? "ok" : ""}">${m.on ? "On" : "Off"}</span>${m.set_by_admin ? ` <span class="small muted">set here</span>` : ""}</td>
      <td>For ${esc(m.provides)}${m.escalate_to ? ` (to ${esc(m.escalate_to)})` : ""}</td><td class="n">${esc(m.acted)}</td>
      <td><button class="link small" data-mod="${esc(m.name)}" data-on="${m.on ? "false" : "true"}">${m.on ? "Turn off" : "Turn on"}</button>${m.set_by_admin ? ` <button class="link small" data-mod="${esc(m.name)}" data-on="null">As configured</button>` : ""}</td></tr>`).join("")}</tbody></table></div>`;
  for (const b of $$("[data-mod]", box)) b.onclick = () => {
    const on = JSON.parse(b.dataset.on), was = v.modules.find(m => m.name === b.dataset.mod);
    act("PATCH", `/modules/${b.dataset.mod}`, { on }, () => {
      UI.toast(on === null ? `${b.dataset.mod} is as configured.` : `${b.dataset.mod} is ${on ? "on" : "off"} for new tasks.`, () => act("PATCH", `/modules/${b.dataset.mod}`, { on: was.set_by_admin ? was.on : null }, () => opsModules(box)));
      opsModules(box);
    });
  };
}

// Operations › Model providers: the appliance's own, never anyone's
// credentials: enabled, where they answer, their models, a connection
// test, the last error.
async function opsProviders(box) {
  UI.loading(box, "the model providers");
  let v;
  try { v = await api("GET", "/providers"); } catch (e) { return UI.error(box, "The model providers could not be read.", e, () => opsProviders(box)); }
  box.innerHTML = v.providers.length ? v.providers.map(p => `<section class="card" data-p="${esc(p.id)}">
      <div class="spread"><h2>${esc(p.id)} ${p.default ? `<span class="chip sch">Default</span>` : ""}</h2>
        <span><span class="chip ${!p.enabled ? "" : p.answers ? "ok" : "bad"}">${!p.enabled ? "Disabled" : p.answers ? "Answering" : "Not answering"}</span></span></div>
      <dl class="kv small"><dt>Program</dt><dd class="mono">${esc(p.program || "")}${p.running ? ` (${esc(p.running.adapter)} ${esc(p.running.version)})` : ""}</dd>
        <dt>Endpoint</dt><dd class="mono ad-wrap">${esc(p.endpoint || "the provider's own")}</dd>
        <dt>Models</dt><dd>${p.models ? (p.models.length ? p.models.map(m => `<span class="mono">${esc(m)}</span>`).join(", ") : "none listed") : `<span class="error">${esc(p.error || "")}</span>`}</dd>
        ${p.default ? `<dt>New tasks run on</dt><dd class="mono">${esc(p.default_model)}</dd>` : ""}
        <dt>Last checked</dt><dd>${p.conformance ? `${p.conformance.passed ? "passed" : "failed"} the conformance suite on ${esc(p.conformance.model)}` : "never"}</dd>
        <dt>Last error</dt><dd>${p.last_error ? `${esc(p.last_error.error)} <span class="muted">${esc(when(p.last_error.at))}</span>` : "none"}</dd></dl>
      <div class="row ad-bar"><button data-test="${esc(p.id)}">Test the connection</button>
        ${p.enabled ? `<button class="danger" data-off="${esc(p.id)}">Disable</button>` : `<button data-on="${esc(p.id)}">Enable</button>`}
        ${p.models && p.models.length ? `<label class="row small">New tasks run on <select data-default="${esc(p.id)}"><option value="">—</option>${p.models.map(m => `<option ${p.default && m === p.default_model ? "selected" : ""}>${esc(m)}</option>`).join("")}</select></label>` : ""}</div>
      <p class="small" data-result="${esc(p.id)}" role="status"></p></section>`).join("")
    : UI.empty("No model provider is set up.", "Providers are the appliance's adapters, set in its config.");
  for (const b of $$("[data-test]", box)) b.onclick = async () => {
    const out = $(`[data-result="${b.dataset.test}"]`, box);
    out.textContent = "Asking it for its models…";
    try { const r = await api("POST", `/providers/${b.dataset.test}/test`); out.innerHTML = r.ok ? `<span class="chip ok">Answered</span> in ${esc(r.ms)} ms with ${esc(r.models.length)} models.` : `<span class="chip bad">No answer</span> ${esc(r.error)} (${esc(r.ms)} ms)`; }
    catch (e) { out.textContent = e.message; }
  };
  for (const b of $$("[data-off]", box)) b.onclick = () => act("PATCH", `/providers/${b.dataset.off}`, { enabled: false }, () => { UI.toast(`${b.dataset.off} is disabled: its models are offered to no one.`, () => act("PATCH", `/providers/${b.dataset.off}`, { enabled: true }, () => opsProviders(box))); opsProviders(box); });
  for (const b of $$("[data-on]", box)) b.onclick = () => act("PATCH", `/providers/${b.dataset.on}`, { enabled: true }, () => { UI.toast(`${b.dataset.on} is enabled.`); opsProviders(box); });
  for (const sel of $$("[data-default]", box)) sel.onchange = () => sel.value && act("PATCH", `/providers/${sel.dataset.default}`, { default_model: sel.value }, () => { UI.toast(`New tasks run on ${sel.value}.`); opsProviders(box); });
}

// Operations › Health: each service, the microVMs, the machine and recent
// errors, kept live by the stream.
async function opsHealth(box) {
  UI.loading(box, "the appliance's health");
  let h;
  try { h = await api("GET", "/health"); } catch (e) { return UI.error(box, "The appliance's health could not be read.", e, () => opsHealth(box)); }
  box.innerHTML = `<ul class="ad-services">${h.services.map(s => `<li><span class="ad-dot ${s.up === null ? "" : s.up ? "ok" : "down"}"></span> ${esc(s.name)} <span class="small muted">${s.up === null ? esc(s.note || "not set up") : s.up ? "answering" : "not answering"}</span></li>`).join("")}</ul>
    <dl class="kv small" id="ops-live"><dt>Tasks running</dt><dd>${esc(h.running_tasks ?? "–")}</dd><dt>MicroVMs running</dt><dd>${esc(h.microvms_running ?? 0)}</dd><dt>Load</dt><dd>${esc(h.host.load ?? "–")} on ${esc(h.host.cpus)} processors</dd><dt>Memory used</dt><dd>${pct(h.host.memory_used)}</dd><dt>Disk used</dt><dd>${pct(h.host.disk_used)}</dd></dl>
    <h3>Recent errors</h3>${(h.recent_errors || []).length ? `<ul class="small">${h.recent_errors.map(e => `<li>${esc(e.reason || "failed")} <span class="muted">· ${esc(e.person)} · ${esc(when(e.at))}</span></li>`).join("")}</ul>` : UI.empty("No recent errors.")}`;
  const on = e => {
    if (!document.body.contains(box)) return document.removeEventListener("ad-status", on);
    const l = $("#ops-live", box);
    if (l) { l.children[1].textContent = e.detail.running_tasks ?? "–"; l.children[3].textContent = e.detail.microvms_running ?? 0; }
  };
  document.addEventListener("ad-status", on);
}

// Operations › Updates: this version and any available, with notes;
// installing and going back need a fresh password. The desktop app's
// channels, each with its published version.
async function opsUpdates(box) {
  UI.loading(box, "the updates");
  let u;
  try { u = await api("GET", "/updates"); } catch (e) { return UI.error(box, "The updates could not be read.", e, () => opsUpdates(box)); }
  box.innerHTML = `<dl class="kv small"><dt>Running</dt><dd class="mono">${esc(u.version)}</dd><dt>Latest release</dt><dd class="mono">${esc(u.latest?.version ?? "none found")}</dd>
      <dt>Installs by itself</dt><dd>${u.automatic ? "yes, with rollback if it fails its checks" : "no"}</dd>${u.previous ? `<dt>Kept to go back to</dt><dd class="mono">${esc(u.previous)}</dd>` : ""}
      ${u.updating ? `<dt>Now</dt><dd>${u.updating.rollback ? "going back" : "installing"} ${esc(u.updating.from)} → ${esc(u.updating.to)}</dd>` : ""}
      ${u.last ? `<dt>Last</dt><dd>${esc(u.last.what)} ${esc(u.last.to)} ${u.last.detail ? `— ${esc(u.last.detail)}` : ""} <span class="muted">${esc(when(u.last.at))}</span></dd>` : ""}</dl>
    ${u.available ? `<div class="card flat"><h3>${esc(u.latest.version)} is available</h3>${u.latest.notes ? `<p class="small ad-wrap">${esc(u.latest.notes)}</p>` : ""}<button class="primary" id="up-install">Install now…</button></div>` : `<p class="small"><span class="chip ok">Up to date</span></p>`}
    ${u.previous ? `<div class="row ad-bar"><button id="up-back">Go back to ${esc(u.previous)}…</button></div>` : ""}
    <h3>The desktop app</h3>
    <table class="t small"><thead><tr><th>Channel</th><th>Published</th><th>Who gets it</th></tr></thead><tbody>${u.desktop.channels.map(c => `<tr><td>${esc(c.channel)}</td><td class="mono">${esc(c.published || "none")}</td><td>${esc(c.channel === u.desktop.everyone && c.channel !== "production" ? `${c.who}, and everyone else` : c.who === "everyone else" && u.desktop.everyone !== "production" ? "no one" : c.who)}</td></tr>`).join("")}</tbody></table>
    <label class="row small">Everyone but the owner and testers follows <select id="up-channel">${["production", "test"].map(c => `<option ${c === u.desktop.everyone ? "selected" : ""}>${c}</option>`).join("")}</select></label>`;
  const inst = $("#up-install", box);
  if (inst) inst.onclick = async () => {
    if (await UI.confirm({ title: `Install ${u.latest.version} now?`, body: "Magnus restarts on it; running tasks go on afterwards. If it fails its checks, it goes back by itself. It asks for your password again.", verb: "Install" }))
      act("POST", "/updates/install", null, r => { UI.toast(r?.status === "installing" ? `Installing ${r.latest}.` : "Already up to date."); opsUpdates(box); });
  };
  const back = $("#up-back", box);
  if (back) back.onclick = async () => {
    if (await UI.confirm({ title: `Go back to ${u.previous}?`, body: "Magnus restarts on the release before this one, and does not install this one again by itself. It asks for your password again.", verb: "Go back", danger: true }))
      act("POST", "/updates/rollback", null, () => { UI.toast(`Going back to ${u.previous}.`); opsUpdates(box); });
  };
  $("#up-channel", box).onchange = e => { const was = u.desktop.everyone; act("PATCH", "/updates/desktop-channel", { channel: e.target.value }, () => { UI.toast(`Everyone's app follows ${e.target.value}.`, () => act("PATCH", "/updates/desktop-channel", { channel: was }, () => opsUpdates(box))); opsUpdates(box); }); };
}

// Operations › Network: the tunnel and its public names, the console's own
// name (never public), the subnets it takes connections from, and the
// certificates. A change to the subnets needs a fresh password, and one
// that would shut out your own address is refused.
async function opsNetwork(box) {
  UI.loading(box, "the network");
  let n;
  try { n = await api("GET", "/network"); } catch (e) { return UI.error(box, "The network could not be read.", e, () => opsNetwork(box)); }
  box.innerHTML = `<h3>Tunnel</h3>
    <p class="small">${!n.tunnel.configured ? "No tunnel is set up on this appliance." : n.tunnel.connected ? `<span class="chip ok">Connected</span>` : `<span class="chip bad">Not connected</span> People outside the organisation's network cannot reach Magnus.`}</p>
    ${n.tunnel.public_hostnames.length ? `<p class="small">Its public names: ${n.tunnel.public_hostnames.map(h => `<span class="mono">${esc(h)}</span>`).join(", ")}. The console is never one of them.</p>` : ""}
    <h3>The console</h3>
    <dl class="kv small"><dt>Name</dt><dd class="mono">${esc(n.admin.hostname)}</dd>${n.admin.aliases.length ? `<dt>Also</dt><dd class="mono">${n.admin.aliases.map(esc).join(", ")}</dd>` : ""}<dt>Listens on</dt><dd class="mono">${n.admin.listen.map(esc).join(", ")}</dd><dt>Public</dt><dd>No: only the organisation's network and VPN reach it.</dd><dt>You are at</dt><dd class="mono">${esc(n.you)}</dd></dl>
    <form id="net-allow" class="ad-form ad-narrow"><label>It takes connections only from <textarea name="allow" rows="3" spellcheck="false" class="mono">${esc(n.allow.join("\n"))}</textarea></label>
      <p class="small muted">One subnet a line, such as 192.168.0.0/24, and the VPN's, such as 100.64.0.0/10. Never loopback. Saving asks for your password again.</p>
      <p class="error" id="net-error" hidden></p><div class="row"><button class="primary">Save</button></div></form>
    <h3>Certificates</h3>
    <p class="small" id="net-trusted">${!n.trusted.via ? "The console's certificate is the organisation's own, from its CA: computers that open the console trust that CA. Renew reads it again once the organisation has renewed it."
      : n.trusted.serving ? `<span class="chip ok">Trusted</span> Browsers opening <span class="mono">${esc(n.admin.hostname)}</span> get a certificate they already trust, issued through the tailnet (Let's Encrypt) and renewed before it ends. Its other names get the organisation's own.`
      : `<span class="chip bad">Not issued</span> Browsers get the organisation's own certificate until the tailnet issues the trusted one${n.trusted.error ? `: ${esc(n.trusted.error)}` : "."}`}</p>
    <table class="t small"><thead><tr><th>For</th><th>Ends</th><th></th></tr></thead><tbody>${n.certificates.map(c => `<tr><td>${esc(c.what)}</td><td>${c.not_after ? esc(when(c.not_after)) : c.what.includes("trusted") ? "not issued yet" : "unreadable"}</td><td>${c.days_left == null ? "" : c.days_left < 14 ? `<span class="chip bad">${esc(c.days_left)} days left</span>` : `<span class="chip ok">${esc(c.days_left)} days left</span>`}</td></tr>`).join("")}</tbody></table>
    <div class="row"><button id="net-renew">Renew</button></div>`;
  $("#net-renew", box).onclick = async e => {
    e.target.disabled = true;
    try { await api("POST", "/network/certificate/renew"); UI.toast("The certificates are renewed: the next connection gets them."); opsNetwork(box); }
    catch (err) { e.target.disabled = false; UI.toast(`The certificates could not be renewed: ${err.message}`); }
  };
  $("#net-allow", box).onsubmit = async e => {
    e.preventDefault();
    const allow = new FormData(e.target).get("allow").split(/[\n,]/).map(x => x.trim()).filter(Boolean);
    try { await api("PATCH", "/network/allowlist", { allow }); UI.toast("The allowed subnets are saved."); opsNetwork(box); }
    catch (err) { const el = $("#net-error", box); el.textContent = err.message; el.hidden = false; }
  };
}

// ---- Security › Audit --------------------------------------------------
//
// The full log as a table, filtered by actor, action, target and time, with
// CSV export; a row opens the entry with its before and after.

async function auditView(view, seq) {
  if (!$("#audit-filter", view)) {
    view.innerHTML = `<form class="row" id="audit-filter" role="search">
        <label>Who <input name="actor" autocomplete="off"></label>
        <label>Action <input name="action" autocomplete="off"></label>
        <label>On <input name="target" autocomplete="off"></label>
        <label>From <input name="since" type="datetime-local"></label>
        <label>To <input name="until" type="datetime-local"></label>
        <button class="primary">Filter</button>
        <a class="button" id="audit-csv" href="/api/v1/audit.csv" download>Export CSV</a>
      </form>
      <div id="audit-rows"></div>`;
    $("#audit-filter", view).onsubmit = e => { e.preventDefault(); drawAudit(view); };
    await drawAudit(view);
  }
  const e = (view.audit || []).find(x => String(x.seq) === seq);
  if (!e) return UI.closePanel();
  UI.panel(e.action, `<dl class="kv small"><dt>When</dt><dd>${esc(when(e.at))}</dd><dt>Who</dt><dd>${esc(e.name || e.admin)} <span class="muted mono">${esc(e.admin)}</span></dd>
    <dt>On</dt><dd>${esc(e.target || "–")}</dd><dt>From</dt><dd class="mono">${esc(e.source || "–")}</dd></dl>
    ${e.before != null ? `<h3>Before</h3><pre class="ad-json">${esc(JSON.stringify(e.before, null, 2))}</pre>` : ""}
    ${e.after != null ? `<h3>After</h3><pre class="ad-json">${esc(JSON.stringify(e.after, null, 2))}</pre>` : ""}`, "#/security/audit");
}

async function drawAudit(view) {
  const form = $("#audit-filter", view), box = $("#audit-rows", view);
  const q = new URLSearchParams();
  for (const [k, v] of new FormData(form)) if (v) q.set(k, k === "since" || k === "until" ? new Date(v).toISOString() : v);
  const qs = q.toString();
  $("#audit-csv", view).href = `/api/v1/audit.csv${qs ? `?${qs}` : ""}`;
  UI.loading(box, "the audit log");
  try {
    const v = await api("GET", `/audit${qs ? `?${qs}` : ""}`);
    view.audit = v.entries;
    box.innerHTML = v.entries.length
      ? `<div class="ad-scroll"><table class="t small"><thead><tr><th>When</th><th>Who</th><th>What</th><th>On</th><th>From</th></tr></thead><tbody>${v.entries.map(e => `<tr class="click" data-href="#/security/audit/${esc(e.seq)}" tabindex="0">
          <td>${esc(when(e.at))}</td><td>${esc(e.name || e.admin)}</td><td>${esc(e.action)}</td><td>${esc(e.target || "")}</td><td class="mono">${esc(e.source || "")}</td></tr>`).join("")}</tbody></table></div>`
      : UI.empty("Nothing in the audit log matches.", qs ? "Clear a filter to see more." : null);
    rowLinks(box);
  } catch (e) { UI.error(box, "The audit log could not be read.", e, () => drawAudit(view)); }
}

// Rows that open their detail: by click, or Enter.
function rowLinks(el) {
  for (const r of $$("tr[data-href]", el)) {
    r.onclick = e => { if (!e.target.closest("button, a, input")) location.hash = r.dataset.href; };
    r.onkeydown = e => { if (e.key === "Enter") location.hash = r.dataset.href; };
  }
}

// ---- Security ----------------------------------------------------------
//
// Its four parts as tabs: Sign-in, Agent policy, Connectors and
// credentials, Audit.

const SECURITY_TABS = [["sign-in", "Sign-in"], ["policy", "Agent policy"], ["connectors", "Connectors and credentials"], ["audit", "Audit"]];

function securityView(view, rest) {
  const tab = SECURITY_TABS.some(([k]) => k === rest[0]) ? rest[0] : "connectors";
  if (!$("#sec-tabs", view)) {
    view.innerHTML = `<h1>Security</h1><nav class="ad-tabs" id="sec-tabs" aria-label="Security">${SECURITY_TABS.map(([k, l]) => `<a href="#/security/${k}" data-tab="${k}">${l}</a>`).join("")}</nav><div id="sec"></div>`;
  }
  for (const a of $$("#sec-tabs a", view)) { if (a.dataset.tab === tab) a.setAttribute("aria-current", "page"); else a.removeAttribute("aria-current"); }
  const box = $("#sec", view);
  if (box.dataset.tab !== tab || tab === "policy" || tab === "sign-in") { box.dataset.tab = tab; box.innerHTML = ""; UI.closePanel(); }
  if (tab === "audit") return auditView(box, rest[1]);
  if (tab === "connectors") return connectorsView(box, rest[1]);
  if (tab === "sign-in") return signInView(box);
  if (tab === "policy") return policyView(box, rest[1]);
}

// Security › Sign-in: how members and admins sign in, how long sessions
// last, and when failures lock an account. Saving asks for a fresh
// password.
async function signInView(box) {
  UI.loading(box, "the sign-in rules");
  let v;
  try { v = await api("GET", "/security/sign-in"); } catch (e) { return UI.error(box, "The sign-in rules could not be read.", e, () => signInView(box)); }
  const n = (name, value, min, max, label, unit) => `<label>${esc(label)} <span class="row"><input name="${name}" type="number" min="${min}" max="${max}" value="${esc(value)}" required> <span class="small muted">${esc(unit)}</span></span></label>`;
  box.innerHTML = `<form id="si" class="ad-form ad-narrow">
    <h2>How members sign in</h2>
    <p class="small">With their password, typed in the app or on the sign-in page.</p>
    <fieldset class="ad-choice"><legend>A code from an authenticator app</legend>
      <label class="row"><input type="radio" name="code" value="optional" ${v.members.code === "optional" ? "checked" : ""}> Optional: whoever sets one up is asked for it</label>
      <label class="row"><input type="radio" name="code" value="required" ${v.members.code === "required" ? "checked" : ""}> Required: a password alone signs no one in</label>
    </fieldset>
    <h2>How admins sign in</h2>
    <p class="small">With their password, and their code as members do. This is fixed: the console has no other way in.</p>
    <h2>Sessions</h2>
    <div class="grid2">${n("m_idle", v.sessions.members.idle_minutes, 5, 1440, "Members: signed out after idle", "minutes")}${n("m_max", v.sessions.members.max_hours, 1, 720, "Members: longest session", "hours")}
      ${n("c_idle", v.sessions.console.idle_minutes, 5, 240, "Console: signed out after idle", "minutes")}${n("c_hours", v.sessions.console.session_hours, 1, 24, "Console: longest session", "hours")}</div>
    <h2>Lockout</h2>
    <label class="row"><input type="checkbox" name="lock" ${v.lockout.enabled ? "checked" : ""}> Lock an account after repeated failed sign-ins</label>
    <div class="grid2">${n("failures", v.lockout.failures, 1, 100, "Failures before a lockout", "tries")}${n("wait", Math.max(1, v.lockout.wait_minutes), 1, 1440, "First wait", "minutes")}${n("max_wait", Math.max(1, v.lockout.max_wait_minutes), 1, 10080, "Longest wait", "minutes")}</div>
    <label class="row"><input type="checkbox" name="permanent" ${v.lockout.permanent ? "checked" : ""}> Lock for good, until an admin turns them back on</label>
    <p class="error" id="si-error" hidden></p>
    <div class="row"><button class="primary">Save the sign-in rules</button></div>
    <p class="small muted">Saving asks for your password again. It changes the rules at ${esc(org())}'s sign-in for everyone.</p>
  </form>`;
  $("#si", box).onsubmit = async e => {
    e.preventDefault();
    const f = new FormData(e.target), num = k => Number(f.get(k));
    const body = {
      members: { code: f.get("code") },
      sessions: { members: { idle_minutes: num("m_idle"), max_hours: num("m_max") }, console: { idle_minutes: num("c_idle"), session_hours: num("c_hours") } },
      lockout: { enabled: f.get("lock") === "on", failures: num("failures"), wait_minutes: num("wait"), max_wait_minutes: num("max_wait"), permanent: f.get("permanent") === "on" },
    };
    try { await api("PATCH", "/security/sign-in", body); UI.toast("The sign-in rules are saved."); signInView(box); }
    catch (err) { const el = $("#si-error", box); el.textContent = err.message; el.hidden = false; }
  };
}

// Security › Agent policy: each Cedar policy set, edited as a draft
// (highlighted, checked against Magnus's schema as it is typed), compared
// with what is live, tried in the test panel, applied with a fresh
// password; every version kept to go back to.

const CEDAR = /(\/\/[^\n]*)|("(?:[^"\\]|\\.)*")|\b(permit|forbid|when|unless|in|has|like|is|if|then|else|true|false)\b|\b(principal|action|resource|context)\b|\b([A-Z][A-Za-z]*)(?=::)/g;
function highlight(text, problems) {
  const lines = new Set((problems || []).map(p => p.line));
  const html = esc(text).replace(/&quot;/g, '"').replace(CEDAR, (m, comment, str, kw, scope, type) =>
    comment ? `<span class="cx-c">${comment}</span>` : str ? `<span class="cx-s">${str}</span>` : kw ? `<span class="cx-k">${kw}</span>` : scope ? `<span class="cx-v">${scope}</span>` : `<span class="cx-t">${type}</span>`);
  return html.split("\n").map((l, i) => `<span class="${lines.has(i + 1) ? "cx-bad" : ""}">${l || " "}</span>`).join("\n");
}

// The lines that differ between two texts: each removed or added, with
// the unchanged around them.
function diff(a, b) {
  const x = (a || "").split("\n"), y = (b || "").split("\n");
  const m = x.length, n = y.length, L = Array.from({ length: m + 1 }, () => new Int32Array(n + 1));
  for (let i = m - 1; i >= 0; i--) for (let j = n - 1; j >= 0; j--) L[i][j] = x[i] === y[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
  const out = [];
  let i = 0, j = 0;
  while (i < m && j < n) {
    if (x[i] === y[j]) { out.push(["=", x[i]]); i++; j++; }
    else if (L[i + 1][j] >= L[i][j + 1]) out.push(["-", x[i++]]);
    else out.push(["+", y[j++]]);
  }
  while (i < m) out.push(["-", x[i++]]);
  while (j < n) out.push(["+", y[j++]]);
  return out;
}
const diffHtml = d => (d.some(([k]) => k !== "=") ? `<pre class="ad-diff">${d.map(([k, l]) => `<span class="d${k === "+" ? "a" : k === "-" ? "r" : "s"}">${k === "=" ? " " : k} ${esc(l)}</span>`).join("\n")}</pre>` : UI.empty("No changes from what is live."));

async function policyView(box, id) {
  UI.loading(box, "the agent policy");
  let v;
  try { v = await api("GET", "/policy/sets"); } catch (e) { return UI.error(box, "The agent policy could not be read.", e, () => policyView(box, id)); }
  const sets = v.sets;
  const cur = sets.find(x => x.id === id) || sets.find(x => x.id === "org");
  box.innerHTML = `<div class="ad-policy">
    <aside class="ad-sets"><h2 class="vh">Policy sets</h2><ul class="ad-list">${sets.map(x => `<li><a href="#/security/policy/${esc(x.id)}" ${x.id === cur.id ? 'aria-current="page"' : ""}>${esc(x.id === "org" ? "The organisation" : x.id.replace(/^group:/, "Group ").replace(/^user:/, "Person "))}</a>
      ${x.draft != null ? `<span class="chip warn">Draft</span>` : ""}${x.default ? `<span class="chip">Default</span>` : ""}<div class="small muted">${x.versions} version${x.versions === 1 ? "" : "s"}${x.changed_at ? ` · ${esc(when(x.changed_at))}` : ""}</div></li>`).join("")}</ul>
      <form id="pol-new" class="ad-form"><label>New set <input name="id" placeholder="group:sales or user:bob" autocomplete="off" pattern="(group|user):[A-Za-z0-9._-]+" required></label><button>Start a draft</button></form>
    </aside>
    <section class="ad-editor-box">
      <h2>${esc(cur.id === "org" ? "The organisation's policy" : cur.id)}</h2>
      <p class="small muted">${cur.id === "org" ? "Every action goes through it first." : "It can only narrow what the organisation's policy allows."} ${cur.draft != null ? "You are editing the draft; what is live stays in force until it is applied." : "Editing starts a draft; nothing changes until it is applied."}</p>
      <div class="ad-editor"><pre class="ad-hl" aria-hidden="true"></pre><textarea id="pol-text" spellcheck="false" aria-label="Policy text" autocapitalize="off" autocomplete="off"></textarea></div>
      <div id="pol-problems" class="small" role="status" aria-live="polite"></div>
      <div class="row ad-bar"><button id="pol-save">Save draft</button><button id="pol-diff">Compare with live</button><button class="primary" id="pol-apply">Apply…</button>${cur.draft != null ? `<button id="pol-drop" class="link">Discard draft</button>` : ""}</div>
      <div id="pol-out"></div>
      <h3>Test</h3>
      <form id="pol-test" class="ad-form grid2">
        <label>Principal <input name="principal" list="pol-people" required autocomplete="off"></label>
        <label>Action <select name="action"><option value="execute">execute (act without asking)</option><option value="request_approval">request_approval (ask first)</option></select></label>
        <label>Resource (tool) <input name="resource" list="pol-tools" required autocomplete="off" placeholder="app__tool"></label>
        <label>Its risk <select name="risk"><option value="">As the tool is classified</option>${["read", "sandbox", "reversible_write", "irreversible", "financial", "prohibited"].map(r => `<option>${r}</option>`).join("")}</select></label>
        <label class="row"><input type="checkbox" name="untrusted"> The run has read untrusted content</label>
        <label class="row"><input type="checkbox" name="private"> The run holds private data</label>
        <label>Against <select name="with"><option value="draft">This draft, with the rest live</option><option value="live">What is live</option></select></label>
        <div class="row"><button class="primary">Try it</button></div>
      </form>
      <datalist id="pol-people">${(v.people || []).map(p => `<option value="${esc(p.id)}">${esc(p.name)}</option>`).join("")}</datalist>
      <datalist id="pol-tools">${(v.tools || []).map(t => `<option value="${esc(t.name)}">${esc(t.risk)}</option>`).join("")}</datalist>
      <div id="pol-result"></div>
      <h3>History</h3><div id="pol-history"></div>
    </section></div>`;
  const ta = $("#pol-text", box), hl = $(".ad-hl", box), probs = $("#pol-problems", box), out = $("#pol-out", box);
  ta.value = cur.draft ?? cur.live ?? "";
  let problems = [], timer = null;
  const draw = () => { hl.innerHTML = highlight(ta.value, problems) + "\n"; hl.scrollTop = ta.scrollTop; hl.scrollLeft = ta.scrollLeft; };
  const check = async () => {
    try {
      const r = await api("POST", "/policy/validate", { cedar: ta.value });
      problems = r.problems;
      probs.innerHTML = r.ok ? `<span class="chip ok">Fits the schema</span>` : `<ul class="ad-problems">${r.problems.map(p => `<li><button class="link" data-at="${p.offset ?? 0}">Line ${p.line}, column ${p.column}</button>: ${esc(p.message)}</li>`).join("")}</ul>`;
      for (const b of $$("[data-at]", probs)) b.onclick = () => { ta.focus(); ta.setSelectionRange(Number(b.dataset.at), Number(b.dataset.at)); };
    } catch (e) { probs.textContent = e.message; }
    draw();
  };
  ta.oninput = () => { draw(); clearTimeout(timer); timer = setTimeout(check, 400); };
  ta.onscroll = () => { hl.scrollTop = ta.scrollTop; hl.scrollLeft = ta.scrollLeft; };
  ta.onkeydown = e => { if (e.key === "Tab") { e.preventDefault(); const p = ta.selectionStart; ta.setRangeText("  ", p, ta.selectionEnd, "end"); ta.oninput(); } };
  check();
  const reload = () => policyView(box, cur.id);
  $("#pol-save", box).onclick = () => act("PUT", `/policy/sets/${cur.id}/draft`, { cedar: ta.value }, () => { UI.toast("Draft saved. It is not in force until it is applied."); reload(); });
  $("#pol-diff", box).onclick = () => { out.innerHTML = `<h3>Changes from live</h3>${diffHtml(diff(cur.live, ta.value))}`; };
  const drop = $("#pol-drop", box);
  if (drop) drop.onclick = async () => {
    const was = cur.draft;
    act("DELETE", `/policy/sets/${cur.id}/draft`, null, () => { UI.toast("Draft discarded.", () => act("PUT", `/policy/sets/${cur.id}/draft`, { cedar: was }, reload)); reload(); });
  };
  $("#pol-apply", box).onclick = async () => {
    await check();
    if (problems.length) { UI.toast("Fix the problems before applying it."); return; }
    const d = diff(cur.live, ta.value);
    const changed = d.filter(([k]) => k !== "=").length;
    if (!changed) { UI.toast("Nothing to apply: it is what is live."); return; }
    out.innerHTML = `<h3>What changes</h3>${diffHtml(d)}`;
    if (!(await UI.confirm({ title: `Apply this policy to ${cur.id === "org" ? "the organisation" : cur.id}?`, body: `${changed} line${changed === 1 ? "" : "s"} change. It is in force at once, for every task's next action. It asks for your password again.`, verb: "Apply" }))) return;
    act("POST", `/policy/sets/${cur.id}/apply`, { cedar: ta.value }, v => { UI.toast(`Applied: version ${v?.version ?? ""} is in force.`); reload(); });
  };
  $("#pol-new", box).onsubmit = e => { e.preventDefault(); const nid = new FormData(e.target).get("id").trim(); act("PUT", `/policy/sets/${nid}/draft`, { cedar: "// What this set forbids, beyond the organisation's policy.\n" }, () => { location.hash = `#/security/policy/${nid}`; }); };
  $("#pol-test", box).onsubmit = async e => {
    e.preventDefault();
    const f = new FormData(e.target);
    const body = { principal: f.get("principal"), resource: f.get("resource"), risk: f.get("risk") || undefined, untrusted: f.get("untrusted") === "on", private: f.get("private") === "on" };
    if (f.get("with") === "draft") body.cedar = { [cur.layer]: ta.value };
    const res = $("#pol-result", box);
    try {
      const r = await api("POST", "/policy/test", body);
      const exec = r.decision === "allow", ask = r.decision !== "deny";
      const yes = f.get("action") === "execute" ? exec : ask;
      res.innerHTML = `<div class="card flat"><p><span class="chip ${yes ? "ok" : "bad"}">${f.get("action")}: ${yes ? "permitted" : "denied"}</span>
        <strong>${{ allow: "It acts without asking.", ask: "It asks first.", deny: "It is refused." }[r.decision]}</strong></p>
        <dl class="kv small"><dt>Decided by</dt><dd class="mono">${esc(r.rule || "no rule: refused by default")}</dd>${r.reason ? `<dt>Why</dt><dd>${esc(r.reason)}</dd>` : ""}<dt>Layers</dt><dd class="mono">${esc(r.layers.join(" → "))}</dd><dt>Tool</dt><dd>${esc(r.risk)}, ${esc(r.audience)}</dd></dl></div>`;
    } catch (err) { res.innerHTML = `<p class="error">${esc(err.message)}</p>`; }
  };
  const hist = $("#pol-history", box);
  try {
    const h = (await api("GET", `/policy/sets/${cur.id}/history`)).versions;
    hist.innerHTML = h.length ? `<table class="t small"><thead><tr><th>Version</th><th>When</th><th>By</th><th></th></tr></thead><tbody>${h.map((x, i) => `<tr><td>${esc(x.version)}${i === 0 ? ` <span class="chip ok">live</span>` : ""}</td><td>${esc(when(x.at))}</td><td>${esc(who(x.by))}</td>
      <td><button class="link small" data-show="${x.version}">Show</button>${i === 0 ? "" : ` <button class="link small" data-back="${x.version}">Roll back to this</button>`}</td></tr>`).join("")}</tbody></table>`
      : UI.empty(cur.default ? "Never written: the default policy is in force." : "No versions yet.");
    for (const b of $$("[data-show]", hist)) b.onclick = () => { const x = h.find(y => String(y.version) === b.dataset.show); out.innerHTML = `<h3>Version ${esc(x.version)} against live</h3>${diffHtml(diff(cur.live, x.cedar))}`; };
    for (const b of $$("[data-back]", hist)) b.onclick = async () => {
      if (await UI.confirm({ title: `Go back to version ${b.dataset.back}?`, body: "It is put in force again as a new version. It asks for your password again.", verb: "Roll back" }))
        act("POST", `/policy/sets/${cur.id}/rollback`, { version: Number(b.dataset.back) }, () => { UI.toast(`Version ${b.dataset.back} is in force again.`); reload(); });
    };
  } catch (e) { UI.error(hist, "The history could not be read.", e); }
}

// Security › Connectors and credentials: the organisation's apps, their
// status, how people reach them, their scopes and last use; a row opens the
// app, where it is turned off or on, who may use it is set, and the
// organisation's credential is set, rotated or revoked. A credential is
// never shown, only whether it is set and when.

const CONN_STATUS = { ready: ["ok", "Ready"], needs_credential: ["warn", "Needs its credential"], needs_review: ["warn", "Tools not reviewed"], off: ["", "Off"] };
const SIGN_IN = { own_account: "Each person's own account", org_key: "The organisation's key", none: "No sign-in" };
const AUDIENCE = [["private", "Only the person"], ["internal", "People in the organisation"], ["public", "Anyone"]];
const LABEL = [["principal", "Trusted, not private"], ["private", "Private"], ["untrusted", "Untrusted"], ["private+untrusted", "Private and untrusted"]];
const credName = c => (c.credential.kind === "client_secret" ? `${c.registration ? c.registration[0].toUpperCase() + c.registration.slice(1) : "The provider"}'s client secret` : "The organisation's key");
const day = t => (t ? new Date(t).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" }) : "");

async function connectorsView(box, name) {
  if (!$("#conn-rows", box)) {
    box.innerHTML = `<div class="row ad-bar">
        <input id="conn-q" type="search" placeholder="Find an app" aria-label="Find an app" autocomplete="off">
        <select id="conn-f" aria-label="Show"><option value="">All apps</option><option value="attention">Needs attention</option><option value="off">Off</option><option value="used">Used by someone</option></select>
        <button class="primary" id="conn-add">Add an app</button>
      </div><div id="conn-rows"></div>`;
    $("#conn-q", box).oninput = () => drawConnectors(box);
    $("#conn-f", box).onchange = () => drawConnectors(box);
    $("#conn-add", box).onclick = () => addConnector(box);
    await loadConnectors(box);
  }
  if (!name) return UI.closePanel();
  connectorPanel(box, name);
}

async function loadConnectors(box) {
  const el = $("#conn-rows", box);
  if (!box.conns) UI.loading(el, "the organisation's apps");
  try { box.conns = (await api("GET", "/connectors")).connectors; drawConnectors(box); }
  catch (e) { UI.error(el, "The organisation's apps could not be read.", e, () => loadConnectors(box)); }
}

function drawConnectors(box) {
  const el = $("#conn-rows", box);
  const q = $("#conn-q", box).value.trim().toLowerCase(), f = $("#conn-f", box).value;
  const all = box.conns || [];
  const rows = all.filter(c => (!q || `${c.title} ${c.name}`.toLowerCase().includes(q))
    && (!f || (f === "attention" && (c.status === "needs_credential" || c.status === "needs_review")) || (f === "off" && !c.enabled) || (f === "used" && c.last_used)));
  el.innerHTML = rows.length ? `<div class="ad-scroll"><table class="t small"><thead><tr><th>App</th><th>Status</th><th>Sign-in</th><th>Scopes</th><th>Credential</th><th class="n">Signed in</th><th>Last used</th></tr></thead><tbody>${rows.map(c => {
      const [cls, label] = CONN_STATUS[c.status] || ["", c.status];
      return `<tr class="click" data-href="#/security/connectors/${esc(c.name)}" tabindex="0">
        <td><strong>${esc(c.title)}</strong> <span class="muted mono">${esc(c.name)}</span></td>
        <td><span class="chip ${cls}">${esc(label)}</span></td>
        <td>${esc(SIGN_IN[c.sign_in] || c.sign_in)}</td>
        <td>${c.scopes.length ? `${c.scopes.length} scope${c.scopes.length === 1 ? "" : "s"}` : "–"}</td>
        <td>${c.credential.kind === "none" ? "–" : c.credential.set ? `Set ${esc(day(c.credential.changed_at))}` : `<span class="chip warn">Not set</span>`}</td>
        <td class="n">${esc(c.signed_in)}</td><td>${esc(c.last_used ? when(c.last_used) : "Never")}</td></tr>`;
    }).join("")}</tbody></table></div>`
    : UI.empty(all.length ? "No app matches." : "The organisation has no apps yet.", all.length ? "Clear the search or the filter." : "Add one by its address.");
  rowLinks(el);
}

async function connectorPanel(box, name) {
  const c = (box.conns || []).find(x => x.name === name);
  const back = "#/security/connectors";
  if (!c) { const b = UI.panel("App", "", back); b.innerHTML = UI.empty("No such app."); return; }
  const [cls, label] = CONN_STATUS[c.status] || ["", c.status];
  const reload = async () => { await loadConnectors(box); if (location.hash === `${back}/${name}`) connectorPanel(box, name); };
  const audience = (AUDIENCE.find(([k]) => k === c.audience) || [, c.audience])[1];
  const returns = (LABEL.find(([k]) => k === c.label) || [, c.label])[1];
  const body = UI.panel(c.title, `
    <p class="small"><span class="chip ${cls}">${esc(label)}</span> <span class="muted">${esc({ catalog: "From the catalog", added: "Added by its address", config: "In the appliance's config file" }[c.source])}</span></p>
    <dl class="kv small">
      <dt>Name</dt><dd class="mono">${esc(c.name)}</dd>
      <dt>Address</dt><dd class="mono ad-wrap">${esc(c.url)}</dd>
      <dt>Sign-in</dt><dd>${esc(SIGN_IN[c.sign_in] || c.sign_in)}</dd>
      <dt>Scopes</dt><dd>${c.scopes.length ? c.scopes.map(x => `<div class="mono ad-wrap">${esc(x)}</div>`).join("") : "–"}</dd>
      <dt>What reaches it</dt><dd>${esc(audience)}</dd>
      <dt>What it returns</dt><dd>${esc(returns)}</dd>
      <dt>Signed in</dt><dd>${esc(c.signed_in)} ${c.signed_in === 1 ? "person" : "people"}</dd>
      <dt>Last used</dt><dd>${esc(c.last_used ? when(c.last_used) : "Never")}</dd>
    </dl>
    <h3>For the organisation</h3>
    <div class="row ad-bar">${c.enabled ? `<button id="cn-off" class="danger">Turn off</button>` : `<button id="cn-on" class="primary">Turn on</button>`}</div>
    <p class="small muted">${c.enabled ? "Turned off, no one is offered it and no task reaches it. People's sign-ins stay until revoked." : "It is off: no one is offered it and no task reaches it."}</p>
    <form id="cn-groups" class="ad-form"><label>Who may use it <input name="groups" value="${esc(c.access_groups.join(", "))}" placeholder="Everyone" autocomplete="off" aria-describedby="cn-groups-how"></label>
      <label>Blocked for <input name="blocked" value="${esc((c.blocked_groups || []).join(", "))}" placeholder="No one" autocomplete="off" aria-describedby="cn-groups-how"></label>
      <p class="small muted" id="cn-groups-how">Groups, separated by commas. Who may use it, empty: everyone in the organisation. A group it is blocked for neither sees it in the Library nor reaches it, whatever the first list says.</p>
      <div class="row"><button>Save</button></div></form>
    <h3>Credential</h3>
    ${c.credential.kind === "none"
      ? `<p class="small">${c.sign_in === "own_account" ? "Each person signs in with their own account. The organisation holds no credential for it." : "It takes no credential."}</p>
         ${c.signed_in ? `<div class="row ad-bar"><button id="cn-revoke" class="danger">Revoke every sign-in</button></div>` : ""}`
      : `<p class="small">${esc(credName(c))}: ${c.credential.set ? `<strong>set</strong>${c.credential.changed_at ? `, ${esc(when(c.credential.changed_at))}` : ""}` : "<strong>not set</strong>"}. It is never shown again, here or anywhere.</p>
         ${c.credential.kind === "client_secret" ? `<p class="small muted">It serves every ${esc(c.registration)} app: setting it here sets it for all of them.</p>` : ""}
         <form id="cn-cred" class="ad-form"><label>${c.credential.set ? "New value" : "Value"} <input name="value" type="password" autocomplete="new-password" spellcheck="false" required></label>
           <div class="row"><button class="primary">${c.credential.set ? "Rotate" : "Set"}</button>${c.credential.set ? ` <button type="button" id="cn-revoke" class="danger">Revoke</button>` : ""}</div></form>`}
    ${c.source === "added" ? `<h3>Remove</h3><p class="small muted">Removing it revokes every sign-in to it and deletes its key.</p><div class="row"><button id="cn-remove" class="danger">Remove ${esc(c.title)}</button></div>` : ""}`, back);
  const setOn = on => act("PATCH", `/connectors/${name}`, { enabled: on }, async () => {
    UI.toast(on ? `${c.title} is on.` : `${c.title} is off for everyone.`, () => act("PATCH", `/connectors/${name}`, { enabled: !on }, reload));
    await reload();
  });
  $("#cn-off", body)?.addEventListener("click", () => setOn(false));
  $("#cn-on", body)?.addEventListener("click", () => setOn(true));
  $("#cn-groups", body).onsubmit = e => {
    e.preventDefault();
    const f = new FormData(e.target), list = k => f.get(k).split(",").map(x => x.trim()).filter(Boolean);
    const groups = list("groups"), blocked = list("blocked");
    const was = { access_groups: c.access_groups, blocked_groups: c.blocked_groups || [] };
    act("PATCH", `/connectors/${name}`, { access_groups: groups, blocked_groups: blocked }, async () => {
      const who = groups.length ? `Only ${groups.join(", ")} may use ${c.title}` : `Everyone may use ${c.title}`;
      UI.toast(blocked.length ? `${who}, except ${blocked.join(", ")}.` : `${who}.`, () => act("PATCH", `/connectors/${name}`, was, reload));
      await reload();
    });
  };
  const cred = $("#cn-cred", body);
  if (cred) cred.onsubmit = e => {
    e.preventDefault();
    const value = new FormData(e.target).get("value");
    e.target.reset();
    act("PUT", `/connectors/${name}/credential`, { value }, async () => { UI.toast(`${credName(c)} is ${c.credential.set ? "rotated" : "set"}.`); await reload(); });
  };
  const rev = $("#cn-revoke", body);
  if (rev) rev.onclick = async () => {
    const shared = c.credential.kind === "client_secret" ? ` Every person's sign-in to each ${c.registration} app ends.` : c.signed_in ? ` ${c.signed_in} sign-in${c.signed_in === 1 ? "" : "s"} end.` : "";
    if (await UI.confirm({ title: c.credential.kind === "none" ? `Revoke every sign-in to ${c.title}?` : `Revoke ${credName(c).toLowerCase()}?`, body: `It is revoked at the app and deleted here.${shared} People sign in again once it is set.`, verb: "Revoke", danger: true, typed: c.name }))
      act("DELETE", `/connectors/${name}/credential`, null, async v => { UI.toast(`Revoked: ${v?.sign_ins_revoked ?? 0} sign-ins ended.`); await reload(); });
  };
  const rm = $("#cn-remove", body);
  if (rm) rm.onclick = async () => {
    if (await UI.confirm({ title: `Remove ${c.title}?`, body: "Every sign-in to it is revoked, its key deleted, and it leaves the Library.", verb: "Remove", danger: true, typed: c.name }))
      act("DELETE", `/connectors/${name}`, null, async () => { UI.toast(`${c.title} is removed.`); location.hash = back; await loadConnectors(box); });
  };
}

function addConnector(box) {
  const d = document.createElement("dialog");
  d.className = "ad-sheet";
  d.setAttribute("aria-labelledby", "add-title");
  d.innerHTML = `<form id="add-form">
    <h2 id="add-title">Add an app</h2>
    <p class="small muted">By its MCP address. Magnus finds its sign-in from the address; each person then connects it with their own account. A server that asks for a key or client details instead is refused.</p>
    <label>Address <input name="url" type="url" required placeholder="https://" autocomplete="off"></label>
    <label>What reaches it <select name="audience">${AUDIENCE.map(([k, l]) => `<option value="${k}">${l}</option>`).join("")}</select></label>
    <label>What it returns <select name="label">${LABEL.map(([k, l]) => `<option value="${k}" ${k === "private+untrusted" ? "selected" : ""}>${l}</option>`).join("")}</select></label>
    <label>Who may use it <input name="groups" placeholder="Everyone" autocomplete="off"></label>
    <p class="error" id="add-error" hidden></p>
    <div class="row ad-sheet-actions"><button type="button" id="add-cancel">Cancel</button><button class="primary">Add</button></div>
  </form>`;
  document.body.append(d);
  d.addEventListener("close", () => d.remove());
  $("#add-cancel", d).onclick = () => d.close();
  $("#add-form", d).onsubmit = async e => {
    e.preventDefault();
    const f = Object.fromEntries(new FormData(e.target));
    const body = { url: f.url, audience: f.audience, label: f.label, access_groups: f.groups.split(",").map(x => x.trim()).filter(Boolean) };
    try {
      const v = await api("POST", "/connectors", body);
      d.close();
      UI.toast(`${v.name} is added.${v.sign_in ? " People sign in to it with their own accounts." : ""}`);
      await loadConnectors(box);
      location.hash = `#/security/connectors/${v.name}`;
    } catch (err) { const el = $("#add-error", d); el.textContent = err.message; el.hidden = false; }
  };
  d.showModal();
}

// ---- Users -------------------------------------------------------------
//
// Everyone the appliance keeps, with Invite; a row opens the person
// in the detail panel, as tabs.

async function usersView(view, id, tab) {
  if (!$("#users-rows", view)) {
    view.innerHTML = `<h1>Users</h1>
      <div class="row ad-bar"><input id="users-q" type="search" placeholder="Find by name or email" aria-label="Find people" autocomplete="off"><button class="primary" id="invite-open">Invite</button></div>
      <div id="users-rows"></div>`;
    $("#users-q", view).oninput = () => drawUsers(view);
    $("#invite-open", view).onclick = () => invite(view);
    await loadUsers(view);
  }
  if (!id) return UI.closePanel();
  return userPanel(view, id, tab || "overview");
}

async function loadUsers(view) {
  const box = $("#users-rows", view);
  UI.loading(box, "the people");
  try { view.users = (await api("GET", "/users")).users; drawUsers(view); }
  catch (e) { UI.error(box, "The people could not be read.", e, () => loadUsers(view)); }
}

function drawUsers(view) {
  const box = $("#users-rows", view);
  const q = $("#users-q", view).value.trim().toLowerCase();
  const all = view.users || [];
  const rows = all.filter(u => !q || `${u.name} ${u.email}`.toLowerCase().includes(q));
  box.innerHTML = rows.length ? `<div class="ad-scroll"><table class="t small"><thead><tr><th>Name</th><th>Email</th><th>Role</th><th>Status</th><th>Last sign-in</th><th class="n">Sessions</th><th></th></tr></thead><tbody>${rows.map(u => `<tr class="click" data-href="#/users/${esc(u.id)}" tabindex="0">
      <td>${esc(u.name)}</td><td>${esc(u.email || "")}</td><td>${u.role === "admin" ? "Admin" : "Member"}</td>
      <td>${pill(u.status)}</td><td>${esc(when(u.last_sign_in))}</td><td class="n">${esc(u.sessions)}</td>
      <td>${u.status === "invited" ? `<button class="link small" data-resend="${esc(u.id)}">Resend</button> <button class="link small" data-otp="${esc(u.id)}">One-time password</button> <button class="link small" data-cancel="${esc(u.id)}">Cancel</button>` : ""}</td></tr>`).join("")}</tbody></table></div>`
    : UI.empty(all.length ? "Nobody matches." : "Nobody is in the organisation yet.", all.length ? null : "Invite the first person.");
  rowLinks(box);
  for (const b of $$("[data-resend]", box)) b.onclick = () => act("POST", `/users/${b.dataset.resend}/invite`, null, v => { if (v) showInviteToken(all.find(x => x.id === b.dataset.resend), v); });
  for (const b of $$("[data-otp]", box)) b.onclick = () => oneTimePassword(all.find(x => x.id === b.dataset.otp));
  for (const b of $$("[data-cancel]", box)) b.onclick = async () => {
    const u = all.find(x => x.id === b.dataset.cancel);
    if (await UI.confirm({ title: "Cancel this invitation?", body: `${u?.email || "Their"} account goes, and the invitation link stops working.`, verb: "Cancel invitation", danger: true }))
      act("DELETE", `/users/${b.dataset.cancel}/invite`, null, () => { UI.toast("The invitation is cancelled."); loadUsers(view); });
  };
}

// An invitation's single-use token, shown once for the admin to pass on:
// the appliance sends no email. The old token dies when a new one shows.
function showInviteToken(u, v) {
  const d = document.createElement("dialog");
  d.className = "ad-sheet";
  d.setAttribute("aria-labelledby", "invite-token-title");
  d.innerHTML = `<h2 id="invite-token-title">Pass on the invitation</h2>
    <p class="small muted">Pass this token on to ${esc(u?.name || "them")}. They set their password with it, within seven days. It is shown only now.</p>
    <label>Username <input readonly value="${esc(u?.id || v.id || "")}"></label>
    <label>Invitation token <input readonly id="invite-token-value" value="${esc(v.invite_token)}"></label>
    <div class="row ad-sheet-actions"><button type="button" id="invite-token-copy">Copy token</button><button type="button" class="primary" id="invite-token-done">Done</button></div>`;
  document.body.append(d);
  d.addEventListener("close", () => d.remove());
  $("#invite-token-copy", d).onclick = () => navigator.clipboard?.writeText(v.invite_token).then(() => UI.toast("Copied."), () => $("#invite-token-value", d).select());
  $("#invite-token-done", d).onclick = () => d.close();
  d.showModal();
}

// For when no invite token can reach them: a password to choose with
// once, shown here only, which the admin passes on; choosing with it
// sets their own. It never signs in.
async function oneTimePassword(u) {
  if (!u) return;
  if (!(await UI.confirm({ title: "Give a one-time password?", body: `${u.email || u.name} chooses their own password with it. It is shown only now: pass it on yourself.`, verb: "Make password" }))) return;
  let v;
  try { v = await api("POST", `/users/${u.id}/invite/password`); }
  catch (e) { return UI.toast(e.message); }
  const d = document.createElement("dialog");
  d.className = "ad-sheet";
  d.setAttribute("aria-labelledby", "otp-title");
  d.innerHTML = `<h2 id="otp-title">One-time password</h2>
    <p class="small muted">Pass these on to ${esc(u.name)}. The password chooses their own; it never signs in.</p>
    <label>Username <input readonly value="${esc(v.username)}"></label>
    <label>One-time password <input readonly id="otp-value" value="${esc(v.password)}"></label>
    <div class="row ad-sheet-actions"><button type="button" id="otp-copy">Copy password</button><button type="button" class="primary" id="otp-done">Done</button></div>`;
  document.body.append(d);
  d.addEventListener("close", () => d.remove());
  $("#otp-copy", d).onclick = () => navigator.clipboard?.writeText(v.password).then(() => UI.toast("Copied."), () => $("#otp-value", d).select());
  $("#otp-done", d).onclick = () => d.close();
  d.showModal();
}

function invite(view) {
  const d = document.createElement("dialog");
  d.className = "ad-sheet";
  d.setAttribute("aria-labelledby", "invite-title");
  d.innerHTML = `<form id="invite-form">
    <h2 id="invite-title">Invite someone</h2>
    <p class="small muted">The appliance makes their account and shows a token to pass on, which they set their password with. The appliance sends no email.</p>
    <label>Email <input name="email" type="email" required autocomplete="off"></label>
    <label>Name <input name="name" required autocomplete="off"></label>
    <label>Role <select name="role"><option value="member">Member</option><option value="admin">Admin (asks for your password again)</option></select></label>
    <p class="error" id="invite-error" hidden></p>
    <div class="row ad-sheet-actions"><button type="button" id="invite-cancel">Cancel</button><button class="primary">Invite</button></div>
  </form>`;
  document.body.append(d);
  d.addEventListener("close", () => d.remove());
  $("#invite-cancel", d).onclick = () => d.close();
  $("#invite-form", d).onsubmit = async e => {
    e.preventDefault();
    try {
      const v = await api("POST", "/users", Object.fromEntries(new FormData(e.target)));
      d.close();
      loadUsers(view);
      showInviteToken({ id: v.id, name: e.target.name.value }, v);
    } catch (err) { const el = $("#invite-error", d); el.textContent = err.message; el.hidden = false; }
  };
  d.showModal();
}

async function userPanel(view, id, tab) {
  const body = UI.panel("Person", "", "#/users");
  UI.loading(body, "this person");
  let u;
  try { u = await api("GET", `/users/${id}`); } catch (e) { return UI.error(body, "This person could not be read.", e, () => userPanel(view, id, tab)); }
  $("#panel h2").textContent = u.name;
  const tabs = [["overview", "Overview"], ["sessions", "Sessions"], ["schedules", "Schedules"], ["data", "Data"], ["delete", "Delete"]];
  const sessions = Array.isArray(u.sessions) ? u.sessions : [];
  const again = () => { loadUsers(view); userPanel(view, id, tab); };
  body.innerHTML = `<p class="muted small">${esc(u.email || "")} · ${u.role === "admin" ? "Admin" : "Member"} · ${pill(u.status)}</p>
    <nav class="ad-tabs" aria-label="${esc(u.name)}">${tabs.map(([k, l]) => `<a href="#/users/${esc(id)}/${k}" ${k === tab ? 'aria-current="page"' : ""}>${l}</a>`).join("")}</nav>
    <div id="user-tab"></div>`;
  const pane = $("#user-tab", body);
  if (tab === "schedules") return userSchedules(pane, u, id);
  if (tab === "data") return userData(pane, u, id);
  if (tab === "delete") return userDelete(pane, u, id, view);
  if (tab === "sessions") {
    pane.innerHTML = sessions.length ? `<table class="t small"><thead><tr><th>Signed in</th><th>From</th><th>Ends</th><th></th></tr></thead><tbody>${sessions.map(s => `<tr><td>${esc(when(s.created))}</td><td>${esc(s.client === "browser" ? "A browser" : "The app")}</td><td>${esc(when(s.expires))}</td><td><button class="link small" data-end="${esc(s.id)}">End</button></td></tr>`).join("")}</tbody></table>`
      : UI.empty("No sessions open on the appliance.");
    for (const b of $$("[data-end]", pane)) b.onclick = async () => {
      if (await UI.confirm({ title: "End this session?", body: `That device is signed out of ${org()}.`, verb: "End session", danger: true }))
        act("DELETE", `/users/${id}/sessions/${b.dataset.end}`, null, () => { UI.toast("Session ended."); again(); });
    };
    return;
  }
  const other = u.role === "admin" ? "member" : "admin";
  pane.innerHTML = `<dl class="kv small"><dt>Created</dt><dd>${esc(when(u.created))}</dd><dt>Sessions here</dt><dd>${esc(sessions.length)}</dd></dl>
    <div class="row ad-bar">
      <button id="u-role">Make ${other === "admin" ? "an admin" : "a member"}</button>
      ${u.status === "disabled" ? `<button id="u-enable">Turn on</button>` : `<button id="u-disable" class="danger">Turn off</button>`}
      <button id="u-end">End all sessions</button>
    </div>
    <p class="small muted">Turning someone off ends their sessions here, and pauses their schedules until they resume them. A role change asks for your password again.</p>`;
  $("#u-role", pane).onclick = async () => {
    if (await UI.confirm({ title: `Make ${u.name} ${other === "admin" ? "an admin" : "a member"}?`, body: other === "admin" ? "An admin has full authority over everything on the appliance." : "They lose the admin console.", verb: "Change role" }))
      act("PATCH", `/users/${id}`, { role: other }, () => { UI.toast(`${u.name} is now ${other === "admin" ? "an admin" : "a member"}.`); again(); });
  };
  const dis = $("#u-disable", pane);
  if (dis) dis.onclick = () => act("POST", `/users/${id}/disable`, null, v => {
    UI.toast(`${u.name} is turned off: ${v?.sessions_ended ?? 0} sessions ended, ${v?.schedules_paused ?? 0} schedules paused.`, () => act("POST", `/users/${id}/enable`, null, again));
    again();
  });
  const en = $("#u-enable", pane);
  if (en) en.onclick = () => act("POST", `/users/${id}/enable`, null, () => { UI.toast(`${u.name} is turned on. Their paused schedules stay paused until they resume them.`); again(); });
  $("#u-end", pane).onclick = async () => {
    if (await UI.confirm({ title: `End every session ${u.name} has open?`, body: "Every device they use is signed out here.", verb: "End all sessions", danger: true }))
      act("DELETE", `/users/${id}/sessions`, null, () => { UI.toast("Every session ended."); again(); });
  };
}

// A schedule's rule in words.
const rule = x => (x.daily_at ? `Every day at ${x.daily_at}` : x.every_minutes ? (x.every_minutes % 1440 === 0 ? `Every ${x.every_minutes / 1440} day${x.every_minutes === 1440 ? "" : "s"}` : x.every_minutes % 60 === 0 ? `Every ${x.every_minutes / 60} hour${x.every_minutes === 60 ? "" : "s"}` : `Every ${x.every_minutes} minutes`) : "Once");

// Users › Schedules and triggers: each of their schedules, suspended or
// resumed by an admin. (Magnus starts work on a schedule or on request;
// it has no other triggers.)
async function userSchedules(pane, u, id) {
  UI.loading(pane, "their schedules");
  let v;
  try { v = await api("GET", `/users/${id}/schedules`); } catch (e) { return UI.error(pane, "Their schedules could not be read.", e, () => userSchedules(pane, u, id)); }
  const list = v.schedules;
  pane.innerHTML = list.length ? `<div class="ad-scroll"><table class="t small"><thead><tr><th>Schedule</th><th>When</th><th>Next</th><th class="n">Runs</th><th class="n">Failures</th><th></th></tr></thead><tbody>${list.map(x => `<tr>
      <td>${esc(x.title)}${x.paused ? `<div class="small"><span class="chip warn">Suspended</span> <span class="muted">${esc(x.paused.reason || x.paused)}</span></div>` : ""}</td>
      <td>${esc(rule(x))}${x.until ? `<div class="small muted">until ${esc(when(x.until))}</div>` : ""}</td>
      <td>${x.paused ? "–" : esc(when(x.next))}</td><td class="n">${esc(x.runs)}</td><td class="n">${esc(x.failures)}</td>
      <td>${x.paused ? `<button class="link small" data-resume="${esc(x.id)}">Resume</button>` : `<button class="link small" data-suspend="${esc(x.id)}">Suspend</button>`}</td></tr>`).join("")}</tbody></table></div>`
    : UI.empty(`${u.name} has no schedules.`);
  const again = () => userSchedules(pane, u, id);
  for (const b of $$("[data-suspend]", pane)) b.onclick = () => act("POST", `/users/${id}/schedules/${b.dataset.suspend}/suspend`, null, () => {
    UI.toast("Suspended: it does not run until it is resumed.", () => act("POST", `/users/${id}/schedules/${b.dataset.suspend}/resume`, null, again));
    again();
  });
  for (const b of $$("[data-resume]", pane)) b.onclick = () => act("POST", `/users/${id}/schedules/${b.dataset.resume}/resume`, null, () => { UI.toast("Resumed."); again(); });
}

// Users › Data: root's access to their tasks, conversations, files and
// memory. Nothing is read until asked; each read is in the audit log and
// in their inbox.
const DATA_KINDS = [["tasks", "Tasks"], ["chats", "Conversations"], ["files", "Files"], ["memory", "Memory"]];

function userData(pane, u, id) {
  pane.innerHTML = `<div class="banner" role="note"><strong>Each access is recorded.</strong> Opening ${esc(u.name)}'s tasks, conversations, files or memory writes it to the audit log, and they are told in their inbox.</div>
    <div class="ad-seg" role="group" aria-label="Their data" id="data-kinds">${DATA_KINDS.map(([k, l]) => `<button data-kind="${k}">${l}</button>`).join("")}</div>
    <div id="data-box">${UI.empty("Choose what to open.")}</div>`;
  const box = $("#data-box", pane);
  for (const b of $$("[data-kind]", pane)) b.onclick = () => {
    for (const x of $$("[data-kind]", pane)) x.setAttribute("aria-pressed", String(x === b));
    dataList(box, u, id, b.dataset.kind);
  };
}

async function dataList(box, u, id, kind) {
  UI.loading(box, `their ${kind}`);
  let v;
  try { v = await api("GET", `/users/${id}/data/${kind}`); } catch (e) { return UI.error(box, `Their ${kind} could not be read.`, e, () => dataList(box, u, id, kind)); }
  const rows = v[kind] || [];
  const open = item => dataItem(box, u, id, kind, item);
  const line = {
    tasks: t => [t.task_id, t.title || t.text, `${t.status} · ${when(t.created_at)}`],
    chats: t => [t.id || t.thread, t.title || "Conversation", when(t.updated_at || t.at || t.created_at)],
    files: f => [f.id, f.name, `${f.media_type || ""} · ${when(f.created_at || f.at)}`],
    memory: m => [m.id, m.fact, `${m.topic || ""} · ${when(m.at)}`],
  }[kind];
  box.innerHTML = rows.length ? `<ul class="ad-list">${rows.map(r => { const [key, title, sub] = line(r); return `<li><button class="link" data-open="${esc(key)}">${esc(title || "(untitled)")}</button><div class="small muted">${esc(sub)}</div></li>`; }).join("")}</ul>`
    : UI.empty(`${u.name} has no ${kind === "chats" ? "conversations" : kind}.`);
  for (const b of $$("[data-open]", box)) b.onclick = () => (kind === "memory" ? null : open(b.dataset.open));
}

async function dataItem(box, u, id, kind, item) {
  UI.loading(box, "it");
  let v;
  try { v = (await api("GET", `/users/${id}/data/${kind}/${encodeURIComponent(item)}`))[kind]; } catch (e) { return UI.error(box, "It could not be read.", e, () => dataItem(box, u, id, kind, item)); }
  const back = `<p class="small"><button class="link" id="data-back">← All their ${kind === "chats" ? "conversations" : kind}</button></p>`;
  let html = "";
  if (kind === "tasks") {
    const t = v.task;
    html = `<h3>${esc(t.title || t.text)}</h3><p class="small muted">${esc(t.status)} · ${esc(when(t.created_at))}${t.summary ? ` · ${esc(t.summary)}` : ""}</p>
      ${(v.timeline.steps || []).map(st => `<div class="card flat small"><div class="muted">Step ${esc(st.step)}</div>${st.text ? `<p>${esc(st.text)}</p>` : ""}${st.calls.map(c => `<div><span class="mono">${esc(c.tool)}</span> ${c.is_error ? `<span class="chip bad">failed</span>` : ""}${c.result ? `<pre class="ad-json">${esc(c.result)}</pre>` : ""}</div>`).join("")}</div>`).join("") || UI.empty("No steps.")}`;
  } else if (kind === "chats") {
    html = (v.entries || []).map(e => `<div class="card flat small"><div class="muted">${esc(e.kind === "message" ? u.name : "Magnus")} · ${esc(when(e.at))}</div><p class="ad-wrap">${esc(e.text || e.summary || "")}</p></div>`).join("") || UI.empty("Nothing was said.");
  } else if (kind === "files") {
    const f = v.file;
    html = `<h3>${esc(f.name)}</h3><p class="small muted">${esc(f.media_type)} · ${esc(v.bytes)} bytes · ${esc(when(f.created_at || f.at))}</p>${v.text != null ? `<pre class="ad-json">${esc(v.text)}</pre>` : UI.empty("Not text: nothing to show here.")}`;
  }
  box.innerHTML = back + html;
  $("#data-back", box).onclick = () => dataList(box, u, id, kind);
}

// Users › Delete: the account and everything of theirs, for good. It asks
// for a fresh password, and their email typed.
function userDelete(pane, u, id, view) {
  const proof = u.email || u.id;
  pane.innerHTML = `<p>Deleting ${esc(u.name)} removes their account here and everything of theirs on the appliance: their running tasks stop, their schedules end, their sessions and their sign-ins to apps are revoked, and their tasks, conversations, files and memory are deleted, from the backups too. It cannot be undone.</p>
    <div class="row ad-bar"><button class="danger" id="u-delete">Delete ${esc(u.name)}</button></div>
    <p class="small muted">It asks for your password again, and for their ${u.email ? "email address" : "id"} typed.</p>`;
  $("#u-delete", pane).onclick = async () => {
    if (!(await UI.confirm({ title: `Delete ${u.name} for good?`, body: "Their account and all of their data go, and nothing brings them back.", verb: "Delete", danger: true, typed: proof }))) return;
    act("DELETE", `/users/${id}`, { email: proof }, v => {
      const erased = v?.erased || {};
      UI.toast(erased.erasure === "incomplete"
        ? `${u.name} is deleted, but not fully erased: ${(erased.backups_pending || []).length} backups still hold their data. Magnus tries again every hour and at start; the audit log says when it is done.`
        : `${u.name} is deleted: ${erased.runs_stopped ?? 0} tasks stopped, ${erased.schedules_ended ?? 0} schedules ended, ${erased.content_removed ?? 0} records removed.`);
      location.hash = "#/users";
      loadUsers(view);
    });
  };
}

// ---- Knowledge ---------------------------------------------------------
//
// The organisation's shared ontology, context and memory, which agents
// curate (the org context bundle), as an admin oversees, corrects and
// rolls it back. Documents are edited as OKF writes them: YAML
// frontmatter, then the body.

const KNOW_TABS = [["ontology", "Ontology"], ["context", "Context"], ["memory", "Memory"], ["activity", "Activity"]];
const byWho = by => { const b = String(by || ""); return b.startsWith("human:") ? who(`admin:${b.slice(6)}`) : b.startsWith("magnus/") ? `an agent (${b})` : who(b); };

function knowledgeView(view, rest) {
  const tab = KNOW_TABS.some(([k]) => k === rest[0]) ? rest[0] : "ontology";
  if (!$("#know-tabs", view)) view.innerHTML = `<h1>Knowledge</h1><nav class="ad-tabs" id="know-tabs" aria-label="Knowledge">${KNOW_TABS.map(([k, l]) => `<a href="#/knowledge/${k}" data-tab="${k}">${l}</a>`).join("")}</nav><div id="know"></div>`;
  for (const a of $$("#know-tabs a", view)) { if (a.dataset.tab === tab) a.setAttribute("aria-current", "page"); else a.removeAttribute("aria-current"); }
  const box = $("#know", view);
  if (box.dataset.tab !== tab) { box.dataset.tab = tab; box.innerHTML = ""; UI.closePanel(); }
  return ({ ontology: ontologyView, context: contextView, memory: memoryView, activity: activityView })[tab](box, rest[1]);
}

// A document's versions, each shown against now and rolled back to.
function versionsHtml(d) {
  return `<div class="ad-scroll"><table class="t small"><thead><tr><th>Version</th><th>When</th><th>By</th><th></th></tr></thead><tbody>${d.versions.map((v, i) => `<tr><td>${esc(v.version)}${i === 0 ? ` <span class="chip ok">now</span>` : ""}</td><td>${esc(when(v.at))}</td><td>${esc(byWho(v.by))}${v.run ? ` <span class="muted mono">${esc(v.run)}</span>` : ""}</td>
    <td><button class="link small" data-vshow="${v.version}">Show</button>${i === 0 ? "" : ` <button class="link small" data-vback="${v.version}">Roll back to this</button>`}</td></tr>`).join("")}</tbody></table></div><div class="ad-vout"></div>`;
}

function bindVersions(el, d, rollback, after) {
  for (const b of $$("[data-vshow]", el)) b.onclick = async () => {
    const v = await act("GET", `/knowledge/documents/${d.id}/versions/${b.dataset.vshow}`);
    if (v) $(".ad-vout", el).innerHTML = `<h4>Version ${esc(b.dataset.vshow)} against now</h4>${diffHtml(diff(d.text, v.text))}`;
  };
  for (const b of $$("[data-vback]", el)) b.onclick = async () => {
    if (await UI.confirm({ title: `Go back to version ${b.dataset.vback}?`, body: "It is written again as a new version; nothing is lost.", verb: "Roll back" }))
      act("POST", rollback, { version: Number(b.dataset.vback) }, () => { UI.toast(`Version ${b.dataset.vback} is back.`); after(); });
  };
}

// An OKF text editor: the text, Save, and the kernel's reason when it
// refuses.
function okfEditor(text, save) {
  const id = `okf-${Math.random().toString(36).slice(2)}`;
  return { html: `<form class="ad-form" id="${id}"><textarea class="mono ad-okf" rows="14" spellcheck="false" aria-label="The document, as OKF text">${esc(text)}</textarea><p class="error" hidden></p><div class="row"><button class="primary">Save</button></div></form>`,
    bind: el => { const f = $(`#${id}`, el); f.onsubmit = async e => { e.preventDefault(); try { await save($("textarea", f).value); } catch (err) { const p = $(".error", f); p.textContent = err.message; p.hidden = false; } }; } };
}

// The type and its directly related types, drawn: the type in the middle,
// each related type around it, each edge named with its cardinality.
function typeGraph(t, all) {
  const rels = (t.relations || []).filter(r => r.to);
  const others = [...new Set(rels.map(r => r.to))];
  const back = all.filter(x => x.path !== t.path && (x.relations || []).some(r => r.to === t.path));
  for (const b of back) if (!others.includes(b.path)) others.push(b.path);
  const W = 360, H = 240, cx = W / 2, cy = H / 2, R = 92;
  const title = p => (all.find(x => x.path === p)?.title) || p.replace(/^\/types\//, "").replace(/\.md$/, "");
  const pos = others.map((p, i) => { const a = (2 * Math.PI * i) / Math.max(others.length, 1) - Math.PI / 2; return [p, cx + R * Math.cos(a), cy + R * Math.sin(a)]; });
  const edges = pos.map(([p, x, y]) => {
    const out = rels.filter(r => r.to === p).map(r => `${r.type}${r.cardinality ? ` (${r.cardinality})` : ""}`);
    const inn = back.find(b => b.path === p) ? (back.find(b => b.path === p).relations || []).filter(r => r.to === t.path).map(r => `← ${r.type}`) : [];
    return `<line x1="${cx}" y1="${cy}" x2="${x}" y2="${y}" class="g-e"/><text x="${(cx + x) / 2}" y="${(cy + y) / 2 - 4}" class="g-l">${esc([...out, ...inn].join(", "))}</text>`;
  }).join("");
  const node = (x, y, label, main) => `<g><rect x="${x - 52}" y="${y - 14}" width="104" height="28" rx="8" class="${main ? "g-n g-main" : "g-n"}"/><text x="${x}" y="${y + 4}" text-anchor="middle" class="g-t">${esc(label.length > 15 ? label.slice(0, 14) + "…" : label)}</text></g>`;
  return `<svg class="ad-graph" viewBox="0 0 ${W} ${H}" role="img" aria-label="${esc(t.title)} and the types it relates to">${edges}${pos.map(([p, x, y]) => node(x, y, title(p), false)).join("")}${node(cx, cy, t.title || "", true)}</svg>`;
}

// Knowledge › Ontology: the types on the left, searchable; the chosen one
// on the right with its definition, attributes, relations, graph, entries
// and history; create, edit, relations, merge, deprecate, delete.
async function ontologyView(box, id) {
  if (!$("#onto-list", box)) {
    box.innerHTML = `<div class="ad-policy"><aside><div class="row ad-bar"><input id="onto-q" type="search" placeholder="Find a type" aria-label="Find a type" autocomplete="off"><button class="primary" id="onto-new">New type</button></div><ul class="ad-list" id="onto-list"></ul></aside><section id="onto-detail"></section></div>`;
    $("#onto-q", box).oninput = () => drawTypes(box);
    $("#onto-new", box).onclick = () => newType(box);
  }
  UI.loading($("#onto-list", box), "the types");
  try { box.types = (await api("GET", "/ontology/types")).documents; } catch (e) { return UI.error($("#onto-list", box), "The types could not be read.", e, () => ontologyView(box, id)); }
  box.cur = id || box.types[0]?.id?.replace(/^types~/, "");
  drawTypes(box);
  typeDetail(box);
}

function drawTypes(box) {
  const q = $("#onto-q", box).value.trim().toLowerCase();
  const l = box.types.filter(t => !q || `${t.title} ${t.description} ${t.id}`.toLowerCase().includes(q));
  $("#onto-list", box).innerHTML = l.length ? l.map(t => { const k = t.id.replace(/^types~/, ""); return `<li><a href="#/knowledge/ontology/${esc(k)}" ${k === box.cur ? 'aria-current="page"' : ""}>${esc(t.title || k)}</a> ${t.status ? `<span class="chip ${t.status === "stable" ? "ok" : t.status === "deprecated" ? "" : "warn"}">${esc(t.status)}</span>` : ""}<div class="small muted">${esc(t.version)} version${t.version === 1 ? "" : "s"}</div></li>`; }).join("")
    : `<li>${UI.empty(box.types.length ? "No type matches." : "Agents have defined no types yet.")}</li>`;
}

async function typeDetail(box) {
  const el = $("#onto-detail", box);
  if (!box.cur) { el.innerHTML = UI.empty("No type chosen."); return; }
  UI.loading(el, "the type");
  let d;
  try { d = await api("GET", `/ontology/types/${box.cur}`); } catch (e) { return UI.error(el, "The type could not be read.", e, () => typeDetail(box)); }
  const fm = d.frontmatter || {}, rels = d.relations || [], again = () => ontologyView(box, box.cur);
  const others = box.types.filter(t => t.path !== d.path);
  el.innerHTML = `<div class="spread"><h2>${esc(fm.title || box.cur)}</h2><span class="chip ${fm.status === "stable" ? "ok" : fm.status === "deprecated" ? "" : "warn"}">${esc(fm.status || "")}</span></div>
    <p>${esc(fm.description || "")}</p>
    <dl class="kv small"><dt>Attributes</dt><dd>${(fm.identity_keys || []).map(k => `<span class="mono">${esc(k)}</span>`).join(", ") || "–"} <span class="muted">(identity keys)</span></dd>
      <dt>System of record</dt><dd>${esc(fm.system_of_record ?? "–")}</dd><dt>Cedar entity</dt><dd class="mono">${esc(fm.cedar_entity ?? "–")}</dd>
      <dt>Entries</dt><dd>${d.entities.length} ${d.entities.length ? `<a href="#/knowledge/memory">in Memory</a>` : ""}</dd><dt>Referred to by</dt><dd>${d.referenced_by.length ? d.referenced_by.map(r => `<span class="mono">${esc(r.path)}</span>`).join(", ") : "nothing"}</dd></dl>
    ${typeGraph({ ...d, title: fm.title, path: d.path, relations: rels }, box.types)}
    <h3>Relations</h3>
    <div class="ad-scroll"><table class="t small"><thead><tr><th>Name</th><th>To</th><th>Cardinality</th><th></th></tr></thead><tbody>${rels.map((r, i) => `<tr><td class="mono">${esc(r.type)}</td><td>${esc(r.to ? (box.types.find(t => t.path === r.to)?.title || r.to) : "–")}</td><td>${esc(r.cardinality || "–")}</td><td><button class="link small" data-rel-rm="${i}">Remove</button></td></tr>`).join("") || `<tr><td colspan="4" class="muted">None.</td></tr>`}</tbody></table></div>
    <form id="rel-add" class="row ad-bar"><input name="type" placeholder="relation, e.g. owned_by" required autocomplete="off" aria-label="Relation name"><select name="to" aria-label="To">${others.map(t => `<option value="${esc(t.path)}">${esc(t.title || t.path)}</option>`).join("")}</select><select name="card" aria-label="Cardinality"><option value="one">one</option><option value="many">many</option></select><button>Add relation</button></form>
    <div class="row ad-bar">${fm.status !== "stable" ? `<button id="ty-stable" class="primary">Approve as stable</button>` : ""}${fm.status !== "deprecated" ? `<button id="ty-dep">Deprecate</button>` : `<button id="ty-draft">Bring back as draft</button>`}
      <button id="ty-merge">Merge into…</button><button id="ty-del" class="danger">Delete</button></div>
    <h3>Edit</h3><div id="ty-edit"></div>
    <h3>History</h3><div id="ty-hist">${versionsHtml(d)}</div>`;
  const setRels = (list, what) => act("PATCH", `/ontology/types/${box.cur}`, { relations: list }, () => { UI.toast(what, () => act("PATCH", `/ontology/types/${box.cur}`, { relations: rels }, again)); again(); });
  for (const b of $$("[data-rel-rm]", el)) b.onclick = () => setRels(rels.filter((_, i) => String(i) !== b.dataset.relRm), "Relation removed.");
  $("#rel-add", el).onsubmit = e => { e.preventDefault(); const f = new FormData(e.target); setRels([...rels, { type: f.get("type").trim(), to: f.get("to"), cardinality: f.get("card") }], "Relation added."); };
  const status = (st, msg) => act("PATCH", `/ontology/types/${box.cur}`, { status: st }, () => { UI.toast(msg, () => act("PATCH", `/ontology/types/${box.cur}`, { status: fm.status }, again)); again(); });
  $("#ty-stable", el)?.addEventListener("click", () => status("stable", "Approved as stable: agents may rely on it."));
  $("#ty-dep", el)?.addEventListener("click", () => status("deprecated", "Deprecated: agents stop using it for new entries."));
  $("#ty-draft", el)?.addEventListener("click", () => status("draft", "Back as a draft."));
  $("#ty-merge", el).onclick = () => mergeType(box, d, fm);
  $("#ty-del", el).onclick = async () => {
    if (await UI.confirm({ title: `Delete the type ${fm.title}?`, body: d.entities.length || d.referenced_by.length ? "It is in use: merge it into another type, or move its entries, first." : "Its versions stay in the log, to roll back to.", verb: "Delete", danger: true, typed: box.cur }))
      act("DELETE", `/ontology/types/${box.cur}`, null, () => { UI.toast(`${fm.title} is deleted.`); location.hash = "#/knowledge/ontology"; ontologyView(box); });
  };
  const ed = okfEditor(d.text, async text => { await api("PATCH", `/ontology/types/${box.cur}`, { text }); UI.toast("Saved as a new version."); again(); });
  $("#ty-edit", el).innerHTML = ed.html;
  ed.bind(el);
  bindVersions($("#ty-hist", el), d, `/ontology/types/${box.cur}/rollback`, again);
}

async function mergeType(box, d, fm) {
  const others = box.types.filter(t => t.path !== d.path);
  if (!others.length) { UI.toast("There is no other type to merge it into."); return; }
  const dlg = document.createElement("dialog");
  dlg.className = "ad-sheet";
  dlg.innerHTML = `<form method="dialog"><h2>Merge ${esc(fm.title)} into…</h2><p class="small">Each of its entries becomes the other type's, every relation to it points to the other, and it goes. Each step is a version, so each can be rolled back.</p>
    <label>Into <select name="into">${others.map(t => `<option value="${esc(t.id.replace(/^types~/, ""))}">${esc(t.title || t.id)}</option>`).join("")}</select></label>
    <div class="row ad-sheet-actions"><button value="no" formnovalidate>Cancel</button><button value="yes" class="primary">Merge</button></div></form>`;
  document.body.append(dlg);
  dlg.addEventListener("close", () => {
    const into = $("select", dlg).value, yes = dlg.returnValue === "yes";
    dlg.remove();
    if (yes) act("POST", "/ontology/types/merge", { from: box.cur, into }, v => { UI.toast(`Merged: ${v?.entities_moved ?? 0} entries moved, ${v?.documents_repointed ?? 0} documents repointed.`); location.hash = `#/knowledge/ontology/${into}`; });
  });
  dlg.showModal();
}

function newType(box) {
  const tmpl = `---\ntype: Entity Type\ntitle: \ndescription: \nscope: org\ngenerated:\n  by: human:${Admin.me.sub}\n  at: "${new Date().toISOString()}"\nstatus: draft\nsystem_of_record: \nidentity_keys: []\nrelations: []\ncedar_entity: \n---\n\n`;
  const dlg = document.createElement("dialog");
  dlg.className = "ad-sheet ad-wide";
  dlg.innerHTML = `<form><h2>A new type</h2><label>Its id <input name="id" required pattern="[a-z0-9_-]+" autocomplete="off" placeholder="customer"></label><label>The type <textarea name="text" rows="14" class="mono ad-okf" spellcheck="false">${esc(tmpl)}</textarea></label><p class="error" hidden></p><div class="row ad-sheet-actions"><button type="button" id="nt-cancel">Cancel</button><button class="primary">Create</button></div></form>`;
  document.body.append(dlg);
  dlg.addEventListener("close", () => dlg.remove());
  $("#nt-cancel", dlg).onclick = () => dlg.close();
  $("form", dlg).onsubmit = async e => {
    e.preventDefault();
    const f = new FormData(e.target);
    try { await api("POST", "/ontology/types", { id: f.get("id"), text: f.get("text") }); dlg.close(); UI.toast("Created as a draft."); location.hash = `#/knowledge/ontology/${f.get("id")}`; }
    catch (err) { const p = $(".error", dlg); p.textContent = err.message; p.hidden = false; }
  };
  dlg.showModal();
}

// Knowledge › Context: what every agent receives (the map, who does what,
// the conventions, the sources), each edited in place; the map as one
// person's agent sees it.
const CONTEXT = [["map", "The map"], ["structure", "Who does what"], ["conventions", "Terminology and standing facts"], ["sources", "Sources"]];

async function contextView(box) {
  box.innerHTML = `<div class="ad-policy"><section id="ctx">${CONTEXT.map(([k, l]) => `<h2>${l}</h2><div data-sec="${k}"></div>`).join("")}</section>
    <aside class="card flat"><h2>As agents see it</h2><label>For <select id="ctx-person"></select></label><pre class="ad-json" id="ctx-seen"></pre></aside></div>`;
  for (const [k] of CONTEXT) loadSection(box, k);
  const sel = $("#ctx-person", box);
  try {
    const p = await api("GET", "/policy/sets");
    sel.innerHTML = (p.people || []).map(x => `<option value="${esc(x.id)}">${esc(x.name)}</option>`).join("");
  } catch { /* the default person */ }
  const seen = async () => {
    const out = $("#ctx-seen", box);
    try { const v = await api("GET", `/context/assembled?person=${encodeURIComponent(sel.value || "")}`); out.textContent = v.text ?? "There is no map yet: agents receive no org context."; }
    catch (e) { out.textContent = e.message; }
  };
  sel.onchange = seen;
  seen();
}

async function loadSection(box, sec) {
  const el = $(`[data-sec="${sec}"]`, box);
  UI.loading(el, "it");
  let v;
  try { v = await api("GET", `/context/${sec}`); } catch (e) { return UI.error(el, "It could not be read.", e, () => loadSection(box, sec)); }
  el.innerHTML = v.documents.length ? `<ul class="ad-list">${v.documents.map(d => `<li class="card flat"><div class="spread"><strong>${esc(d.title || d.path)}</strong><button class="link small" data-edit="${esc(d.id)}">Edit</button></div>
      ${d.description ? `<div class="small">${esc(d.description)}</div>` : ""}<div class="small muted"><span class="mono">${esc(d.path)}</span> · changed by ${esc(byWho(d.changed_by))} ${esc(when(d.changed_at))}</div><div data-ed="${esc(d.id)}"></div></li>`).join("")}</ul>`
    : UI.empty("Nothing here yet.", "Agents write it as they learn the organisation.");
  for (const b of $$("[data-edit]", el)) b.onclick = async () => {
    const slot = $(`[data-ed="${b.dataset.edit}"]`, el);
    if (slot.innerHTML) { slot.innerHTML = ""; return; }
    const d = await act("GET", `/knowledge/documents/${b.dataset.edit}`);
    if (!d) return;
    const ed = okfEditor(d.text, async text => { await api("PATCH", `/context/${sec}`, { id: d.id, text }); UI.toast("Saved as a new version."); loadSection(box, sec); });
    slot.innerHTML = ed.html;
    ed.bind(slot);
  };
}

// Knowledge › Memory: the entries, searched and filtered; a row opens the
// entry with its provenance and versions; edit, delete, roll back; and in
// bulk, with a fresh password.
async function memoryView(box, id) {
  if (!$("#mem-rows", box)) {
    box.innerHTML = `<form class="row ad-bar" id="mem-f" role="search">
        <input name="q" type="search" placeholder="Search the memory" aria-label="Search the memory" autocomplete="off">
        <select name="source" aria-label="Source"><option value="">Any source</option></select>
        <select name="agent" aria-label="Written by"><option value="">Anyone</option></select>
        <select name="entity" aria-label="Linked to"><option value="">Linked to anything</option></select>
        <label class="row small">Since <input name="since" type="date"></label></form>
      <div class="row ad-bar" id="mem-bulk" hidden><span class="small" id="mem-n"></span><button id="mem-del" class="danger">Delete…</button><button id="mem-back">Roll back…</button></div>
      <div id="mem-rows"></div>`;
    $("#mem-f", box).oninput = () => drawMemory(box);
    $("#mem-f", box).onsubmit = e => e.preventDefault();
    $("#mem-del", box).onclick = () => memoryBulk(box, "delete");
    $("#mem-back", box).onclick = () => memoryBulk(box, "rollback");
    await loadMemory(box);
  }
  if (!id) return UI.closePanel();
  memoryPanel(box, id);
}

async function loadMemory(box) {
  const el = $("#mem-rows", box);
  UI.loading(el, "the memory");
  try { box.mem = (await api("GET", "/memory")).documents; } catch (e) { return UI.error(el, "The memory could not be read.", e, () => loadMemory(box)); }
  box.picked = new Set();
  const uniq = xs => [...new Set(xs.filter(Boolean))].sort();
  const opt = (sel, xs, label) => { $(sel, box).innerHTML = `<option value="">${label}</option>${xs.map(x => `<option>${esc(x)}</option>`).join("")}`; };
  opt('[name="source"]', uniq(box.mem.flatMap(m => m.sources || [])), "Any source");
  opt('[name="agent"]', uniq(box.mem.map(m => m.changed_by)), "Anyone");
  opt('[name="entity"]', uniq(box.mem.flatMap(m => (m.relations || []).map(r => r.to))), "Linked to anything");
  drawMemory(box);
}

function drawMemory(box) {
  const f = Object.fromEntries(new FormData($("#mem-f", box)));
  const q = (f.q || "").toLowerCase();
  const rows = box.mem.filter(m => (!q || `${m.title} ${m.description} ${m.path}`.toLowerCase().includes(q)) && (!f.source || (m.sources || []).includes(f.source))
    && (!f.agent || m.changed_by === f.agent) && (!f.entity || (m.relations || []).some(r => r.to === f.entity)) && (!f.since || (m.changed_at && m.changed_at >= f.since)));
  const el = $("#mem-rows", box);
  el.innerHTML = rows.length ? `<div class="ad-scroll"><table class="t small"><thead><tr><th><input type="checkbox" id="mem-all" aria-label="Choose all shown"></th><th>Summary</th><th>Linked to</th><th>Source</th><th>Created</th><th>Last used</th></tr></thead><tbody>${rows.map(m => `<tr class="click" data-href="#/knowledge/memory/${esc(m.id)}" tabindex="0">
      <td><input type="checkbox" data-pick="${esc(m.id)}" ${box.picked.has(m.id) ? "checked" : ""} aria-label="Choose ${esc(m.title || m.path)}"></td>
      <td><strong>${esc(m.title || m.path)}</strong><div class="muted">${esc(m.type || "")}</div></td>
      <td>${(m.relations || []).map(r => `<span class="mono">${esc(r.type)} → ${esc(String(r.to).replace(/^\/entities\//, ""))}</span>`).join("<br>") || "–"}</td>
      <td class="ad-wrap">${(m.sources || []).map(esc).join("<br>") || "–"}</td><td>${esc(when(m.created))}</td><td>${esc(m.last_used ? when(m.last_used) : "never")}</td></tr>`).join("")}</tbody></table></div>`
    : UI.empty(box.mem.length ? "No entry matches." : "The organisation's memory is empty.", box.mem.length ? "Clear a filter to see more." : "Agents add to it as they work.");
  rowLinks(el);
  const bulk = () => { $("#mem-bulk", box).hidden = !box.picked.size; $("#mem-n", box).textContent = `${box.picked.size} chosen`; };
  for (const c of $$("[data-pick]", el)) c.onchange = () => { if (c.checked) box.picked.add(c.dataset.pick); else box.picked.delete(c.dataset.pick); bulk(); };
  const all = $("#mem-all", el);
  if (all) all.onchange = () => { for (const r of rows) all.checked ? box.picked.add(r.id) : box.picked.delete(r.id); drawMemory(box); };
  bulk();
}

async function memoryBulk(box, action) {
  const ids = [...box.picked];
  if (!(await UI.confirm({ title: action === "delete" ? `Delete ${ids.length} entries?` : `Roll back ${ids.length} entries?`, body: action === "delete" ? "They leave the memory; their versions stay in the log, to bring back. It asks for your password again." : "Each goes back to its previous version. It asks for your password again.", verb: action === "delete" ? "Delete" : "Roll back", danger: action === "delete" }))) return;
  act("POST", "/memory/bulk", { action, ids }, v => {
    const done = v.results.filter(r => r.done).length, not = v.results.filter(r => !r.done);
    UI.toast(`${done} of ${ids.length} done.${not.length ? ` Not done: ${not.map(r => `${r.id.replace(/^entities~/, "")} (${r.why || "refused"})`).join(", ")}` : ""}`);
    loadMemory(box);
  });
}

async function memoryPanel(box, id) {
  const body = UI.panel("Entry", "", "#/knowledge/memory");
  UI.loading(body, "the entry");
  let d;
  try { d = await api("GET", `/memory/${id}`); } catch (e) { return UI.error(body, "The entry could not be read.", e, () => memoryPanel(box, id)); }
  const fm = d.frontmatter || {}, again = () => { loadMemory(box); memoryPanel(box, id); };
  $("#panel h2").textContent = fm.title || d.path;
  body.innerHTML = `<p class="small muted mono">${esc(d.path)}</p>${fm.description ? `<p>${esc(fm.description)}</p>` : ""}
    <h3>Where it came from</h3><dl class="kv small"><dt>Written by</dt><dd>${esc(byWho(d.versions[0]?.by))}</dd><dt>Run</dt><dd class="mono">${esc(d.versions[0]?.run || "–")}</dd><dt>When</dt><dd>${esc(when(d.versions[0]?.at))}</dd>
      <dt>Sources</dt><dd class="ad-wrap">${(d.sources || []).map(esc).join("<br>") || "–"}</dd><dt>Read under</dt><dd>${d.label?.untrusted ? "untrusted content" : "trusted content"}${d.label?.private ? ", private data" : ""}</dd><dt>Last used</dt><dd>${esc(d.last_used ? when(d.last_used) : "never")}</dd></dl>
    <h3>Linked to</h3>${(fm.relations || []).length ? `<ul class="small">${fm.relations.map(r => `<li><span class="mono">${esc(r.type)}</span> → <span class="mono">${esc(r.to)}</span></li>`).join("")}</ul>` : UI.empty("Nothing.")}
    <h3>The entry</h3><div id="mem-ed"></div>
    <div class="row ad-bar"><button class="danger" id="mem-rm">Delete</button></div>
    <h3>Versions</h3><div id="mem-hist">${versionsHtml(d)}</div>`;
  const ed = okfEditor(d.text, async text => { await api("PATCH", `/memory/${id}`, { text }); UI.toast("Saved as a new version."); again(); });
  $("#mem-ed", body).innerHTML = ed.html;
  ed.bind(body);
  $("#mem-rm", body).onclick = async () => {
    if (await UI.confirm({ title: "Delete this entry?", body: "It leaves the memory; its versions stay in the log, to bring back.", verb: "Delete", danger: true }))
      act("DELETE", `/memory/${id}`, null, () => { UI.toast("Deleted."); location.hash = "#/knowledge/memory"; loadMemory(box); });
  };
  bindVersions($("#mem-hist", body), d, `/memory/${id}/rollback`, again);
}

// Knowledge › Activity: every change agents make, newest first, with its
// before and after; one undone, or a selection as a batch with a fresh
// password. Kept live by the stream.
async function activityView(box, seq) {
  if (!$("#act-rows", box)) {
    box.innerHTML = `<form class="row ad-bar" id="act-f" role="search">
        <select name="agent" aria-label="By"><option value="">Every agent</option></select>
        <select name="area" aria-label="Area"><option value="">Every area</option>${["type", "entity", "convention", "people", "source", "index", "log"].map(a => `<option>${a}</option>`).join("")}</select>
        <select name="change" aria-label="Change"><option value="">Every change</option><option>created</option><option>updated</option><option>removed</option></select>
        <label class="row small">Since <input name="since" type="date"></label>
        <label class="row small"><input type="checkbox" name="everyone"> Admins' changes too</label></form>
      <div class="row ad-bar" id="act-bulk" hidden><span class="small" id="act-n"></span><button id="act-undo" class="danger">Undo these…</button></div>
      <div id="act-rows"></div>`;
    $("#act-f", box).oninput = e => (e.target.name === "everyone" ? loadActivity(box) : drawActivity(box));
    $("#act-undo", box).onclick = () => undoActivity(box, [...box.picked], true);
    await loadActivity(box);
    const on = e => {
      if (!document.body.contains(box)) return document.removeEventListener("ad-status", on);
      if (box.last !== undefined && e.detail.knowledge_last_change !== box.last) loadActivity(box);
      box.last = e.detail.knowledge_last_change;
    };
    document.addEventListener("ad-status", on);
  }
  if (!seq) return UI.closePanel();
  changePanel(box, seq);
}

async function loadActivity(box) {
  const el = $("#act-rows", box);
  if (!box.acts) UI.loading(el, "the activity");
  const everyone = $('[name="everyone"]', box).checked ? "1" : "0";
  try { box.acts = (await api("GET", `/activity?everyone=${everyone}`)).changes; } catch (e) { return UI.error(el, "The activity could not be read.", e, () => loadActivity(box)); }
  box.picked = box.picked || new Set();
  const agents = [...new Set(box.acts.map(c => c.by))].sort();
  const sel = $('[name="agent"]', box), was = sel.value;
  sel.innerHTML = `<option value="">Every agent</option>${agents.map(a => `<option value="${esc(a)}">${esc(byWho(a))}</option>`).join("")}`;
  sel.value = was;
  drawActivity(box);
}

function drawActivity(box) {
  const f = Object.fromEntries(new FormData($("#act-f", box)));
  const rows = box.acts.filter(c => (!f.agent || c.by === f.agent) && (!f.area || c.area === f.area) && (!f.change || c.change === f.change) && (!f.since || String(c.at) >= f.since));
  const el = $("#act-rows", box);
  el.innerHTML = rows.length ? `<ul class="ad-feed">${rows.map(c => `<li class="card flat"><label class="row"><input type="checkbox" data-pick="${c.seq}" ${box.picked.has(c.seq) ? "checked" : ""} aria-label="Choose this change"></label>
      <div class="ad-feed-body"><div><span class="chip ${c.change === "removed" ? "bad" : c.change === "created" ? "ok" : ""}">${esc(c.change)}</span> <a href="#/knowledge/activity/${c.seq}" class="mono">${esc(c.path)}</a></div>
      <div class="small muted">${esc(byWho(c.by))} · ${esc(when(c.at))}${c.run ? ` · run <span class="mono">${esc(c.run)}</span>` : ""}${(c.sources || []).length ? ` · from ${c.sources.map(esc).join(", ")}` : ""}</div></div>
      <button class="link small" data-undo="${c.seq}">Undo</button></li>`).join("")}</ul>`
    : UI.empty(box.acts.length ? "No change matches." : "Agents have changed nothing yet.");
  const bulk = () => { $("#act-bulk", box).hidden = !box.picked.size; $("#act-n", box).textContent = `${box.picked.size} chosen`; };
  for (const c of $$("[data-pick]", el)) c.onchange = () => { const s = Number(c.dataset.pick); if (c.checked) box.picked.add(s); else box.picked.delete(s); bulk(); };
  for (const b of $$("[data-undo]", el)) b.onclick = () => undoActivity(box, [Number(b.dataset.undo)], false);
  bulk();
}

async function undoActivity(box, seqs, batch) {
  if (!(await UI.confirm({ title: batch ? `Undo ${seqs.length} changes?` : "Undo this change?", body: `Each document goes back to how it was before ${batch ? "the earliest of them" : "it"}, as a new version.${batch ? " It asks for your password again." : ""}`, verb: "Undo" }))) return;
  act("POST", batch ? "/activity/revert-batch" : `/activity/${seqs[0]}/revert`, batch ? { seqs } : null, v => {
    UI.toast(`Undone: ${(v?.restored || []).map(r => r.path).join(", ")}.`);
    box.picked = new Set();
    loadActivity(box);
  });
}

async function changePanel(box, seq) {
  const body = UI.panel("Change", "", "#/knowledge/activity");
  UI.loading(body, "the change");
  let c;
  try { c = await api("GET", `/activity/${seq}`); } catch (e) { return UI.error(body, "The change could not be read.", e, () => changePanel(box, seq)); }
  $("#panel h2").textContent = c.path;
  body.innerHTML = `<p class="small">${esc(byWho(c.by))} · ${esc(when(c.at))}${c.run ? ` · run <span class="mono">${esc(c.run)}</span>` : ""}</p>${(c.sources || []).length ? `<p class="small">From ${c.sources.map(s => `<span class="mono">${esc(s)}</span>`).join(", ")}</p>` : ""}
    <h3>Before and after</h3>${c.before == null ? `<p class="small muted">New: it did not exist before.</p>` : ""}${c.after == null ? `<p class="small muted">Removed.</p>` : ""}${diffHtml(diff(c.before || "", c.after || ""))}
    <div class="row ad-bar"><button class="primary" id="ch-undo">Undo this change</button></div>`;
  $("#ch-undo", body).onclick = () => undoActivity(box, [seq], false);
}

// ---- Sections ----------------------------------------------------------

const SECTIONS = {
  overview: view => overviewView(view),
  users: (view, rest) => usersView(view, rest[0], rest[1]),
  security: (view, rest) => securityView(view, rest),
  operations: (view, rest) => operationsView(view, rest),
  knowledge: (view, rest) => knowledgeView(view, rest),
};

function section() {
  const parts = (location.hash.replace(/^#\/?/, "") || "overview").split("/");
  const name = parts[0] in SECTIONS ? parts[0] : "overview";
  for (const a of $$("#nav a")) {
    if (a.dataset.section === name) a.setAttribute("aria-current", "page"); else a.removeAttribute("aria-current");
  }
  let view = $("#view");
  // A different section starts its screen afresh; within one, the list
  // stays and only the panel changes.
  const keep = ["users", "security", "operations", "knowledge"].includes(name);
  if (!view || view.dataset.section !== `${name}/${parts[1] && !keep ? parts[1] : ""}`) {
    view?.remove();
    UI.closePanel();
    view = document.createElement("section");
    view.id = "view";
    view.dataset.section = `${name}/${parts[1] && !keep ? parts[1] : ""}`;
    $("#main").prepend(view);
  }
  const draw = SECTIONS[name];
  if (draw) return draw(view, parts.slice(1));
  const title = $(`#nav a[data-section="${name}"]`)?.textContent || "Overview";
  view.innerHTML = `<h1>${esc(title)}</h1>${UI.empty("Not built yet.", "docs/admin-console.md lists what this section holds when it is done.")}`;
}

async function session() {
  try {
    const r = await fetch("/api/v1/session", { credentials: "same-origin" });
    if (!r.ok) return signedOut();
    Admin.me = await r.json();
  } catch { return signedOut(); }
  document.body.classList.add("signed-in");
  $("#sign-in").hidden = true;
  $("#nav").hidden = false;
  menu();
  await loadNames();
  section();
  killSwitch();
  live();
}

addEventListener("hashchange", () => { if (Admin.me) section(); });
addEventListener("DOMContentLoaded", () => {
  status();
  session();
  setInterval(status, 15000);
});
