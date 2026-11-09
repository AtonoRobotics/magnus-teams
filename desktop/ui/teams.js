// Workspaces: channels, whispers, members, files and knowledge.
UI.team = '<circle cx="9" cy="8" r="3"/><path d="M3 20a6 6 0 0 1 12 0"/><path d="M16 5.5a3 3 0 0 1 0 5M18 14.5a6 6 0 0 1 3 5.5"/>';
const TEAM_REFRESH_MS = 4000, TEAM_ROLES = ["guest", "member", "admin", "owner"];

route(/^#\/teams$/, main => showTeams(main));
route(/^#\/team\/([^/]+)(?:\/([a-z]+)(?:\/([^/]+))?)?$/, (main, ws, tab, more) => showTeam(main, decodeURIComponent(ws), tab || "channels", more && decodeURIComponent(more)));

async function showTeams(main) {
  const draw = async () => {
    const list = await api("GET", "/v1/workspaces");
    paint(main, `<h1>Teams</h1>${list.length ? `<table class="t"><tbody>${list.map(w => `<tr data-workspace="${esc(w.id)}"><td><a href="#/team/${enc(w.id)}">${esc(w.name)}</a></td><td class="small faint">${esc(w.role)}</td></tr>`).join("")}</tbody></table>` : `<p class="empty">You are in no workspace yet.</p>`}
      <form class="row" id="new-team"><input id="team-name" placeholder="A new workspace's name" aria-label="Workspace name"><button class="primary">Create</button></form>`);
  };
  main.onsubmit = async e => {
    if (e.target.id !== "new-team") return;
    e.preventDefault();
    try { const w = await api("POST", "/v1/workspaces", { name: $("#team-name", main).value }); location.hash = `#/team/${enc(w.id)}`; } catch (err) { toast(err.message); }
  };
  await draw();
}

async function showTeam(main, ws, tab, more) {
  const hash = location.hash, base = `/v1/workspaces/${enc(ws)}`, TABS = [["channels", "Channels"], ["members", "Members"], ["files", "Files"], ["knowledge", "Knowledge"]];
  const w = await api("GET", base);
  if (location.hash !== hash) return;
  main._html = null;
  main.innerHTML = `<p class="small"><a href="#/teams">Teams</a></p><h1>${esc(w.name)}</h1><nav class="tabs" aria-label="Workspace">${TABS.map(([k, l]) => `<a href="#/team/${enc(ws)}/${k}" ${k === tab ? 'aria-current="page"' : ""}>${l}</a>`).join("")}</nav><div id="team-tab"></div>`;
  const body = $("#team-tab", main);
  main.onclick = main.onsubmit = main.onchange = null;
  ({ channels: teamChannels, members: teamMembers, files: teamFiles, knowledge: teamKnowledge })[tab](main, body, ws, base, w, hash, more);
}

document.addEventListener("submit", e => { if (e.target.id === "say") e.preventDefault(); });

function teamChannels(main, body, ws, base, w, hash, more) {
  const channel = more && w.channels.some(c => c.id === more) ? more : w.channels[0]?.id;
  const list = `<div class="row">${w.channels.map(c => `<a class="chip ${c.id === channel ? "on" : ""}" href="#/team/${enc(ws)}/channels/${enc(c.id)}">#${esc(c.name)}</a>`).join("")}</div>
    <form class="row" id="new-channel"><input id="channel-name" placeholder="New channel" aria-label="Channel name"><button>Add</button></form>`;
  if (!channel) { body.innerHTML = `${list}<p class="empty">No channels yet.</p>`; bindNewChannel(main, base, ws); return; }
  const guest = w.role === "guest", at = `${base}/channels/${enc(channel)}`;
  body.innerHTML = `${list}<div id="messages" class="team-messages" aria-live="polite"></div>
    <form id="say"><textarea id="say-text" rows="2" placeholder="Write a message" aria-label="Message"></textarea>
      <div class="row"><button type="button" class="primary" data-how="post">Send</button>${guest ? "" : `<button type="button" data-how="agent">Ask the team agent</button><button type="button" data-how="whisper">Ask my agent privately</button>`}</div></form>
    <h3>Only you can see</h3><div id="whispers"></div>`;
  bindNewChannel(main, base, ws);
  const draw = async () => {
    const [messages, whispers] = await Promise.all([api("GET", `${at}/messages`), api("GET", `${at}/whispers`).catch(() => [])]);
    if (location.hash !== hash) return false;
    const box = $("#messages", body), html = messages.map(m => `<div class="msg ${m.sender === "agent" ? "magnus" : ""}"><div class="small faint">${esc(m.sender === "agent" ? "Team agent" : m.sender)}${m.requester ? ` for ${esc(m.requester)}` : ""}</div><div class="pre">${esc(m.text)}</div></div>`).join("") || `<p class="empty">No messages yet.</p>`;
    if (box._html !== html) { box.innerHTML = box._html = html; box.scrollTop = box.scrollHeight; }
    const mine = whispers.map(x => `<div class="msg" data-whisper="${esc(x.task)}"><div class="pre">${esc(x.text || "Working on it…")}</div>${x.done && !x.shared && x.text ? `<button data-share="${esc(x.task)}">Post to the channel</button>` : x.shared ? `<span class="small faint">Posted</span>` : ""}</div>`).join("") || `<p class="small faint">Nothing yet.</p>`;
    const held = $("#whispers", body);
    if (held._html !== mine) held.innerHTML = held._html = mine;
    return true;
  };
  (async () => { while (await draw().catch(() => location.hash === hash)) await new Promise(r => setTimeout(r, TEAM_REFRESH_MS)); })();
  $("#say", body).onclick = async e => {
    const how = e.target.closest("[data-how]")?.dataset.how;
    const box = $("#say-text", body);
    if (!how || !box.value.trim()) return;
    try { await api("POST", `${at}/messages`, { text: box.value, agent: how === "agent", whisper: how === "whisper" }); box.value = ""; await draw(); } catch (err) { toast(err.message); }
  };
  $("#whispers", body).onclick = async e => {
    const b = e.target.closest("[data-share]");
    if (!b) return;
    try { await api("POST", `${base}/whispers/${enc(b.dataset.share)}/share`, {}); await draw(); } catch (err) { toast(err.message); }
  };
}

function bindNewChannel(main, base, ws) {
  main.onsubmit = async e => {
    if (e.target.id !== "new-channel") return;
    e.preventDefault();
    try { const c = await api("POST", `${base}/channels`, { name: $("#channel-name", main).value }); location.hash = `#/team/${enc(ws)}/channels/${enc(c.id)}`; } catch (err) { toast(err.message); }
  };
}

function teamMembers(main, body, ws, base, w) {
  const admin = ["admin", "owner"].includes(w.role), redraw = () => showTeam(main, ws, "members");
  const roles = selected => TEAM_ROLES.map(r => `<option ${r === selected ? "selected" : ""}>${r}</option>`).join("");
  body.innerHTML = `<table class="t"><tbody>${Object.entries(w.members).map(([p, role]) => `<tr data-member="${esc(p)}"><td>${esc(p)}</td><td>${admin ? `<select data-role aria-label="Role of ${esc(p)}">${roles(role)}</select>` : esc(role)}</td><td>${admin ? `<button class="link" data-remove>Remove</button>` : ""}</td></tr>`).join("")}</tbody></table>
    ${admin ? `<form class="row" id="new-member"><input id="member-id" placeholder="Their sign-in name" aria-label="Person"><select id="member-role" aria-label="Role">${roles("member")}</select><button class="primary">Add</button></form>` : ""}`;
  main.onchange = async e => {
    if (!e.target.matches("[data-role]")) return;
    try { await api("POST", `${base}/members`, { person: e.target.closest("tr").dataset.member, role: e.target.value }); toast("Changed."); } catch (err) { toast(err.message); redraw(); }
  };
  main.onclick = async e => {
    if (!e.target.closest("[data-remove]")) return;
    const p = e.target.closest("tr").dataset.member;
    if (!confirm(`Remove ${p}? Their running work here stops.`)) return;
    try { await api("DELETE", `${base}/members/${enc(p)}`); redraw(); } catch (err) { toast(err.message); }
  };
  main.onsubmit = async e => {
    if (e.target.id !== "new-member") return;
    e.preventDefault();
    try { await api("POST", `${base}/members`, { person: $("#member-id", main).value.trim(), role: $("#member-role", main).value }); redraw(); } catch (err) { toast(err.message); }
  };
}

function teamFiles(main, body, ws, base, w) {
  const redraw = () => showTeam(main, ws, "files"), guest = w.role === "guest";
  body.innerHTML = `${w.files.length ? `<table class="t"><tbody>${w.files.map(f => `<tr data-team-file="${esc(f.name)}"><td>${esc(f.name)}</td><td class="small faint">${esc(f.by)}</td><td class="small faint n">${esc(f.size)} B</td><td><div class="row"><button class="link" data-save>Download</button>${guest ? "" : `<button class="link" data-delete>Delete</button>`}</div></td></tr>`).join("")}</tbody></table>` : `<p class="empty">No files yet.</p>`}
    ${guest ? "" : `<label class="row"><span>Add a file</span><input type="file" id="team-upload"></label>`}`;
  main.onclick = async e => {
    const row = e.target.closest("tr[data-team-file]");
    if (!row) return;
    const name = row.dataset.teamFile, at = `${base}/files/${enc(name)}`;
    try {
      if (e.target.closest("[data-save]")) { await Magnus.download(at, name); toast("Downloaded."); }
      if (e.target.closest("[data-delete]") && confirm(`Delete ${name}?`)) { await api("DELETE", at); redraw(); }
    } catch (err) { toast(err.message); }
  };
  main.onchange = async e => {
    if (e.target.id !== "team-upload" || !e.target.files.length) return;
    const f = e.target.files[0];
    try { await Magnus.putBytes(`${base}/files/${enc(f.name)}`, new Uint8Array(await f.arrayBuffer())); redraw(); } catch (err) { toast(err.message); }
  };
}

async function teamKnowledge(main, body, ws, base, w, hash) {
  const redraw = () => showTeam(main, ws, "knowledge");
  const scope = await api("GET", `${base}/knowledge`);
  if (location.hash !== hash) return;
  const entries = Array.isArray(scope) ? scope : scope.entries || [];
  body.innerHTML = `${entries.length ? `<table class="t"><tbody>${entries.map(k => `<tr data-entry="${esc(k.id)}"><td>${esc(k.statement)}<div class="small faint">${esc(k.kind)} · ${esc(k.source)} · ${esc(k.author)}</div></td><td><div class="row"><button class="link" data-flag>Flag</button><button class="link" data-retire>Retire</button></div></td></tr>`).join("")}</tbody></table>` : `<p class="empty">Nothing recorded yet.</p>`}
    ${w.role === "guest" ? "" : `<form id="new-entry"><textarea id="entry-text" rows="2" placeholder="Something the team should know" aria-label="Statement"></textarea><div class="row"><button class="primary">Add</button></div></form>`}`;
  main.onclick = async e => {
    const row = e.target.closest("tr[data-entry]");
    if (!row) return;
    const at = `${base}/knowledge/${enc(row.dataset.entry)}`;
    try {
      if (e.target.closest("[data-flag]")) { const why = prompt("What is wrong with it?"); if (why) { await api("POST", `${at}/flag`, { why }); toast("Flagged."); } }
      if (e.target.closest("[data-retire]") && confirm("Retire this?")) { await api("POST", `${at}/retire`, {}); redraw(); }
    } catch (err) { toast(err.message); }
  };
  main.onsubmit = async e => {
    if (e.target.id !== "new-entry") return;
    e.preventDefault();
    try { await api("POST", `${base}/knowledge`, { statement: $("#entry-text", main).value }); redraw(); } catch (err) { toast(err.message); }
  };
}
