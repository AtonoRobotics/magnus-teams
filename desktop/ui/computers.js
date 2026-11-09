// The person's cloud computers (status, preview, takeover, files, activity)
// and this computer, connected through the desktop app.
UI.computer = '<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>';
const PREVIEW_EVERY_MS = 3000, EDITABLE_BYTES = 256 * 1024, FILES_ROOT = "/workspace/";

route(/^#\/computer$/, main => showComputers(main));
route(/^#\/computer\/([^/]+)(?:\/([a-z]+))?$/, (main, id, tab) => showComputer(main, decodeURIComponent(id), tab || "screen"));

const stillAt = hash => location.hash === hash;
const bytesOf = b64 => Uint8Array.from(atob(b64), c => c.charCodeAt(0));
const b64Of = bytes => { let s = ""; for (const b of bytes) s += String.fromCharCode(b); return btoa(s); };
const STATE_WORDS = { running: ["good", "Running"], stopped: ["", "Stopped"], hibernated: ["", "Asleep"], queued: ["", "Waiting to start"], paused: ["warn", "Paused"], killed: ["bad", "Stopped for good"] };
const stateChip = c => { const [cls, word] = STATE_WORDS[c.status] || ["", c.status]; return `<span class="chip ${cls}">${esc(word)}</span>${c.controller ? ` <span class="chip warn">${esc(c.controller)} has control</span>` : ""}`; };

async function showComputers(main) {
  const hash = location.hash;
  const draw = async () => {
    let computers = [], unavailable = "";
    try { computers = (await api("GET", "/v1/computers")).computers; } catch (e) { unavailable = e.message; }
    const here = await thisComputerSection();
    if (!stillAt(hash)) return;
    paint(main, `<h1>Computers</h1>
      <h2>Cloud computers</h2>
      ${unavailable ? `<p class="muted">${esc(unavailable)}</p>` : computers.length ? `<table class="t"><tbody>${computers.map(c => `<tr data-computer="${esc(c.id)}"><td><a href="#/computer/${enc(c.id)}">${esc(c.id)}</a></td><td>${stateChip(c)}${c.queue_position ? ` <span class="small faint">number ${esc(c.queue_position)} in line</span>` : ""}</td></tr>`).join("")}</tbody></table>` : `<p class="empty">You have no cloud computer yet.</p>`}
      ${unavailable ? "" : `<div class="row"><button class="primary" data-act="new-computer">New computer</button></div>`}
      ${here}`);
  };
  main.onclick = async e => {
    const b = e.target.closest("[data-act]");
    if (!b) return;
    try {
      if (b.dataset.act === "new-computer") { const c = await api("POST", "/v1/computers", {}); location.hash = `#/computer/${enc(c.id)}`; return; }
      if (await thisComputerAction(b, main)) await draw();
    } catch (err) { toast(err.message); }
  };
  await draw();
}

// The page's policy forbids a form from going anywhere: forms here are read
// by their own handlers.
document.addEventListener("submit", e => { if (e.target.closest("#main")) e.preventDefault(); });

// ---------- this computer ----------

async function thisComputerSection() {
  const [known, running] = await Promise.all([api("GET", "/v1/local/computers").catch(() => []), Magnus.localState()]);
  const rows = known.map(({ computer: c, online, in_use }) => `<tr data-local="${esc(c.id)}"><td>${esc(c.name)}<div class="small faint">${esc(c.folders.join(", "))}</div></td>
    <td><span class="chip ${online ? "good" : ""}">${online ? (in_use ? "In use by you" : "Online") : "Offline"}</span> <span class="small faint">${esc({ ask: "Asks each time", allow: "Allowed", never: "Never" }[c.mode] || c.mode)}</span></td>
    <td><div class="row">${online && in_use ? `<button data-act="local-resume" data-id="${esc(c.id)}">Let the agent go on</button>` : ""}<button class="link" data-act="local-remove" data-id="${esc(c.id)}">Disconnect</button></div></td></tr>`).join("");
  const connect = !running ? "" : running.running
    ? `<div class="card flat"><p>This computer is connected as <b>${esc(running.name)}</b>${running.paused ? ". The agent is paused here." : "."}</p><div class="row"><button data-act="local-pause" data-to="${running.paused ? "" : "1"}">${running.paused ? "Let the agent work" : "Pause the agent"}</button><button class="link" data-act="local-stop">Stop</button></div></div>`
    : `<form class="card flat local-connect"><h3>Connect this computer</h3><p class="small muted">The agent works here only inside the folders you name, and only while this app is open.</p>
        <label>Name<input id="lc-name" value="${esc(running.suggested_name || "")}"></label>
        <label>Folders, one per line<textarea id="lc-folders" rows="3" placeholder="/home/me/Projects"></textarea></label>
        <label>When the agent wants to work here<select id="lc-mode"><option value="ask">Ask me each time</option><option value="allow">Allow it</option><option value="never">Never</option></select></label>
        <div class="row"><button type="button" class="primary" data-act="local-connect">Connect</button></div></form>`;
  return `<h2>This computer</h2>${rows ? `<table class="t"><tbody>${rows}</tbody></table>` : `<p class="empty">No computer of yours is connected.</p>`}${connect}`;
}

async function thisComputerAction(b, main) {
  const id = b.dataset.id;
  switch (b.dataset.act) {
    case "local-connect": {
      const folders = $("#lc-folders", main).value.split("\n").map(f => f.trim()).filter(Boolean);
      await Magnus.localStart({ name: $("#lc-name", main).value.trim(), mode: $("#lc-mode", main).value, folders });
      toast("Connected."); return true;
    }
    case "local-pause": await Magnus.localPause(!!b.dataset.to); return true;
    case "local-stop": await Magnus.localStop(); return true;
    case "local-resume": await api("POST", `/v1/local/computers/${enc(id)}/resume`, {}); toast("The agent will go on."); return true;
    case "local-remove": await Magnus.localStop(); await api("DELETE", `/v1/local/computers/${enc(id)}`); return true;
  }
  return false;
}

// ---------- one computer ----------

async function showComputer(main, id, tab) {
  const hash = location.hash, base = `/v1/computers/${enc(id)}`, TABS = [["screen", "Screen"], ["files", "Files"], ["activity", "Activity"]];
  const c = await api("GET", base);
  if (!stillAt(hash)) return;
  const head = c => `<p class="small"><a href="#/computer">Computers</a></p><h1>Computer</h1><div class="row"><span data-state>${stateChip(c)}</span>
    ${["stopped", "hibernated", "paused"].includes(c.status) ? `<button class="primary" data-act="start">Start</button>` : ""}
    ${c.status === "running" ? `<button data-act="hibernate">Put to sleep</button>` : ""}</div>
    <nav class="tabs" aria-label="Computer">${TABS.map(([k, l]) => `<a href="#/computer/${enc(id)}/${k}" ${k === tab ? 'aria-current="page"' : ""}>${l}</a>`).join("")}</nav><div id="computer-tab"></div>`;
  paint(main, head(c));
  const body = $("#computer-tab", main);
  main.onclick = async e => {
    const b = e.target.closest("[data-act]");
    if (!b || !["start", "hibernate"].includes(b.dataset.act)) return;
    try { await api("POST", `${base}/${b.dataset.act}`, {}); } catch (err) { return toast(err.message); }
    main._html = null;
    showComputer(main, id, tab);
  };
  if (tab === "files") return computerFiles(body, id, base, hash);
  if (tab === "activity") {
    const { activity } = await api("GET", `${base}/activity`);
    body.innerHTML = activity.length ? `<table class="t"><tbody>${[...activity].reverse().map(a => `<tr><td>${esc(a.what)}</td><td class="small faint">${esc(a.who)}</td><td class="small faint n">${esc(when(a.at))}</td></tr>`).join("")}</tbody></table>` : `<p class="empty">Nothing has happened on this computer yet.</p>`;
    return;
  }
  computerScreen(body, c, base, hash);
}

function computerScreen(body, c, base, hash) {
  if (c.status !== "running") { body.innerHTML = `<p class="empty">Start this computer to see its screen.</p>`; return; }
  body.innerHTML = `<div class="row"><button class="primary" data-take>Take control</button></div><div class="screen"><img id="preview" alt="The computer's screen"></div>`;
  let looking = true;
  const look = async () => {
    while (looking && stillAt(hash)) {
      try { const { image_b64 } = await api("GET", `${base}/screen`); if (looking) { const img = $("#preview", body); if (img) img.src = `data:image/png;base64,${image_b64}`; } } catch { /* The next look may work. */ }
      await new Promise(r => setTimeout(r, PREVIEW_EVERY_MS));
    }
  };
  look();
  $("[data-take]", body).onclick = async () => {
    let grant;
    try { grant = await api("POST", `${base}/takeover`, {}); } catch (e) { return toast(e.message); }
    looking = false;
    takeOver(body, c.id, base, grant, hash);
  };
}

// The person has the whole screen until they say they are done.
function takeOver(body, id, base, grant, hash) {
  body.innerHTML = `<div class="row"><span class="small muted">You have control. The agent is paused.</span><button class="primary" data-done>I'm done</button></div><div class="screen"><canvas id="display" tabindex="0" aria-label="The computer's screen"></canvas></div><p class="small bad" id="display-error" hidden></p>`;
  const canvas = $("#display", body), ctx = canvas.getContext("2d");
  let link, ended = false;
  const failed = message => { const p = $("#display-error", body); if (p) { p.hidden = false; p.textContent = message; } };
  const rfb = new Rfb({
    send: b => link.send(b),
    onSize: (w, h) => { canvas.width = w; canvas.height = h; canvas.focus(); },
    onRect: (x, y, w, h, rgba) => ctx.putImageData(new ImageData(rgba, w, h), x, y),
    onCopy: (x, y, w, h, fromX, fromY) => ctx.drawImage(canvas, fromX, fromY, w, h, x, y, w, h),
    onError: failed,
  });
  const leave = async () => {
    if (ended) return;
    ended = true;
    window.removeEventListener("hashchange", leave);
    link.close();
    try { await api("POST", `${base}/handback`, {}); toast("Handed back. The agent carries on."); } catch (e) { toast(e.message); }
  };
  link = Magnus.displayOpen(id, grant.ticket, grant.protocol_prefix, b => rfb.feed(b), reason => { if (!ended) failed(reason || "The connection to the screen ended."); });
  const at = e => { const r = canvas.getBoundingClientRect(); return [Math.round((e.clientX - r.left) * canvas.width / r.width), Math.round((e.clientY - r.top) * canvas.height / r.height)]; };
  const mask = e => (e.buttons & 1) | (e.buttons & 4 ? 2 : 0) | (e.buttons & 2 ? 4 : 0);
  for (const type of ["mousemove", "mousedown", "mouseup"]) canvas.addEventListener(type, e => { const [x, y] = at(e); rfb.pointer(x, y, mask(e)); e.preventDefault(); });
  canvas.addEventListener("wheel", e => { const [x, y] = at(e), bit = e.deltaY < 0 ? 8 : 16; rfb.pointer(x, y, bit); rfb.pointer(x, y, 0); e.preventDefault(); }, { passive: false });
  canvas.addEventListener("contextmenu", e => e.preventDefault());
  for (const [type, down] of [["keydown", true], ["keyup", false]]) canvas.addEventListener(type, e => { if (rfb.key(e.key, down)) e.preventDefault(); });
  $("[data-done]", body).onclick = async () => { await leave(); if (stillAt(hash)) showComputer($("#main"), id, "screen"); };
  window.addEventListener("hashchange", leave);
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
    body.innerHTML = `<div class="row files-bar"><span>${crumbs()}</span><button data-bulk disabled>Delete selected</button></div>
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
    if (e.target.closest("[data-pick]")) { $("[data-bulk]", body).disabled = !$$("[data-pick]:checked", body).length; return; }
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
