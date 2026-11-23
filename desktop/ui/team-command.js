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

/** Shared workspace spend limits are retired — each collaborator runs on their own personal budget. */
function limitCommands() {
  return [];
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
    out.push({ group: "Create", label: `New project ${t}`, hint: "Gets a conversation, files, a task board and Magnus", run: { kind: "start", what: "project", name: t } });
    if (channelSlug(t)) out.push({ group: "Create", label: `New channel #${channelSlug(t)}`, run: { kind: "start", what: "channel", name: t } });
    out.push({ group: "Create", label: `New task: ${t}`, hint: project ? `On ${home.projects.find(p => p.id === project)?.name || "this project"}'s board` : "On the project's board", run: { kind: "task", title: t, project } });
    if (/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(t)) out.push({ group: "Create", label: `Invite ${t}`, run: { kind: "invite", email: t } });
    out.push({ group: "Ask Magnus", label: `Ask ${home.agent.name}: ${t}`, hint: here && !here.direct?.agent ? `In ${channelTitle(home, here)}, your Magnus shares its output` : "In your conversation with it", run: { kind: "ask", text: t } });
  }
  for (const p of home.projects.filter(p => match(p.name))) out.push({ group: "Go to", label: p.name, hint: `Project · ${p.progress.done}/${p.progress.total} done`, run: { kind: "go", place: { kind: "project", id: p.id } } });
  for (const c of home.channels.filter(c => match(channelTitle(home, c)))) out.push({ group: "Go to", label: channelTitle(home, c), hint: c.mentions ? `${c.mentions} mentions` : c.unread ? `${c.unread} unread` : "", run: { kind: "go", place: { kind: "channel", id: c.id } } });
  for (const m of home.members.filter(m => m.id !== home.me && match(m.name))) out.push({ group: "Go to", label: m.name, hint: "Direct message", run: { kind: "dm", person: m.id } });
  const actions = [
    { label: "Activity: mentions, replies and asks", run: { kind: "go", place: { kind: "activity" } } },
    { label: "People and invites", run: { kind: "go", place: { kind: "people" } } },
    { label: "Add people, remove them, or leave this workspace", run: { kind: "go", place: { kind: "people" } } },
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
  async run(r) {
    const ws = Team.ws, base = `/v1/workspaces/${enc(ws)}`, place = Team.place;
    try {
      if (r.kind === "go") location.hash = teamHref(ws, r.place);
      if (r.kind === "hash") location.hash = r.hash;
      if (r.kind === "start") {
        const made = await Server.workspaces.start(ws, { kind: r.what, name: r.name });
        await loadHome(ws);
        if (made.project) { Team.justMade = made.project; location.hash = teamHref(ws, { kind: "project", id: made.project }); }
        else location.hash = teamHref(ws, { kind: "channel", id: made.channel });
      }
      if (r.kind === "dm") {
        const c = await Server.workspaces.direct(ws, r.agent ? { agent: true } : { people: [r.person] });
        await loadHome(ws);
        location.hash = teamHref(ws, { kind: "channel", id: c.id });
      }
      if (r.kind === "ask") {
        const here = place.kind === "channel" && Team.home.channels.find(c => c.id === place.id);
        if (here) { await TeamChat.send(here.direct?.agent ? r.text : `@${Team.home.agent.name} ${r.text}`, place.thread || null, false, []); return; }
        const c = await Server.workspaces.direct(ws, { agent: true });
        await Server.channels.post(ws, c.id, { text: r.text });
        await loadHome(ws);
        location.hash = teamHref(ws, { kind: "channel", id: c.id });
      }
      if (r.kind === "task") { await Server.workspaces.addTask(ws, { title: r.title, project: r.project || undefined }); toast("Added to the board."); TeamPanel.refresh(); }
      if (r.kind === "invite") { const x = await Server.workspaces.invite(ws, r.email); toast(inviteOutcome(r.email, x), 8000); }
      if (r.kind === "set" && place.kind === "channel") {
        await Server.channels.act(ws, place.id, r.what, r.body);
        await loadHome(ws);
        TeamChat.draw();
        toast("Done.");
      }
      if (r.kind === "download") await Magnus.download(`${base}/files/${enc(r.name)}`, r.name);
    } catch (err) { toast(err.message); }
  },

  /** The full search, in the centre. */
  async results(center, ws, q) {
    const found = q.trim() ? await Server.workspaces.search(ws, q) : {};
    const items = foundItems(found, Team.home);
    paint(center, `<header class="team-head"><h1>Search</h1></header><form class="team-search-form" data-search><input value="${esc(q)}" aria-label="Search messages, files, tasks and knowledge" placeholder="Search messages, files, tasks and knowledge"><button class="primary">Search</button></form>
      <div class="team-scroll">${items.length ? `<ul class="team-activity-list">${items.map((it, i) => `<li><button class="link" data-found="${i}"><strong>${esc(it.label)}</strong></button> <span class="small faint">${esc(it.hint || "")}</span></li>`).join("")}</ul>` : `<p class="empty">${q ? "Nothing found." : "Type what to look for."}</p>`}</div>`);
    const side = $("#team-side");
    if (side?.isConnected) TeamPanel.side(side, ws, null, null);
    center.onsubmit = e => { if (e.target.matches("[data-search]")) { e.preventDefault(); location.hash = teamHref(ws, { kind: "search", q: $("input", e.target).value }); } };
    center.onclick = e => { const b = e.target.closest("[data-found]"); if (b) this.run(items[Number(b.dataset.found)].run); };
  },
};

