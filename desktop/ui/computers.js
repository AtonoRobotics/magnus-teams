// The person's cloud computers (status, preview, takeover, files, activity)
// and this computer, connected through the desktop app.
UI.computer = '<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>';
const PREVIEW_EVERY_MS = 3000, EDITABLE_BYTES = 256 * 1024, FILES_ROOT = "/workspace/";

route(/^#\/computer$/, main => showComputers(main));
route(/^#\/computer\/([^/]+)(?:\/([a-z]+))?$/, (main, id, tab) => showComputer(main, decodeURIComponent(id), tab || "screen"));

const stillAt = hash => location.hash === hash;
const bytesOf = b64 => Uint8Array.from(atob(b64), c => c.charCodeAt(0));
const b64Of = bytes => { let s = ""; for (const b of bytes) s += String.fromCharCode(b); return btoa(s); };
// Asleep or stopped, any use wakes it: nobody starts a computer by hand. One
// pill says how it stands and who is using it; the line after it, for whom.
const stateChip = c => {
  const w = controlWords(c, false), runs = (c.driving?.runs || []).filter(r => r.status === "running"), whom = workingForWhom(c);
  const tasks = runs.map(r => `<a href="#/task/${enc(r.task)}" data-open="task:${esc(r.task)}">${esc(taskName(r.task))}</a>`);
  const more = w.key === "working" && tasks.length ? ` <span class="small">${whom.length ? `for ${esc(andList(whom))} ` : ""}on ${tasks.join(", ")}</span>` : w.who === "person" ? ` <span class="small">${esc(w.text)}</span>` : "";
  return `<span class="state-pill" data-pill="${w.key}">${esc(w.pill)}</span>${more}`;
};

/**
 * One computer, always there (spec rev 142): the person's agent has exactly
 * one, made with their account, and each workspace's team agent has one.
 * Nobody makes or picks one; the list is the person's own and, for an admin,
 * those of the workspaces they look after.
 * @param {Array<{ id: string, name?: string, owner?: { person?: string, workspace?: string }, queue_position?: number }>} computers as GET /v1/computers lists them
 */
function cloudComputers(computers) {
  const row = c => `<tr data-computer="${esc(c.id)}"><td><a href="#/computer/${enc(c.id)}">${esc(c.name || "Computer")}</a></td><td>${stateChip(c)}${c.queue_position ? ` <span class="small faint">number ${esc(c.queue_position)} in line</span>` : ""}</td><td><button data-open="computer:${esc(c.id)}">Pin beside the conversation</button></td></tr>`;
  const mine = computers.filter(c => c.owner?.person), teams = computers.filter(c => c.owner?.workspace);
  return `<h2>Your computer</h2>
    ${mine.length ? `<table class="t"><tbody>${mine.map(row).join("")}</tbody></table>` : `<p class="empty">Your computer is being set up. It is ready the first time it is needed.</p>`}
    <p class="small muted">It is always there: it sleeps when no one uses it and wakes whenever ${esc(agentName({}))} or you need it.</p>
    ${teams.length ? `<h2>Team computers</h2><table class="t"><tbody>${teams.map(row).join("")}</tbody></table>` : ""}`;
}

async function showComputers(main) {
  const hash = location.hash;
  const draw = async () => {
    let computers = [], unavailable = "";
    try { computers = (await api("GET", "/v1/computers")).computers; } catch (e) { unavailable = e.message; }
    const here = await thisComputerSection();
    if (!stillAt(hash)) return;
    paint(main, `<h1>Computers</h1>
      ${unavailable ? `<h2>Your computer</h2><p class="muted">${esc(unavailable)}</p>` : cloudComputers(computers)}
      ${here}`);
  };
  main.onclick = async e => {
    const b = e.target.closest("[data-act]");
    if (!b) return;
    try {
      if (await thisComputerAction(b, main)) await draw();
    } catch (err) { toast(err.message); }
  };
  await draw();
}

// The page's policy forbids a form from going anywhere: forms here are read
// by their own handlers.
document.addEventListener("submit", e => { if (e.target.closest("#main")) e.preventDefault(); });

// ---------- this computer ----------

/** Per computer, the person picks how Magnus may work there (spec, Work on the user's desktop: Modes). */
const LOCAL_MODES = { ask: "Asks each time", allow: "Allowed", never: "Never" };
const LOCAL_ASKS = { ask: "Ask me every time", allow: "Allow it", never: "Never" };

async function thisComputerSection() {
  const [known, running] = await Promise.all([api("GET", "/v1/local/computers").catch(() => []), Magnus.localState()]);
  // A team's or the admin's ceiling can make the mode stricter; the page says the one that holds.
  const modeWords = (c, held) => held && held !== c.mode ? `${LOCAL_MODES[held] || held}: your team or admin allows no more than this` : LOCAL_MODES[c.mode] || c.mode;
  const rows = known.map(({ computer: c, online, in_use, in_force }) => `<tr data-local="${esc(c.id)}"><td>${esc(c.name)}<div class="small faint">${esc(c.folders.join(", "))}</div></td>
    <td><span class="chip ${online ? (in_use ? "warn" : "good") : ""}">${online ? (in_use ? "You are using it" : "Online") : "Offline"}</span> <span class="small faint" data-mode>${esc(modeWords(c, in_force))}</span>${online && in_use ? `<div class="small muted">Magnus waits while you use the mouse or keyboard, and goes on when you are idle.</div>` : ""}</td>
    <td><div class="row">${online && in_use ? `<button data-act="local-resume" data-id="${esc(c.id)}">Let Magnus go on now</button>` : ""}<button class="link" data-act="local-remove" data-id="${esc(c.id)}">Disconnect</button></div></td></tr>`).join("");
  const here = running?.running ? known.find(k => k.computer.name === running.name)?.computer : null;
  const modes = mode => Object.entries(LOCAL_ASKS).map(([v, l]) => `<option value="${v}"${v === mode ? " selected" : ""}>${l}</option>`).join("");
  const connect = !running ? "" : running.running
    ? `<div class="card flat"><p>This computer is connected as <b>${esc(running.name)}</b>${running.paused ? ". Magnus is paused here." : "."}</p><div class="row"><button data-act="local-pause" data-to="${running.paused ? "" : "1"}">${running.paused ? "Let Magnus work" : "Pause Magnus here"}</button><button class="link" data-act="local-stop">Stop</button></div>
        ${here ? `<form class="local-connect"><label>When Magnus wants to work here<select id="lc-mode">${modes(here.mode)}</select></label>
          <label>Folders it may use, one per line<textarea id="lc-folders" rows="3">${esc(here.folders.join("\n"))}</textarea></label>
          <div class="row"><button type="button" data-act="local-change" data-name="${esc(running.name)}">Save</button></div></form>` : ""}</div>`
    : `<form class="card flat local-connect"><h3>Connect this computer</h3><p class="small muted">Magnus works here only inside the folders you name, only while this app is open, and pauses while you use the mouse or keyboard.</p>
        <label>Name<input id="lc-name" value="${esc(running.suggested_name || "")}"></label>
        <label>Folders, one per line<textarea id="lc-folders" rows="3" placeholder="/home/me/Projects"></textarea></label>
        <label>When Magnus wants to work here<select id="lc-mode">${modes("ask")}</select></label>
        <div class="row"><button type="button" class="primary" data-act="local-connect">Connect</button></div></form>`;
  return `<h2>This computer</h2>${rows ? `<table class="t"><tbody>${rows}</tbody></table>` : `<p class="empty">No computer of yours is connected.</p>`}${known.map(madeThere).join("")}${connect}`;
}

/** The files Magnus wrote on one of the person's own computers, to keep or delete.
 * @param {{computer: {id: string, name: string}, online: boolean, files?: {path: string, at: string}[]}} k */
function madeThere({ computer: c, online, files = [] }) {
  if (!files.length) return "";
  const id = esc(c.id);
  return `<div class="card flat" data-local-files="${id}"><h3>Files Magnus made on ${esc(c.name)}</h3>
    ${online ? "" : `<p class="small muted">${esc(c.name)} is offline. Open Magnus on it to download or delete these.</p>`}
    <div class="row files-bar"><button data-act="local-files-save" data-id="${id}">Download selected</button><button data-act="local-files-delete" data-id="${id}">Delete selected</button></div>
    <table class="t files"><tbody>${files.map(f => `<tr data-path="${esc(f.path)}"><td><input type="checkbox" data-local-pick aria-label="Select ${esc(f.path)}"></td><td>${esc(f.path)}</td>
      <td class="small faint n">${esc(when(f.at))}</td><td><button class="link" data-act="local-file-save" data-id="${id}">Download</button> <button class="link danger" data-act="local-file-delete" data-id="${id}">Delete</button></td></tr>`).join("")}</tbody></table></div>`;
}

/** The paths an action on Magnus's files on this computer is for: its row, or the ones picked. */
function madePaths(b) {
  const row = b.closest("tr[data-path]");
  if (row) return [row.dataset.path];
  return $$("[data-local-pick]:checked", b.closest("[data-local-files]")).map(p => p.closest("tr").dataset.path);
}

async function madeAction(b, id) {
  const paths = madePaths(b), base = `/v1/local/computers/${enc(id)}/files`, q = p => `?path=${encodeURIComponent(p)}`;
  if (!paths.length) { toast("Pick the files first."); return false; }
  if (b.dataset.act.endsWith("-save")) {
    let failed = 0;
    for (const p of paths) { try { await Magnus.download(`${base}/raw${q(p)}`, p.split("/").pop()); } catch (e) { failed++; if (paths.length === 1) throw e; } }
    toast(failed ? `${failed} could not be downloaded.` : paths.length === 1 ? "Downloaded." : `${paths.length} files downloaded.`);
    return false;
  }
  if (!confirm(`Delete ${paths.length === 1 ? paths[0] : `${paths.length} files`} from this computer? This cannot be undone.`)) return false;
  let failed = 0;
  for (const p of paths) { try { await api("DELETE", `${base}${q(p)}`); } catch (e) { failed++; if (paths.length === 1) throw e; } }
  toast(failed ? `${failed} could not be deleted.` : "Deleted.");
  return true;
}

async function thisComputerAction(b, main) {
  const id = b.dataset.id;
  switch (b.dataset.act) {
    case "local-connect": {
      const folders = $("#lc-folders", main).value.split("\n").map(f => f.trim()).filter(Boolean);
      await Magnus.localStart({ name: $("#lc-name", main).value.trim(), mode: $("#lc-mode", main).value, folders });
      toast("Connected."); return true;
    }
    // The same connection, with a new mode or folders: the app restarts it.
    case "local-change": {
      const folders = $("#lc-folders", main).value.split("\n").map(f => f.trim()).filter(Boolean);
      await Magnus.localStart({ name: b.dataset.name, mode: $("#lc-mode", main).value, folders });
      toast("Saved."); return true;
    }
    case "local-pause": await Magnus.localPause(!!b.dataset.to); return true;
    case "local-stop": await Magnus.localStop(); return true;
    case "local-resume": await api("POST", `/v1/local/computers/${enc(id)}/resume`, {}); toast("The agent will go on."); return true;
    case "local-remove": await Magnus.localStop(); await api("DELETE", `/v1/local/computers/${enc(id)}`); return true;
    case "local-files-save": case "local-files-delete": case "local-file-save": case "local-file-delete": return madeAction(b, id);
  }
  return false;
}

// ---------- one computer ----------

// What each control on a computer does. Each returns the computer to show
// next: the same id, another one (a reset that erased it), or null to stay as
// it is. There is no delete: a reset takes its place, so the agent is never
// without its computer (spec rev 142).
const COMPUTER_ACTS = {
  start: async (base, id) => { await api("POST", `${base}/start`, {}); return id; },
  hibernate: async (base, id) => { await api("POST", `${base}/hibernate`, {}); return id; },
  // The kill switch: one step and no question, so it is as fast as it can be.
  pause: async (base, id) => {
    const r = await api("POST", `${base}/kill`, { mode: "pause" });
    toast(`Paused. ${accessWord(r.revoked)} taken back. Resume when you are ready.`);
    return id;
  },
  reset: async (base, id) => {
    const choice = await chooseReset();
    if (!choice) return null;
    const r = await api("POST", `${base}/reset`, { keep_files: choice === "keep" });
    toast(choice === "keep" ? "Reset. It is rebuilt when it is next used, with its apps installed again." : "Reset. This is a new, empty computer.");
    return r.id;
  },
};
const accessWord = n => n === 1 ? "1 access was" : n ? `${n} accesses were` : "No access was";

// Reset asks which way: keep the person's files, or erase them too.
function chooseReset() {
  return new Promise(done => {
    const d = document.createElement("dialog");
    d.className = "ask";
    d.innerHTML = `<h3>Reset this computer?</h3>
      <p>The computer is stopped and put back to a fresh system. Running programs, anything the agent changed outside your files, and every access it was given are lost. Its apps are installed again when it next starts.</p>
      <p>You choose what happens to your files in /workspace:</p>
      <div class="row"><button data-ask="no">Back</button><button class="primary" data-ask="keep">Reset and keep my files</button><button class="danger" data-ask="erase">Reset and erase my files</button></div>
      <p class="small muted">Erasing your files also deletes the computer's backups and the passwords and tokens the vault keeps for it, and cannot be undone. Either way you keep one computer: a fresh one takes its place.</p>`;
    d.addEventListener("click", e => { const b = e.target.closest("[data-ask]"); if (b) d.close(b.dataset.ask); });
    d.addEventListener("close", () => { d.remove(); done(["keep", "erase"].includes(d.returnValue) ? d.returnValue : null); });
    document.body.append(d);
    d.showModal();
    // Keeping the files is the default (spec rev 142).
    $("[data-ask=keep]", d).focus();
  });
}

async function showComputer(main, id, tab) {
  const hash = location.hash, base = `/v1/computers/${enc(id)}`, TABS = [["screen", "Screen"], ["files", "Files"], ["apps", "Apps"], ["activity", "Activity"]];
  const c = await api("GET", base);
  if (!stillAt(hash)) return;
  // Opening it is a use, and any use wakes it: there is no Start button
  // (spec rev 131, Lifecycle). The kill switch's pause stays until Resume.
  if (c.status !== "paused") api("POST", `${base}/wake`, {}).catch(() => { /* The screen says how it stands. */ });
  // The buttons the state allows, redrawn as the screen's look finds it
  // changed: opened asleep, the page offers what a running computer does once
  // it is. Who holds control is said once, on the screen's own strip.
  const state = c => `<div class="row">
    ${c.status === "paused" ? `<button class="primary" data-act="start">Resume</button>` : ""}
    ${c.status === "running" ? `<button data-act="hibernate">Put to sleep</button>` : ""}<button data-open="computer:${esc(id)}">Pin beside the conversation</button>
    ${c.status === "running" ? `<button class="danger" data-act="pause" title="The kill switch: freezes the computer and takes back every access it was given">Pause now</button>` : ""}
    <button data-act="reset">Reset…</button></div>
    ${takeoverAsk(c)}`;
  const head = c => `<p class="small"><a href="#/computer">Computers</a></p><h1>${esc(c.name || "Computer")}</h1><div data-head>${state(c)}</div><nav class="tabs" aria-label="Computer">${TABS.map(([k, l]) => `<a href="#/computer/${enc(id)}/${k}" ${k === tab ? 'aria-current="page"' : ""}>${l}</a>`).join("")}</nav><div id="computer-tab"></div>`;
  paint(main, head(c));
  let drawn = state(c);
  const restate = now => { const el = $("[data-head]", main), html = state(now); if (el && html !== drawn) { drawn = html; el.innerHTML = html; } };
  const body = $("#computer-tab", main);
  main.onclick = async e => {
    const b = e.target.closest("[data-act]");
    if (!b || !COMPUTER_ACTS[b.dataset.act]) return;
    let next = id;
    try { next = await COMPUTER_ACTS[b.dataset.act](base, id); } catch (err) { return toast(err.message); }
    if (next === null) return;
    if (next !== id) { location.hash = next ? `#/computer/${enc(next)}` : "#/computer"; return; }
    main._html = null;
    showComputer(main, id, tab);
  };
  if (tab === "files") return computerFiles(body, id, base, hash);
  if (tab === "apps") { body.innerHTML = computerApps(c); return; }
  if (tab === "activity") {
    const { activity } = await api("GET", `${base}/activity`);
    body.innerHTML = activity.length ? `<table class="t"><tbody>${[...activity].reverse().map(a => `<tr><td>${esc(a.what)}</td><td class="small faint">${esc(a.who)}</td><td class="small faint n">${esc(when(a.at))}</td></tr>`).join("")}</tbody></table>` : `<p class="empty">Nothing has happened on this computer yet.</p>`;
    return;
  }
  computerScreen(body, c, base, hash, { onState: restate });
}

/**
 * The apps the agent installed, from the manifest kept outside the computer
 * (spec, Installing and using apps): what each is, where it came from and
 * what it may reach. A reset or rebuild puts them all back.
 * @param {{ apps?: Array<{ name: string, version: string, source: string, hosts?: string[] }>, replay_error?: string|null, driving?: object }} c
 */
function computerApps(c) {
  const apps = c.apps || [];
  return `<p class="small muted">${esc(agentName(c))} installs an app itself when a task needs one. Each is written down here, with where it came from, before it counts as installed, so a reset or a rebuild puts it back.</p>
    ${c.replay_error ? `<div class="banner bad" role="status">Putting the apps back failed: ${esc(c.replay_error)}</div>` : ""}
    ${apps.length ? `<table class="t computer-apps"><thead><tr><th>App</th><th>Version</th><th>From</th><th>May also reach</th></tr></thead><tbody>${apps.map(a => `<tr data-app="${esc(a.name)}"><td>${esc(a.name)}</td><td class="n">${esc(a.version)}</td><td class="small">${esc(a.source)}</td><td class="small faint">${esc((a.hosts || []).join(", ") || "Nothing else")}</td></tr>`).join("")}</tbody></table>`
      : `<p class="empty">No apps installed yet. It has a desktop, a browser and the usual tools.</p>`}`;
}

/**
 * The computer's screen with its control strip, on its page and pinned
 * beside the conversation alike (spec rev 131, User takeover and handback).
 * One frame holds the last screenshot, the live preview and the person's
 * own stream: it shows something whether the computer sleeps, wakes, works
 * or is in their hands, and taking or handing back control never empties
 * it. The strip always says who holds control and whether the agent is
 * paused, with Take control or I'm done, and Full screen.
 * @param {HTMLElement} body
 * @param {object} c the computer, as the server shows it
 * @param {string} base
 * @param {string|(() => boolean)} alive the page's hash, or whether the view is still shown
 * @param {{ ask?: boolean, onState?: (c: object) => void }} [opts] ask: show the agent's takeover request here too; onState: told the computer as each look finds it
 */
function computerScreen(body, c, base, alive, opts = {}) {
  if (typeof alive === "string") { const hash = alive; alive = () => stillAt(hash); }
  body.innerHTML = `<div class="computer-view" data-view><div class="control-strip" data-screen-bar role="status" aria-live="polite"></div>${opts.ask ? `<div data-ask-host></div>` : ""}<div class="screen" data-screen><img id="preview" alt="The computer's screen"><canvas id="display" tabindex="0" aria-label="The computer's screen" hidden></canvas><div class="screen-veil" data-veil hidden></div><div class="screen-hold" data-hold role="status" hidden><span data-hold-words></span><button data-done-here>I'm done</button></div><p class="screen-caption" data-caption></p></div><p class="small bad" id="display-error" hidden></p></div>`;
  const preview = $("#preview", body);
  const view = { c, alive, holding: false, live: false, done: null, looking: false, bar: "", shot: null, note: "" };
  const take = async () => {
    if (view.holding) return;
    view.holding = true;
    view.note = view.c.status === "running" ? "" : "Waking it so you can take control";
    let grant;
    try { grant = await api("POST", `${base}/takeover`, {}); } catch (e) { view.holding = false; view.note = ""; strip(); return toast(e.message); }
    view.note = "";
    takeOver(body, view.c.id, base, grant, view, strip);
  };
  // A sleeping screen wakes when clicked; an awake one is taken (the mockup).
  const wakeNow = async () => {
    view.c = { ...view.c, waking: view.c.waking || { phase: "starting" } };
    strip();
    try { await api("POST", `${base}/wake`, {}); } catch (e) { toast(e.message); }
  };
  const handBack = async () => { try { await api("POST", `${base}/handback`, {}); toast(`Handed back. ${agentName(view.c)} carries on.`); } catch (e) { toast(e.message); } };
  const done = () => view.done ? view.done() : handBack();
  // The strip, the veil, the "you are in control" bar and the caption, all
  // from the computer as last seen; only what changed is redrawn.
  const strip = () => {
    const bar = $("[data-screen-bar]", body);
    if (!bar) return;
    const c = view.c, html = controlStrip(c, view.holding);
    if (html !== view.bar) { view.bar = html; bar.innerHTML = html; }
    const t = $("[data-take]", body), d = $("[data-done]", body), f = $("[data-full]", body), ask = $("[data-ask-host]", body);
    if (t) t.onclick = take;
    if (d) d.onclick = done;
    if (f) f.onclick = () => fullScreen($("[data-view]", body));
    if (ask) ask.innerHTML = takeoverAsk(c);
    const veil = view.holding ? "" : screenVeil(c, view.shot), v = $("[data-veil]", body);
    if (v && v.html !== veil) { v.innerHTML = veil; v.html = veil; }
    if (v) v.hidden = !veil;
    const hold = $("[data-hold]", body), words = $("[data-hold-words]", body), done2 = $("[data-done-here]", body);
    if (hold) hold.hidden = !view.holding;
    if (words) words.textContent = `You're in control. ${agentName(c)} is paused.`;
    if (done2) done2.onclick = done;
    const p = $("[data-caption]", body);
    if (p) { p.hidden = false; p.textContent = view.note || screenCaption(c, view.shot, view.holding); }
    const asleep = !view.holding && c.status !== "running" && c.status !== "paused", frame = $("[data-screen]", body);
    if (frame) {
      frame.className = `screen${asleep || (c.waking && !view.holding) ? " asleep" : ""}${view.holding ? " held" : ""}${c.status === "paused" ? " still" : ""}`;
      frame.title = view.holding || c.status === "paused" ? "" : asleep && !c.waking ? "Click to wake it" : "Click to take control";
      frame.onclick = view.holding || c.status === "paused" ? null : asleep && !c.waking && c.status !== "queued" ? wakeNow : take;
    }
  };
  view.strip = strip;
  view.take = take;
  // One look loop: the computer's state for the strip, and while no live
  // stream is drawing, the screen (or, asleep, its last screenshot).
  const look = async () => {
    view.looking = true;
    while (view.alive()) {
      try {
        const [now, shot] = await Promise.all([api("GET", base).catch(() => null), view.live ? null : api("GET", `${base}/screen`)]);
        if (now?.status) view.c = now;
        if (shot?.status) { view.c = { ...view.c, status: shot.status, waking: shot.waking }; view.shot = shot; }
        if (shot?.image_b64 && !view.live) preview.src = `data:image/png;base64,${shot.image_b64}`;
        strip();
        opts.onState?.(view.c);
      } catch { /* The next look may work. */ }
      await new Promise(r => setTimeout(r, PREVIEW_EVERY_MS));
    }
    view.looking = false;
    view.leave?.();
  };
  strip();
  look();
  // Asked for from a card, a banner or the pinned screen.
  if (Screen.takeOnOpen === c.id) { Screen.takeOnOpen = null; take(); }
  return view;
}

function fullScreen(frame) {
  if (!frame) return;
  if (document.fullscreenElement) document.exitFullscreen?.(); else frame.requestFullscreen?.().catch?.(() => toast("Full screen is not available here."));
}

// The person drives in the same frame until they say they are done. The
// last preview stays until the stream draws its first frame, and comes back
// if the stream fails, so the screen never disappears (2026-11-09: it went
// blank on takeover).
function takeOver(body, id, base, grant, view, strip) {
  const preview = $("#preview", body), canvas = $("#display", body), ctx = canvas.getContext("2d");
  view.holding = true;
  view.c = { ...view.c, status: "running", controller: App.me?.id || view.c.controller, waking: null };
  let link, ended = false;
  const showLive = live => { view.live = live; canvas.hidden = !live; preview.hidden = live; };
  const failed = message => {
    const p = $("#display-error", body);
    if (p) { p.hidden = false; p.textContent = message; }
    showLive(false);
  };
  const rfb = new Rfb({
    send: b => link.send(b),
    onSize: (w, h) => { canvas.width = w; canvas.height = h; },
    onRect: (x, y, w, h, rgba) => {
      ctx.putImageData(new ImageData(rgba, w, h), x, y);
      if (!view.live) { showLive(true); canvas.focus(); }
    },
    onCopy: (x, y, w, h, fromX, fromY) => ctx.drawImage(canvas, fromX, fromY, w, h, x, y, w, h),
    onError: failed,
  });
  const gone = () => { if (!view.alive()) leave(); };
  const leave = async () => {
    if (ended) return;
    ended = true;
    window.removeEventListener("hashchange", gone);
    view.leave = null;
    link.close();
    try { await api("POST", `${base}/handback`, {}); toast(`Handed back. ${agentName(view.c)} carries on with a fresh look at the screen.`); } catch (e) { toast(e.message); }
  };
  view.leave = leave;
  link = Magnus.displayOpen(id, grant.ticket, grant.protocol_prefix, b => rfb.feed(b), reason => { if (!ended) failed(reason || "The connection to the screen ended."); });
  const at = e => { const r = canvas.getBoundingClientRect(); return [Math.round((e.clientX - r.left) * canvas.width / r.width), Math.round((e.clientY - r.top) * canvas.height / r.height)]; };
  const mask = e => (e.buttons & 1) | (e.buttons & 4 ? 2 : 0) | (e.buttons & 2 ? 4 : 0);
  for (const type of ["mousemove", "mousedown", "mouseup"]) canvas.addEventListener(type, e => { const [x, y] = at(e); rfb.pointer(x, y, mask(e)); e.preventDefault(); });
  canvas.addEventListener("wheel", e => { const [x, y] = at(e), bit = e.deltaY < 0 ? 8 : 16; rfb.pointer(x, y, bit); rfb.pointer(x, y, 0); e.preventDefault(); }, { passive: false });
  canvas.addEventListener("contextmenu", e => e.preventDefault());
  for (const [type, down] of [["keydown", true], ["keyup", false]]) canvas.addEventListener(type, e => { if (rfb.key(e.key, down)) e.preventDefault(); });
  // Handing back keeps the last frame on screen as the preview.
  view.done = async () => {
    await leave();
    if (view.live) { try { preview.src = canvas.toDataURL(); } catch { /* The next look replaces it. */ } }
    showLive(false);
    view.holding = false;
    view.done = null;
    view.c = { ...view.c, controller: null };
    if (view.alive()) strip();
  };
  window.addEventListener("hashchange", gone);
  strip();
}

// ---------- the file explorer ----------

function computerFiles(body, id, base, hash) {
  let at = FILES_ROOT;
  const q = path => `?path=${encodeURIComponent(path)}`;
  const crumbs = () => { const parts = at.slice(FILES_ROOT.length).split("/").filter(Boolean); let so = FILES_ROOT; return `<button class="link" data-go="${FILES_ROOT}">Files</button>${parts.map(p => { so += `${p}/`; return ` / <button class="link" data-go="${esc(so)}">${esc(p)}</button>`; }).join("")}`; };
  const size = n => n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(1)} MB`;
  const draw = async () => {
    let entries;
    try { ({ entries } = await api("GET", `${base}/listing${q(at)}`)); } catch (e) { body.innerHTML = `<p class="muted">${esc(e.message)}</p>`; return; }
    if (!stillAt(hash)) return;
    body.innerHTML = `<div class="row files-bar"><span>${crumbs()}</span><button data-bulk-save disabled>Download selected</button><button data-bulk disabled>Delete selected</button><button class="link" data-save-all>Download everything</button></div>
      ${entries.length ? `<table class="t files"><tbody>${entries.map(f => `<tr data-file="${esc(f.name)}" data-dir="${f.dir ? 1 : ""}">
        <td><input type="checkbox" data-pick aria-label="Select ${esc(f.name)}"></td><td><button class="link" data-browse>${esc(f.name)}${f.dir ? "/" : ""}</button></td>
        <td class="small faint n">${f.dir ? "" : esc(size(f.size))}</td><td class="small faint n">${esc(when(new Date(f.modified * 1000).toISOString()))}</td>
        <td>${f.dir ? "" : `<button class="link" data-save>Download</button>`}</td></tr>`).join("")}</tbody></table>` : `<p class="empty">This folder is empty.</p>`}<div id="file-open"></div>`;
  };
  const pathOf = row => `${at}${row.dataset.file}`;
  const open = async path => {
    const pane = $("#file-open", body);
    let file;
    try { file = await api("GET", `${base}/files${q(path)}`); } catch (e) { return toast(e.message); }
    const bytes = bytesOf(file.data_b64);
    let text = null;
    if (bytes.length <= EDITABLE_BYTES) { try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); } catch { /* Not text. */ } }
    if (text === null) { pane.innerHTML = `<p class="small muted">${esc(path)} cannot be edited here. Download it to open it.</p>`; return; }
    pane.innerHTML = `<h3>${esc(path)}</h3><textarea id="file-text" rows="16" spellcheck="false"></textarea><div class="row"><button class="primary" data-write>Save</button></div>`;
    $("#file-text", pane).value = text;
    $("[data-write]", pane).onclick = async () => {
      try { await api("PUT", `${base}/files`, { path, data_b64: b64Of(new TextEncoder().encode($("#file-text", pane).value)) }); toast("Saved."); await draw(); } catch (e) { toast(e.message); }
    };
  };
  body.onclick = async e => {
    const go = e.target.closest("[data-go]");
    if (go) { at = go.dataset.go; return draw(); }
    const row = e.target.closest("tr[data-file]");
    if (e.target.closest("[data-browse]") && row) { if (row.dataset.dir) { at = `${pathOf(row)}/`; return draw(); } return open(pathOf(row)); }
    if (e.target.closest("[data-save]") && row) { try { await Magnus.download(`${base}/files/raw${q(pathOf(row))}`, row.dataset.file); toast("Downloaded."); } catch (err) { toast(err.message); } return; }
    if (e.target.closest("[data-pick]")) { const none = !$$("[data-pick]:checked", body).length; $("[data-bulk]", body).disabled = none; $("[data-bulk-save]", body).disabled = none; return; }
    // Many files leave as one archive; nothing picked is the whole volume with its apps.
    const archive = e.target.closest("[data-bulk-save]") ? $$("[data-pick]:checked", body).map(p => { const r = p.closest("tr"); return `${pathOf(r)}${r.dataset.dir ? "/" : ""}`; }) : e.target.closest("[data-save-all]") ? [] : null;
    if (archive) {
      toast("Packing the files…");
      try { const saved = await Magnus.download(`${base}/export?paths=${encodeURIComponent(archive.join("\n"))}`, `${id}-files.tar`); toast(saved ? `Saved to ${saved}.` : "Downloaded."); } catch (err) { toast(err.message); }
      return;
    }
    if (e.target.closest("[data-bulk]")) {
      const rows = $$("[data-pick]:checked", body).map(p => p.closest("tr"));
      if (!confirm(`Delete ${rows.length} item${rows.length === 1 ? "" : "s"}? They are also removed from backups.`)) return;
      let failed = 0;
      for (const r of rows) { try { await api("DELETE", `${base}/files${q(pathOf(r))}`); } catch { failed++; } }
      toast(failed ? `${failed} could not be deleted.` : "Deleted.");
      draw();
    }
  };
  draw();
}

// ---------- secure sign-in, on its card ----------

// Typed here and sent to the browser only: the agent never sees it.
function secureFormCard(i) {
  const d = i.details || {};
  if (!i.can_respond) return `<p class="small faint">Waiting for its person to sign in.</p>`;
  return `<form class="secure-form" data-secure="${esc(i.id)}"><div class="what"><div class="field"><span>Site</span>${fieldValue(d.site)}</div></div>
    ${d.username_field ? `<label>Username<input data-username autocomplete="off"></label>` : ""}
    <label>Password<input type="password" data-password autocomplete="off" required></label>
    <label class="row small"><input type="checkbox" data-remember> Remember this sign-in in the vault</label>
    <div class="row" style="margin-top:12px"><button type="button" class="primary" data-fill>Sign in</button><button type="button" data-item="${esc(i.id)}" data-decision="decline">Decline</button></div></form>`;
}

document.addEventListener("submit", e => { if (e.target.matches("[data-secure], .local-connect")) e.preventDefault(); });

document.addEventListener("click", async e => {
  const button = e.target.closest("[data-fill]");
  if (!button) return;
  const form = button.closest("[data-secure]"), item = App.inbox.find(i => i.id === form.dataset.secure), d = item?.details;
  if (!d) return;
  const base = `/v1/identities/${enc(d.identity)}/secure-form`, username = $("[data-username]", form)?.value.trim(), password = $("[data-password]", form).value, save = $("[data-remember]", form).checked;
  if (!password) return toast("Enter the password.");
  button.disabled = true;
  try {
    if (d.username_field && username) await api("POST", base, { target: d.username_field, value: username, submit: false, save: false });
    await api("POST", base, { target: d.password_field, value: password, submit: true, save, username: save && username ? username : undefined });
    $("[data-password]", form).value = "";
    toast("Signed in. The agent carries on.");
    await refresh();
  } catch (err) { toast(err.message); button.disabled = false; }
});
