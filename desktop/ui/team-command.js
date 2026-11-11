// The command menu (Ctrl or Cmd+K) and the one search. Every action in a
// workspace is reachable from here, and making a project or a channel is
// typing its name and pressing Enter.

/**
 * @typedef {{ group: string, label: string, hint?: string, run: Object }} TeamCommandItem
 */

const TEAM_SEARCH_DEBOUNCE_MS = 200, TEAM_COMMANDS_SHOWN = 12;

/** A channel's name from what was typed, as the server makes it. */
function channelSlug(typed) {
  return String(typed || "").trim().replace(/^#/, "").split(/[^\p{L}\p{N}]+/u).filter(Boolean).map(w => w.toLowerCase()).join("-");
}

/** Dollars as a person reads them. */
function teamUsd(n) { const v = Number(n || 0); return v > 0 && v < 0.01 ? "under $0.01" : `$${v.toFixed(v % 1 ? 2 : 0)}`; }

/**
 * An owner's spend limits, set by typing an amount: for this channel and
 * for the whole workspace, a month at a time. With nothing typed, lifting
 * a limit that is set.
 * @param {TeamHome} home @param {?TeamChannel} here @param {string} typed
 * @returns {TeamCommandItem[]}
 */
function limitCommands(home, here, typed) {
  if (home.role !== "owner") return [];
  const out = [], amount = String(typed || "").trim().match(/^\$?\s*(\d{1,7}(?:\.\d{1,2})?)$/), spend = home.spend || { channels: {} };
  const named = here && !here.direct ? channelTitle(home, here) : null;
  if (amount) {
    const n = Number(amount[1]);
    if (named) out.push({ group: "Spend limits", label: `Limit ${named} to ${teamUsd(n)} a month`, hint: "What the team agent may spend on work asked here", run: { kind: "limit", channel: here.id, usd: n } });
    out.push({ group: "Spend limits", label: `Limit the workspace to ${teamUsd(n)} a month`, hint: "What the team agent may spend on all its work here", run: { kind: "limit", usd: n } });
  } else if (!typed) {
    if (named && spend.channels?.[here.id]?.limit_usd != null) out.push({ group: "Spend limits", label: `Lift the spend limit on ${named}`, run: { kind: "limit", channel: here.id, usd: null } });
    if (spend.limit_usd != null) out.push({ group: "Spend limits", label: "Lift the workspace's spend limit", run: { kind: "limit", usd: null } });
  }
  return out;
}

/**
 * What the menu offers for what was typed, where the person is.
 * @param {TeamHome} home @param {TeamPlace} place @param {string} q
 * @returns {TeamCommandItem[]}
 */
function commandsFor(home, place, q) {
  const t = String(q || "").trim(), low = t.toLowerCase(), out = [];
  const match = s => !low || String(s).toLowerCase().includes(low);
  const member = home.role !== "guest";
  const here = place.kind === "channel" ? home.channels.find(c => c.id === place.id) : null;
  const project = place.kind === "project" ? place.id : here?.project;
  if (t && member) {
    out.push({ group: "Create", label: `New project ${t}`, hint: "Gets a channel, files, a task board and Magnus", run: { kind: "start", what: "project", name: t } });
    if (channelSlug(t)) out.push({ group: "Create", label: `New channel #${channelSlug(t)}`, run: { kind: "start", what: "channel", name: t } });
    out.push({ group: "Create", label: `New task: ${t}`, hint: project ? `On ${home.projects.find(p => p.id === project)?.name || "this project"}'s board` : "On the workspace's board", run: { kind: "task", title: t, project } });
    if (/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(t) && ["admin", "owner"].includes(home.role)) out.push({ group: "Create", label: `Invite ${t}`, run: { kind: "invite", email: t } });
    out.push({ group: "Ask Magnus", label: `Ask ${home.agent.name}: ${t}`, hint: here && !here.direct?.agent ? `In ${channelTitle(home, here)}, visible to everyone there` : "In your conversation with it", run: { kind: "ask", text: t } });
  }
  // An amount typed by an owner is most likely a limit, so it comes first.
  const limits = limitCommands(home, here, t);
  if (t) out.unshift(...limits); else out.push(...limits);
  for (const p of home.projects.filter(p => match(p.name))) out.push({ group: "Go to", label: p.name, hint: `Project · ${p.progress.done}/${p.progress.total} done`, run: { kind: "go", place: { kind: "project", id: p.id } } });
  for (const c of home.channels.filter(c => match(channelTitle(home, c)))) out.push({ group: "Go to", label: channelTitle(home, c), hint: c.mentions ? `${c.mentions} mentions` : c.unread ? `${c.unread} unread` : "", run: { kind: "go", place: { kind: "channel", id: c.id } } });
  for (const m of home.members.filter(m => m.id !== home.me && match(m.name))) out.push({ group: "Go to", label: m.name, hint: "Direct message", run: { kind: "dm", person: m.id } });
  const actions = [
    { label: "Activity: mentions, replies and asks", run: { kind: "go", place: { kind: "activity" } } },
    { label: "People and invites", run: { kind: "go", place: { kind: "people" } } },
    { label: `Talk to ${home.agent.name} directly`, run: { kind: "dm", agent: true } },
    { label: "All workspaces", run: { kind: "hash", hash: "#/teams" } },
  ];
  if (here && !here.direct && member) {
    actions.push({ label: `${home.agent.name} here: reply to mentions only`, run: { kind: "set", what: "agent", body: { mode: "mentions" } } });
    actions.push({ label: `${home.agent.name} here: join when it can help`, run: { kind: "set", what: "agent", body: { mode: "when_helpful" } } });
  }
  if (here) for (const [level, l] of [["all", "all messages"], ["mentions", "mentions only"], ["off", "off"]]) actions.push({ label: `Notifications here: ${l}`, run: { kind: "set", what: "notify", body: { level } } });
  if (here) actions.push({ label: `Mark ${channelTitle(home, here)} read`, run: { kind: "set", what: "read", body: {} } });
  for (const a of actions.filter(a => match(a.label))) out.push({ group: "Actions", ...a });
  if (t) out.push({ group: "Search", label: `Search everything for “${t}”`, hint: "Messages, files, tasks and knowledge", run: { kind: "go", place: { kind: "search", q: t } } });
  return out;
}

/** Search results as menu items. */
function foundItems(found, home) {
  const out = [];
  for (const m of found.messages || []) out.push({ group: "Found", label: firstLine(m.text), hint: `#${m.channel_name || "direct"} · ${home.members.find(x => x.id === m.sender)?.name || m.sender}`, run: { kind: "go", place: { kind: "channel", id: m.channel, thread: m.thread || undefined } } });
  for (const t of found.tasks || []) out.push({ group: "Found", label: t.title, hint: `Task · ${t.status}`, run: t.project ? { kind: "go", place: { kind: "project", id: t.project, tab: "board" } } : { kind: "none" } });
  for (const f of found.files || []) out.push({ group: "Found", label: f.name, hint: "File", run: f.project ? { kind: "go", place: { kind: "project", id: f.project, tab: "files" } } : { kind: "download", name: f.name } });
  for (const k of found.knowledge || []) out.push({ group: "Found", label: k.statement, hint: `Knowledge · ${k.kind}`, run: k.project ? { kind: "go", place: { kind: "project", id: k.project, tab: "knowledge" } } : { kind: "none" } });
  return out;
}

const TeamCommand = {
  dialog: null, items: [], at: 0, found: { messages: [], files: [], tasks: [], knowledge: [] }, timer: 0,

  open(initial = "") {
    if (!Team.home || !location.hash.startsWith("#/team/")) return;
    this.close();
    const d = this.dialog = document.createElement("dialog");
    d.className = "team-command";
    d.setAttribute("aria-label", "Search or run a command");
    d.innerHTML = `<input type="text" placeholder="Search, or type a name to create" aria-label="Search or run a command" aria-controls="team-command-list" autocomplete="off"><div id="team-command-list" role="listbox"></div><p class="small faint">Up and Down to move · Enter to choose · Esc to close</p>`;
    document.body.append(d);
    const input = $("input", d);
    input.value = initial;
    input.oninput = () => { this.at = 0; this.found = { messages: [], files: [], tasks: [], knowledge: [] }; this.draw(); this.search(input.value); };
    input.onkeydown = e => {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); this.at = (this.at + (e.key === "ArrowDown" ? 1 : -1) + this.items.length) % Math.max(1, this.items.length); this.draw(); }
      if (e.key === "Enter") { e.preventDefault(); const it = this.items[this.at]; if (it) this.run(it.run); }
    };
    d.onclick = e => { const b = e.target.closest("[data-cmd]"); if (b) this.run(this.items[Number(b.dataset.cmd)].run); else if (e.target === d) this.close(); };
    d.addEventListener("close", () => { d.remove(); if (this.dialog === d) this.dialog = null; });
    d.showModal();
    input.focus();
    this.draw();
    if (initial) this.search(initial);
  },

  close() { this.dialog?.close(); this.dialog = null; },

  draw() {
    const d = this.dialog;
    if (!d) return;
    const q = $("input", d).value;
    this.items = [...commandsFor(Team.home, Team.place, q).slice(0, TEAM_COMMANDS_SHOWN), ...foundItems(this.found, Team.home)];
    let group = "";
    $("#team-command-list", d).innerHTML = this.items.map((it, i) => {
      const head = it.group !== group ? `<div class="team-command-group" role="presentation">${esc(group = it.group)}</div>` : "";
      return `${head}<button type="button" role="option" data-cmd="${i}" aria-selected="${i === this.at}" class="${i === this.at ? "on" : ""}"><span>${esc(it.label)}</span>${it.hint ? `<span class="small faint">${esc(it.hint)}</span>` : ""}</button>`;
    }).join("") || `<p class="small faint">Nothing matches.</p>`;
    $(`[data-cmd="${this.at}"]`, d)?.scrollIntoView?.({ block: "nearest" });
  },

  search(q) {
    clearTimeout(this.timer);
    if (!q.trim()) return;
    this.timer = setTimeout(async () => {
      try {
        const found = await api("GET", `/v1/workspaces/${enc(Team.ws)}/search?q=${enc(q)}`);
        if (this.dialog && $("input", this.dialog).value === q) { this.found = found; this.draw(); }
      } catch { /* the typed commands still work */ }
    }, TEAM_SEARCH_DEBOUNCE_MS);
  },

  async run(r) {
    const ws = Team.ws, base = `/v1/workspaces/${enc(ws)}`, place = Team.place;
    this.close();
    try {
      if (r.kind === "go") location.hash = teamHref(ws, r.place);
      if (r.kind === "hash") location.hash = r.hash;
      if (r.kind === "start") {
        const made = await api("POST", `${base}/start`, { kind: r.what, name: r.name });
        await loadHome(ws);
        if (made.project) { Team.justMade = made.project; location.hash = teamHref(ws, { kind: "project", id: made.project }); }
        else location.hash = teamHref(ws, { kind: "channel", id: made.channel });
      }
      if (r.kind === "dm") {
        const c = await api("POST", `${base}/direct`, r.agent ? { agent: true } : { people: [r.person] });
        await loadHome(ws);
        location.hash = teamHref(ws, { kind: "channel", id: c.id });
      }
      if (r.kind === "ask") {
        const here = place.kind === "channel" && Team.home.channels.find(c => c.id === place.id);
        if (here) { await TeamChat.send(here.direct?.agent ? r.text : `@${Team.home.agent.name} ${r.text}`, place.thread || null, false, []); return; }
        const c = await api("POST", `${base}/direct`, { agent: true });
        await api("POST", `${base}/channels/${enc(c.id)}/messages`, { text: r.text });
        await loadHome(ws);
        location.hash = teamHref(ws, { kind: "channel", id: c.id });
      }
      if (r.kind === "task") { await api("POST", `${base}/tasks`, { title: r.title, project: r.project || undefined }); toast("Added to the board."); TeamPanel.refresh(); }
      if (r.kind === "invite") { const x = await api("POST", `${base}/invites`, { email: r.email }); toast(inviteOutcome(r.email, x), 8000); }
      if (r.kind === "set" && place.kind === "channel") {
        await api("POST", `${base}/channels/${enc(place.id)}/${r.what}`, r.body);
        await loadHome(ws);
        TeamChat.draw();
        toast("Done.");
      }
      if (r.kind === "download") await Magnus.download(`${base}/files/${enc(r.name)}`, r.name);
      if (r.kind === "limit") {
        await api("POST", `${base}/limit`, { usd: r.usd, channel: r.channel || undefined });
        await loadHome(ws);
        await refreshCenter($("#main"), ws);
        toast(r.usd == null ? "Limit lifted." : `Limited to ${teamUsd(r.usd)} a month.`);
      }
    } catch (err) { toast(err.message); }
  },

  /** The full search, in the centre. */
  async results(center, ws, q) {
    const found = q.trim() ? await api("GET", `/v1/workspaces/${enc(ws)}/search?q=${enc(q)}`) : {};
    const items = foundItems(found, Team.home);
    paint(center, `<header class="team-head"><h1>Search</h1></header><form class="team-search-form" data-search><input value="${esc(q)}" aria-label="Search messages, files, tasks and knowledge" placeholder="Search messages, files, tasks and knowledge"><button class="primary">Search</button></form>
      <div class="team-scroll">${items.length ? `<ul class="team-activity-list">${items.map((it, i) => `<li><button class="link" data-found="${i}"><strong>${esc(it.label)}</strong></button> <span class="small faint">${esc(it.hint || "")}</span></li>`).join("")}</ul>` : `<p class="empty">${q ? "Nothing found." : "Type what to look for."}</p>`}</div>`);
    paint($("#team-side"), "");
    center.onsubmit = e => { if (e.target.matches("[data-search]")) { e.preventDefault(); location.hash = teamHref(ws, { kind: "search", q: $("input", e.target).value }); } };
    center.onclick = e => { const b = e.target.closest("[data-found]"); if (b) this.run(items[Number(b.dataset.found)].run); };
  },
};

document.addEventListener("keydown", e => {
  if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "k" && location.hash.startsWith("#/team/")) {
    e.preventDefault();
    // A dialog just closed with Escape still awaits its close event: it is
    // shut, so the keys open the menu again rather than "closing" it.
    if (TeamCommand.dialog?.open) TeamCommand.close(); else TeamCommand.open();
  }
});
