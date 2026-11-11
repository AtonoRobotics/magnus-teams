// The right panel and the project page: the team computer's screen, the
// agents' current work, the task board, files, knowledge and members, with
// progress at a glance. A task made from a conversation links back to it.

/**
 * @typedef {{ id: string, project: ?string, title: string, status: "todo"|"doing"|"done", assignee: ?string, from: ?{ channel: string, message: string }, by: string }} TeamTask
 * @typedef {{ project: ?Object, channels: string[], files: Array<{ name: string, size: number, by: string, by_name: string, project: ?string }>, tasks: TeamTask[], work: Array<{ task: string, channel: string, requester: string, requester_name: string, waiting: boolean, steps: number }>, members: Array<{ id: string, name: string }>, knowledge: number, progress: ?{ done: number, total: number }, computer: ?string }} TeamPanelData
 */

/** The last look at each team computer's screen, so a redraw keeps it. */
const TeamScreens = {}, TeamScreenCaptions = {};

/**
 * Where the team computer shows before the server has made it: the
 * workspace always has one (spec rev 142), so nobody sets it up; it is made
 * with the workspace and is ready the first time the agent needs it.
 * @param {TeamHome} home
 */
function noTeamComputer(home) {
  const agent = esc(home.agent.name);
  return `<div class="team-screen"><div class="team-screen-none"><p class="small">${agent}'s computer is being set up. It is ready the first time ${agent} needs it.</p></div></div>`;
}
const TEAM_STATUSES = [["todo", "To do"], ["doing", "Doing"], ["done", "Done"]];
const TEAM_PROJECT_TABS = [["overview", "Overview"], ["board", "Board"], ["files", "Files"], ["knowledge", "Knowledge"], ["members", "Members"]];

/** The kinds of fact a project starts with; it adds its own by using them. */
const TEAM_KB_TYPES = ["decision", "definition", "person", "system", "client", "process"];

/**
 * One knowledge entry as the spec has it: its type and statement, source,
 * author, owner and last review, the thread it came from, its relations,
 * and any flags. Curators correct or retire it; anyone flags it.
 * @param {{ id: string, kind: string, statement: string, source: string, weight: string, author: string, owner: string, thread: ?string, reviewed_at: string, relations?: Array<{ kind: string, to: string }>, flags?: Array<{ by: string, why: string }> }} k
 * @param {Array<{ id: string, statement: string }>} all @param {(id: string) => string} name @param {boolean} curator
 */
function kbEntryHtml(k, all, name, curator) {
  const rel = (k.relations || []).map(r => `${esc(r.kind.replaceAll("_", " "))} “${esc(all.find(e => e.id === r.to)?.statement || r.to)}”`).join("; ");
  const flags = (k.flags || []).map(f => `<div class="small bad">Flagged by ${esc(name(f.by))}: ${esc(f.why)}</div>`).join("");
  const from = k.thread ? " · from a thread" : "";
  return `<li data-entry="${esc(k.id)}"><div class="team-kb-head"><span class="chip">${esc(k.kind)}</span><span class="grow">${esc(k.statement)}</span>${curator ? `<button class="link" data-entry-correct>Correct</button><button class="link" data-entry-retire>Retire</button>` : ""}<button class="link" data-entry-flag>Flag</button></div>
    <div class="small faint">Source: ${esc(k.source)}${k.weight === "outside" ? " (from outside the workspace: weigh it accordingly)" : ""} · added by ${esc(name(k.author))} · owner ${esc(name(k.owner))} · reviewed ${esc(when(k.reviewed_at))}${from}</div>${rel ? `<div class="small">${rel}</div>` : ""}${flags}</li>`;
}

/** A progress bar a reader takes in at a glance. */
function progressBar(p) {
  if (!p || !p.total) return `<span class="small faint">No tasks yet</span>`;
  const pct = Math.round(100 * p.done / p.total);
  return `<span class="team-progress" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${pct}" aria-label="${p.done} of ${p.total} tasks done"><span style="width:${pct}%"></span></span><span class="small faint">${p.done} of ${p.total} done</span>`;
}

/**
 * One task card: its status, who has it, and where it came from.
 * @param {TeamTask} t @param {string} ws @param {(id: string) => string} name @param {boolean} edit
 */
function teamTaskCard(t, ws, name, edit) {
  const from = t.from ? `<a class="small" href="${teamHref(ws, { kind: "channel", id: t.from.channel, thread: t.from.message })}">From the conversation</a>` : "";
  return `<li class="team-task" data-task="${esc(t.id)}" draggable="${edit}"><span>${esc(t.title)}</span>
    <div class="row small">${edit ? `<select data-task-status aria-label="Status of ${esc(t.title)}">${TEAM_STATUSES.map(([v, l]) => `<option value="${v}"${t.status === v ? " selected" : ""}>${l}</option>`).join("")}</select>` : `<span class="chip">${esc(t.status)}</span>`}
    ${t.assignee ? `<span class="faint">${esc(name(t.assignee))}</span>` : ""}${from}${edit ? `<button class="link" data-task-remove aria-label="Remove ${esc(t.title)}">Remove</button>` : ""}</div></li>`;
}

/**
 * The board: three columns, each taking a new task by typing its title.
 * @param {TeamTask[]} tasks @param {string} ws @param {(id: string) => string} name @param {boolean} edit
 */
function boardHtml(tasks, ws, name, edit) {
  return `<div class="team-board">${TEAM_STATUSES.map(([v, l]) => {
    const here = tasks.filter(t => t.status === v);
    return `<section class="team-col" data-col="${v}" aria-label="${l}"><h3>${l} <span class="small faint">${here.length}</span></h3><ul>${here.map(t => teamTaskCard(t, ws, name, edit)).join("")}</ul>
      ${edit ? `<form data-new-task="${v}"><input placeholder="Add a task" aria-label="New task in ${l}" autocomplete="off"></form>` : ""}</section>`;
  }).join("")}</div>`;
}

/**
 * The panel's overview: the computer, the agents at work, tasks and files.
 * @param {TeamPanelData} d @param {string} ws @param {?string} [channel] the channel open beside it
 */
function overviewHtml(d, ws, channel = null) {
  const home = Team.home, project = d.project?.id;
  const link = tab => project ? teamHref(ws, { kind: "project", id: project, tab }) : "";
  const doing = d.tasks.filter(t => t.status !== "done").slice(0, 5);
  const status = d.work.length ? `${esc(home.agent.name)} is working ${d.work[0].waiting ? `and waiting for ${esc(d.work[0].requester_name)}` : `for ${esc(d.work[0].requester_name)}`}` : "Ready";
  return `<section class="team-card"><h3>Team computer</h3>${d.computer ? `<div class="team-screen"><img data-team-screen="${esc(d.computer)}" alt="The team computer's screen"${TeamScreens[d.computer] ? ` src="${TeamScreens[d.computer]}"` : ""}><div class="small"><strong>${status}</strong></div><div class="small faint" data-team-screen-caption="${esc(d.computer)}">${esc(TeamScreenCaptions[d.computer] || "")}</div>
      <div class="row small"><a href="#/computer/${enc(d.computer)}/screen">Watch</a>${home.role === "guest" ? "" : `<button class="link" data-take-over="${esc(d.computer)}">Take control</button>`}</div></div>` : noTeamComputer(home)}</section>
    <section class="team-card"><h3>Agents at work</h3>${d.work.length ? `<ul class="team-work">${d.work.map(w => `<li><a href="${teamHref(ws, { kind: "channel", id: w.channel })}">${esc(home.agent.name)}</a> <span class="small faint">for ${esc(w.requester_name)}${w.waiting ? " · waiting on an ask" : ` · step ${w.steps + 1}`}</span></li>`).join("")}</ul>` : `<p class="small faint">Nothing running.</p>`}</section>
    <section class="team-card"><h3>Tasks ${project ? `<a class="small" href="${link("board")}">Board</a>` : ""}</h3>${d.progress ? progressBar(d.progress) : ""}${doing.length ? `<ul class="team-mini">${doing.map(t => `<li><span>${esc(t.title)}</span><span class="chip${t.status === "doing" ? " run" : ""}">${t.status === "doing" ? "Doing" : "To do"}</span></li>`).join("")}</ul>` : `<p class="small faint">Nothing open.</p>`}</section>
    <section class="team-card"><h3>Recent files ${project ? `<a class="small" href="${link("files")}">All files</a>` : ""}</h3>${d.files.length ? `<ul class="team-mini">${d.files.slice(-5).reverse().map(f => `<li><span>${esc(f.name)}</span><span class="small faint">${esc(f.by_name)}</span></li>`).join("")}</ul>` : `<p class="small faint">No files yet. Drop one into the conversation.</p>`}</section>
    <section class="team-card"><h3>Members</h3><div class="team-faces">${d.members.map(m => avatar(m.name)).join("")}${avatar(home.agent.name, true)}</div></section>${spendHtml(home, channel)}`;
}

/**
 * What the team agent spent this month against the owner's limits: the
 * workspace's and this channel's. An owner sets them from the command menu.
 * @param {TeamHome} home @param {?string} channel
 */
function spendHtml(home, channel) {
  const s = home.spend;
  if (!s) return "";
  const here = channel ? s.channels?.[channel] : null;
  const line = (what, spent, limit) => `<li><span>${what}</span><span class="small${limit != null && spent >= limit ? " bad" : " faint"}">${teamUsd(spent)}${limit != null ? ` of ${teamUsd(limit)}` : ""}</span></li>`;
  const c = channel && home.channels.find(x => x.id === channel);
  return `<section class="team-card" data-spend><h3>Spend this month</h3><ul class="team-mini">${line("Workspace", s.spent_usd, s.limit_usd)}${here ? line(c ? channelTitle(home, c) : "This channel", here.spent_usd, here.limit_usd) : ""}</ul>
    ${home.role === "owner" ? `<p class="small faint">Type an amount in the command menu (Ctrl K) to set a limit.</p>` : s.limit_usd == null && here?.limit_usd == null ? `<p class="small faint">No limit set.</p>` : ""}</section>`;
}

const TeamPanel = {
  s: null,

  async load(ws, project) {
    return api("GET", `/v1/workspaces/${enc(ws)}/panel${project ? `?project=${enc(project)}` : ""}`);
  },

  /** The right panel beside a channel: its project's, or the workspace's. */
  async side(el, ws, project, channel = null) {
    if (!el) return;
    const d = await this.load(ws, project).catch(() => null);
    if (!d || !el.isConnected) return;
    const p = Team.home.projects.find(x => x.id === project);
    paint(el, `<header class="team-side-head"><h2>${p ? `<a href="${teamHref(ws, { kind: "project", id: p.id })}">${esc(p.name)}</a>` : esc(Team.home.name)}</h2></header><div class="team-scroll">${overviewHtml(d, ws, channel)}</div>`);
    this.look(el, d.computer);
  },

  /** One look at the team computer's screen, each time the panel refreshes. */
  async look(el, id) {
    if (!id) return;
    const c = await api("GET", `/v1/computers/${enc(id)}`).catch(() => null);
    if (c && typeof screenCaption === "function") {
      TeamScreenCaptions[id] = screenCaption(c, null);
      const cap = $(`[data-team-screen-caption="${id}"]`, el);
      if (cap) cap.textContent = TeamScreenCaptions[id];
      if (c.status !== "running") return;
    }
    try {
      const { image_b64 } = await api("GET", `/v1/computers/${enc(id)}/screen`);
      TeamScreens[id] = `data:image/png;base64,${image_b64}`;
      const img = $(`[data-team-screen="${id}"]`, el);
      if (img) img.src = TeamScreens[id];
    } catch { /* asleep or busy: the next refresh looks again */ }
  },

  /** A project's own page, with its tabs. */
  async project(center, side, ws, place) {
    this.s = { center, side, ws, id: place.id, tab: place.tab || "overview" };
    await this.refresh(true);
    bindInvite(center, ws);
    center.addEventListener("click", e => this.click(e));
    center.addEventListener("change", e => this.change(e));
    center.addEventListener("submit", e => this.submit(e));
    center.addEventListener("dragstart", e => { const t = e.target.closest?.("[data-task]"); if (t) e.dataTransfer.setData("text/plain", t.dataset.task); });
    center.addEventListener("dragover", e => { if (e.target.closest?.("[data-col]")) e.preventDefault(); });
    center.addEventListener("drop", e => {
      const col = e.target.closest?.("[data-col]"), id = e.dataTransfer?.getData("text/plain");
      if (col && id) { e.preventDefault(); this.move(id, col.dataset.col); }
    });
  },

  async refresh(first = false) {
    const s = this.s;
    if (!s || !s.center.isConnected || Team.place.kind !== "project") return;
    const d = s.data = await this.load(s.ws, s.id);
    if (s !== this.s) return;
    this.draw(first);
  },

  name(id) { return id === "agent" ? Team.home.agent.name : Team.home.members.find(m => m.id === id)?.name || id; },

  draw() {
    const s = this.s, d = s.data, ws = s.ws, edit = Team.home.role !== "guest";
    const fresh = Team.justMade === s.id ? `<div class="banner" data-just-made><strong>${esc(d.project.name)} is ready.</strong> Invite people now, or add them later by mentioning them.
      <div class="row"><a href="${teamHref(ws, { kind: "project", id: s.id, tab: "members" })}">Add members</a>${["admin", "owner"].includes(Team.home.role) ? inviteForm() : ""}<button class="link" data-dismiss-made>Later</button></div></div>` : "";
    const head = `${fresh}<header class="team-head"><div><h1>${esc(d.project.name)}</h1>${progressBar(d.progress)}</div></header>
      <nav class="tabs" aria-label="Project">${TEAM_PROJECT_TABS.map(([k, l]) => `<a href="${teamHref(ws, { kind: "project", id: s.id, tab: k })}"${k === s.tab ? ' aria-current="page"' : ""}>${l}</a>`).join("")}</nav>`;
    let body = "";
    if (s.tab === "overview") {
      const chans = Team.home.channels.filter(c => d.channels.includes(c.id));
      body = `<h2>Channels</h2><ul class="team-mini">${chans.map(c => `<li><a href="${teamHref(ws, { kind: "channel", id: c.id })}">#${esc(c.name)}</a>${railCount(c)}</li>`).join("")}</ul>${boardHtml(d.tasks, ws, n => this.name(n), edit)}`;
    }
    if (s.tab === "board") body = boardHtml(d.tasks, ws, n => this.name(n), edit);
    if (s.tab === "files") body = `${d.files.length ? `<ul class="team-files">${d.files.map(f => `<li data-team-file="${esc(f.name)}"><span>${esc(f.name)}</span><span class="small faint">${esc(f.by_name)} · ${esc(kb(f.size))}</span><button class="link" data-file-save>Download</button>${edit ? `<button class="link" data-file-delete>Delete</button>` : ""}</li>`).join("")}</ul>` : `<p class="empty">No files yet.</p>`}
      ${edit ? `<label class="team-drop" data-drop-files>Drop files here, or <input type="file" multiple data-project-files aria-label="Add files"></label>` : ""}`;
    if (s.tab === "knowledge") body = `<div id="tp-knowledge"><p class="small faint">Loading…</p></div>`;
    if (s.tab === "members") {
      const owner = d.project.owner === Team.home.me, inIt = new Set(d.members.map(m => m.id));
      const others = Team.home.members.filter(m => !inIt.has(m.id));
      body = `<ul class="team-people">${d.members.map(m => `<li>${avatar(m.name)}<span>${esc(m.name)}</span>${m.id === d.project.owner ? `<span class="small faint">owner</span>` : owner ? `<button class="link" data-project-drop="${esc(m.id)}">Remove</button>` : ""}</li>`).join("")}<li>${avatar(Team.home.agent.name, true)}<span>${esc(Team.home.agent.name)}</span><span class="small faint">team agent</span></li></ul>
        ${owner && others.length ? `<form class="row" data-project-add><select aria-label="Add a member of the workspace">${others.map(m => `<option value="${esc(m.id)}">${esc(m.name)}</option>`).join("")}</select><button class="primary">Add to the project</button></form>` : ""}
        ${["admin", "owner"].includes(Team.home.role) ? `<h3>Invite someone new to the workspace</h3>${inviteForm()}` : ""}`;
    }
    paint(s.center, `${head}<div class="team-scroll">${body}</div>`);
    paint(s.side, `<header class="team-side-head"><h2>${esc(d.project.name)}</h2></header><div class="team-scroll">${overviewHtml(d, ws)}</div>`);
    this.look(s.side, d.computer);
    if (s.tab === "knowledge") this.knowledge();
  },

  async knowledge() {
    const s = this.s, base = `/v1/workspaces/${enc(s.ws)}/knowledge`;
    const entries = s.entries = await api("GET", `${base}?project=${enc(s.id)}`).catch(() => []);
    const el = $("#tp-knowledge", s.center);
    if (!el) return;
    const curator = s.data.project && (s.data.project.owner === Team.home.me || (s.data.project.curators || []).includes(Team.home.me));
    const types = [...new Set([...TEAM_KB_TYPES, ...entries.map(k => k.kind)])];
    paint(el, `<div class="row"><span class="grow small faint">Durable facts the team and ${esc(Team.home.agent.name)} rely on. ${curator ? "You keep them right here." : "The project's owner and curators keep them right."}</span><button class="link" data-kb-export>Export</button></div>
      ${entries.length ? `<ul class="team-kb">${entries.map(k => kbEntryHtml(k, entries, n => this.name(n), curator)).join("")}</ul>` : `<p class="empty">Nothing recorded yet. ${esc(Team.home.agent.name)} adds what it learns here, and you can too.</p>`}
      ${Team.home.role === "guest" ? "" : `<form data-new-entry class="team-kb-new"><select aria-label="Type">${types.map(t => `<option>${esc(t)}</option>`).join("")}</select><textarea rows="2" placeholder="Something the team should know" aria-label="Statement"></textarea><div class="row"><button class="primary">Add</button></div></form>`}`);
  },

  /** A task moved, shown at once. */
  async move(id, status) {
    const s = this.s, t = s.data.tasks.find(x => x.id === id);
    if (!t || t.status === status) return;
    const was = t.status;
    t.status = status;
    if (s.data.progress) s.data.progress.done += (status === "done") - (was === "done");
    this.draw();
    try { await api("PUT", `/v1/workspaces/${enc(s.ws)}/tasks/${enc(id)}`, { status }); } catch (err) { toast(err.message); await this.refresh(); }
  },

  async change(e) {
    if (e.target.matches("[data-task-status]")) this.move(e.target.closest("[data-task]").dataset.task, e.target.value);
    if (e.target.matches("[data-project-files]")) { await this.upload(e.target.files); e.target.value = ""; }
  },

  async upload(files) {
    const s = this.s;
    for (const f of files) {
      try { await Magnus.putBytes(`/v1/workspaces/${enc(s.ws)}/files/${enc(f.name)}?project=${enc(s.id)}`, new Uint8Array(await f.arrayBuffer())); } catch (err) { toast(err.message); }
    }
    await this.refresh();
  },

  async submit(e) {
    const s = this.s, f = e.target;
    if (f.matches("[data-new-task]")) {
      e.preventDefault();
      const input = $("input", f), title = input.value.trim(), status = f.dataset.newTask;
      if (!title) return;
      const tmp = { id: `tmp-${Date.now()}`, project: s.id, title, status, assignee: null, from: null, by: Team.home.me };
      s.data.tasks.push(tmp); if (s.data.progress) s.data.progress.total++;
      this.draw();
      $(`[data-new-task="${status}"] input`, s.center)?.focus();
      try { const r = await api("POST", `/v1/workspaces/${enc(s.ws)}/tasks`, { title, project: s.id, status }); tmp.id = r.id; } catch (err) { toast(err.message); await this.refresh(); }
    }
    if (f.matches("[data-new-entry]")) {
      e.preventDefault();
      const statement = $("textarea", f).value.trim();
      if (!statement) return;
      try { await api("POST", `/v1/workspaces/${enc(s.ws)}/knowledge`, { statement, kind: $("select", f)?.value || undefined, project: s.id }); this.knowledge(); } catch (err) { toast(err.message); }
    }
    if (f.matches("[data-entry-fix]")) {
      e.preventDefault();
      const statement = $("input", f).value.trim(), id = f.closest("[data-entry]").dataset.entry;
      if (!statement) return;
      try { await api("POST", `/v1/workspaces/${enc(s.ws)}/knowledge/${enc(id)}/correct`, { statement }); toast("Corrected."); } catch (err) { toast(err.message); }
      this.knowledge();
    }
    if (f.matches("[data-project-add]")) {
      e.preventDefault();
      const members = [...s.data.project.members, $("select", f).value];
      try { await api("PUT", `/v1/workspaces/${enc(s.ws)}/projects/${enc(s.id)}`, { members }); await loadHome(s.ws); await this.refresh(); } catch (err) { toast(err.message); }
    }
  },

  async click(e) {
    const s = this.s, t = e.target;
    const file = t.closest("[data-team-file]")?.dataset.teamFile, at = file && `/v1/workspaces/${enc(s.ws)}/files/${enc(file)}`;
    const entry = t.closest("[data-entry]")?.dataset.entry, kat = entry && `/v1/workspaces/${enc(s.ws)}/knowledge/${enc(entry)}`;
    if (t.closest("[data-dismiss-made]")) { Team.justMade = null; this.draw(); return; }
    try {
      if (t.closest("[data-task-remove]")) {
        const id = t.closest("[data-task]").dataset.task;
        s.data.tasks = s.data.tasks.filter(x => x.id !== id); this.draw();
        await api("DELETE", `/v1/workspaces/${enc(s.ws)}/tasks/${enc(id)}`);
      }
      if (t.closest("[data-file-save]")) await Magnus.download(at, file);
      if (t.closest("[data-file-delete]") && await ask(`Delete ${file}?`, "Delete")) { await api("DELETE", at); await this.refresh(); }
      if (t.closest("[data-entry-flag]")) {
        const why = prompt("What is wrong with it?");
        if (why) { await api("POST", `${kat}/flag`, { why }); toast("Flagged."); this.knowledge(); }
      }
      if (t.closest("[data-entry-correct]")) {
        const li = t.closest("[data-entry]"), was = s.entries?.find(x => x.id === entry)?.statement || "";
        paint($(".team-kb-head .grow", li), `<form data-entry-fix class="row"><input aria-label="Corrected statement" value="${esc(was)}"><button class="primary">Save</button></form>`);
        $("[data-entry-fix] input", li)?.focus();
      }
      if (t.closest("[data-kb-export]")) await Magnus.download(`/v1/workspaces/${enc(s.ws)}/knowledge/export`, "knowledge.json");
      if (t.closest("[data-entry-retire]") && await ask("Retire this entry?", "Retire")) { await api("POST", `${kat}/retire`, {}); this.knowledge(); }
      if (t.closest("[data-project-drop]")) {
        const members = s.data.project.members.filter(m => m !== t.closest("[data-project-drop]").dataset.projectDrop);
        await api("PUT", `/v1/workspaces/${enc(s.ws)}/projects/${enc(s.id)}`, { members }); await this.refresh();
      }
    } catch (err) { toast(err.message); await this.refresh(); }
  },
};

document.addEventListener("drop", e => {
  const zone = e.target.closest?.("[data-drop-files]");
  if (!zone || !e.dataTransfer?.files?.length || !TeamPanel.s) return;
  e.preventDefault();
  TeamPanel.upload(e.dataTransfer.files);
});
document.addEventListener("dragover", e => { if (e.target.closest?.("[data-drop-files]")) e.preventDefault(); });
