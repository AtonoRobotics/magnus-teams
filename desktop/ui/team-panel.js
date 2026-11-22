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

/** The side panel's tabs: clean Work + People (no Kanban/Files/Computer clutter). */
const TEAM_SIDE_TABS = [["activity", "Work"], ["members", "People"]];

/** A brand's logo for what a row names, when one is known. */
function teamBrand(text) {
  return SidePanel.brand(text);
}

/** Where the agent's work stands, for the header's status. @param {TeamPanelData} d */
function teamSideStatus(d) {
  const waiting = d.work.filter(w => w.waiting).length, working = d.work.length - waiting;
  if (waiting) return { dot: "warn", label: `${waiting} waiting on someone` };
  if (working) return { dot: "run", label: `Working · ${working} active` };
  return { dot: "ok", label: "Ready" };
}

/**
 * The panel's head: who works here and how it stands, in one row, then the
 * tabs.
 * @param {TeamPanelData} d @param {string} ws @param {string} tab
 */
function teamSideHead(d, ws, tab) {
  const home = Team.home, st = teamSideStatus(d);
  const titleHtml = esc(home.name);
  const people = d.members.length;
  const subText = `${people} ${people === 1 ? "person" : "people"} · Isolated personal agents`;
  return `${SidePanel.head({ letter: home.agent.name || "M", dot: st.dot, tone: st.dot, label: st.label, titleHtml, subText })}${SidePanel.tabs(TEAM_SIDE_TABS, tab, { label: "Team panel", buttonAttr: "data-team-side-tab" })}`;
}

/**
 * The panel's foot: clean workspace memory link (retires shared Team Computer VM).
 */
function teamSideFoot() {
  return "";
}

/**
 * The Work tab: active Room Missions & Airlock, inline Start Shared Work,
 * waiting items, active agents, delegations, and scheduled routines.
 * @param {TeamPanelData} d @param {string} ws @param {?string} [channel]
 */
function overviewHtml(d, ws, channel = null) {
  const home = Team.home, agent = esc(home.agent.name);
  const where = id => { const c = home.channels.find(x => x.id === id); return c ? esc(channelTitle(home, c)) : ""; };
  const waiting = d.work.filter(w => w.waiting), busy = d.work.filter(w => !w.waiting);
  const banner = SidePanel.waitingBanner({
    several: true,
    count: waiting.length,
    rowsHtml: waiting.map(w => `<a class="gbox-row" href="${teamHref(ws, { kind: "channel", id: w.channel })}"><span class="gbox-ic warn" aria-hidden="true">${icon("need")}</span><span class="gbox-main"><span class="gbox-lbl">${agent} is waiting for ${esc(w.requester_name)}</span><span class="small muted">${where(w.channel)}</span></span></a>`).join(""),
  });
  const active = busy[0] ? SidePanel.activeCard({
    href: teamHref(ws, { kind: "channel", id: busy[0].channel }),
    rightMeta: `Step ${busy[0].steps + 1}`,
    title: `${home.agent.name} is working for ${busy[0].requester_name}`,
    footHtml: `<span class="small muted">${where(busy[0].channel)}</span>`,
    inspectLabel: "Open →",
  }) : "";
  const missions = d.missions || [];
  const roomWork = typeof TeamRooms !== "undefined" && missions.length
    ? TeamRooms.workPanelHtml(missions, ws, home.me, id => home.members.find(m => m.id === id)?.name || id)
    : `<div class="room-empty-work"><div class="small"><strong>Work Canvas</strong></div><p class="small muted">Start shared work below or mention @${agent} in chat. Your requests share automatically; teammates review their own slice privately first.</p></div>`;
  const others = home.members.filter(m => m.id !== home.me);
  const memberPicks = others.map(m => `<label class="room-pick small"><input type="checkbox" name="member" value="${esc(m.id)}"> ${esc(m.name)}</label>`).join("");
  const startMission = channel && home.role !== "guest" ? `<section class="companion-sec"><div class="companion-sec-head"><span>Start shared work</span></div>
    <form class="room-mission-form" data-mission-new data-mission-room="${esc(channel)}">
      <div class="room-steer-row"><input name="goal" placeholder="e.g. Draft Friday's board update…" aria-label="Shared work goal" autocomplete="off"><button class="primary" type="submit">Start</button></div>
      <div class="room-picks">${memberPicks}<label class="room-pick small"><input type="checkbox" name="open_web" value="1"> Public web</label></div>
    </form></section>` : "";
  return `${banner}${roomWork}${startMission}${active}${delegationsHtml(d, ws, where)}${routinesHtml(d, home, channel, where)}${spendHtml(home, channel)}`;
}

/**
 * Active and recent delegations from the team orchestrator to members'
 * personal agents (`team_ask_personal_agent`).
 * @param {TeamPanelData} d @param {string} ws @param {(id: string) => string} where
 */
function delegationsHtml(d, ws, where) {
  const list = d.delegations || [];
  if (!list.length) return "";
  const rows = list.map(x => {
    const ic = x.status === "waiting" ? "need" : x.status === "done" ? "done" : "working";
    const tone = x.status === "waiting" ? " warn" : x.status === "done" ? " ok" : "";
    const state = x.status === "waiting" ? "Waiting on personal approval" : x.status === "done" ? (x.shared ? "Shared to thread" : "Held privately") : "Working in personal agent";
    const sub = [where(x.channel), state, x.mine && x.summary ? firstLine(x.summary) : ""].filter(Boolean).join(" · ");
    return `<a class="gbox-row" data-delegation="${esc(x.task)}" href="${teamHref(ws, { kind: "channel", id: x.channel })}"><span class="gbox-ic${tone}" aria-hidden="true">${icon(ic)}</span><span class="gbox-main"><span class="gbox-lbl">${esc(x.member_name)}'s Magnus</span><span class="small muted">${esc(sub)}</span></span></a>`;
  }).join("");
  return `<section class="companion-sec" data-delegations><div class="companion-sec-head"><span>Personal agent delegations</span><span class="count">${list.length}</span></div><div class="gbox">${rows}</div></section>`;
}

/**
 * Scheduled routines in this workspace or project, with create, restart, and
 * remove controls for workspace admins.
 * @param {TeamPanelData} d @param {TeamHome} home @param {?string} channel @param {(id: string) => string} where
 */
function routinesHtml(d, home, channel, where) {
  const routines = d.routines || [], admin = isTeamAdmin(home);
  if (!routines.length && !admin) return "";
  const targetChannel = channel || d.channels?.[0] || home.channels?.[0]?.id || "";
  const rows = routines.map(r => {
    const cadence = r.every_hours === 24 ? "Daily" : r.every_hours % 24 === 0 ? `Every ${r.every_hours / 24}d` : `Every ${r.every_hours}h`;
    const stopLine = r.stopped ? `<span class="small bad">Stopped: ${esc(r.stopped)}</span>` : `<span class="small muted">${where(r.channel)} · ${esc(cadence)}</span>`;
    const acts = admin ? `<span class="row small">${r.stopped ? `<button class="link" data-routine-restart="${esc(r.id)}">Restart</button>` : ""}<button class="link" data-routine-remove="${esc(r.id)}">Remove</button></span>` : `<span class="small muted">${esc(cadence)}</span>`;
    return `<div class="gbox-row" data-routine="${esc(r.id)}"><span class="gbox-ic${r.stopped ? " warn" : ""}" aria-hidden="true">${icon("clock")}</span><span class="gbox-main"><span class="gbox-lbl">${esc(r.text)}</span>${stopLine}</span>${acts}</div>`;
  }).join("");
  const form = admin && targetChannel ? `<form class="row small" data-routine-new><input name="text" placeholder="Add a scheduled routine" aria-label="Routine instructions" autocomplete="off"><select name="every_hours" aria-label="Cadence"><option value="1">Hourly</option><option value="6">Every 6h</option><option value="24" selected>Daily</option><option value="168">Weekly</option></select><input type="hidden" name="channel" value="${esc(targetChannel)}"><button class="link" type="submit">Add</button></form>` : "";
  return `<section class="companion-sec" data-routines><div class="companion-sec-head"><span>Routines</span><span class="count">${routines.length}</span></div>${routines.length ? `<div class="gbox">${rows}</div>` : `<p class="small faint">No scheduled routines.</p>`}${form}</section>`;
}

/**
 * The Tasks tab: progress at a glance and what is open, to the board.
 * @param {TeamPanelData} d @param {string} ws
 */
function teamTasksHtml(d, ws) {
  const project = d.project?.id, open = d.tasks.filter(t => t.status !== "done");
  return `<section class="companion-sec"><div class="companion-sec-head"><span>Tasks</span>${project ? `<a class="small" href="${teamHref(ws, { kind: "project", id: project, tab: "board" })}">Board →</a>` : `<span class="count">${open.length}</span>`}</div>
    ${d.progress ? `<div class="team-side-progress">${progressBar(d.progress)}</div>` : ""}
    ${open.length ? `<div class="gbox">${open.map(t => `<div class="gbox-row">${teamBrand(t.title) || `<span class="gbox-ic${t.status === "doing" ? " run" : ""}" aria-hidden="true">${icon(t.status === "doing" ? "working" : "tasks")}</span>`}<span class="gbox-main"><span class="gbox-lbl">${esc(t.title)}</span>${t.assignee ? `<span class="small muted">${esc(TeamPanel.name(t.assignee))}</span>` : ""}</span><span class="chip${t.status === "doing" ? " run" : ""}">${t.status === "doing" ? "Doing" : "To do"}</span></div>`).join("")}</div>` : `<p class="small faint">Nothing open.</p>`}</section>`;
}

/**
 * The Files tab: the newest first, who added each and its size.
 * @param {TeamPanelData} d @param {string} ws
 */
function teamFilesHtml(d, ws) {
  const project = d.project?.id;
  return `<section class="companion-sec"><div class="companion-sec-head"><span>Files</span>${project ? `<a class="small" href="${teamHref(ws, { kind: "project", id: project, tab: "files" })}">All files →</a>` : `<span class="count">${d.files.length}</span>`}</div>
    ${d.files.length ? `<div class="gbox">${d.files.slice(-12).reverse().map(f => `<div class="gbox-row">${teamBrand(f.name) || `<span class="gbox-ic" aria-hidden="true">${icon("folder")}</span>`}<span class="gbox-main"><span class="gbox-lbl">${esc(f.name)}</span><span class="small muted">${esc(f.by_name)}</span></span><span class="mono small muted lib-size">${esc(kb(f.size))}</span></div>`).join("")}</div>` : `<p class="small faint">No files yet. Drop one into the conversation.</p>`}</section>`;
}

/**
 * The Members tab: who is here by name, and the way to the people page,
 * where a Team Admin adds and removes people and anyone sees who is in,
 * followed by how the team orchestrator delegates app requests to each
 * member's personal Magnus.
 * @param {TeamPanelData} d @param {string} ws
 */
function membersCardHtml(d, ws) {
  const home = Team.home, admin = isTeamAdmin(home);
  const face = m => `<span class="team-face" title="${esc(m.name)}">${avatar(m.name, false, m.id)}<span class="sr">${esc(m.name)}</span></span>`;
  const caps = d.capabilities || Team.capabilities?.[ws] || [];
  const capHtml = typeof TeamRooms !== "undefined" && caps.length
    ? `<section class="companion-sec"><div class="companion-sec-head"><span>People &amp; personal apps</span><a class="small" href="${teamHref(ws, { kind: "people" })}" data-members-manage>${admin ? "Add or remove" : "See all"}</a></div>${TeamRooms.peopleCapabilitiesHtml(caps, home.me)}</section>`
    : `<section class="companion-sec" data-members-card><div class="companion-sec-head"><span>Members</span><a class="small" href="${teamHref(ws, { kind: "people" })}" data-members-manage>${admin ? "Add or remove" : "See all"}</a></div><div class="gbox team-faces-box"><div class="team-faces">${d.members.map(face).join("")}${avatar(home.agent.name, true)}</div></div></section>`;
  const myAv = Team.avatars?.[home.me] || { kind: "default" };
  const avatarSec = typeof UserAvatar !== "undefined"
    ? `<section class="companion-sec"><div class="companion-sec-head"><span>Your avatar</span></div>${UserAvatar.pickerHtml(home.me_name || home.me, myAv)}</section>`
    : "";
  const apps = d.apps || home.apps || [];
  const appsSec = apps.length ? `<section class="companion-sec" data-team-apps-card><div class="companion-sec-head"><span>Connectors &amp; agents</span><a class="small" href="${teamHref(ws, { kind: "people" })}">${admin ? "Configure" : "View"}</a></div>
    <div class="gbox">${apps.map(a => `<div class="gbox-row"><span class="gbox-main"><span class="gbox-lbl">${esc(a.title || a.name)}</span><span class="small muted">${a.signs_in ? "Delegated to member's personal Magnus" : a.roles?.length ? `Workspace app (${a.roles.join(", ")})` : "Not granted"}</span></span></div>`).join("")}</div></section>` : "";
  return `${capHtml}${avatarSec}${appsSec}`;
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
  return `<section class="companion-sec" data-spend><div class="companion-sec-head"><span>Spend this month</span></div><div class="gbox team-spend"><ul class="team-mini">${line("Workspace", s.spent_usd, s.limit_usd)}${here ? line(c ? channelTitle(home, c) : "This channel", here.spent_usd, here.limit_usd) : ""}</ul></div>
    ${home.role === "owner" ? `<p class="small faint">Type an amount in the command menu (Ctrl K) to set a limit.</p>` : s.limit_usd == null && here?.limit_usd == null ? `<p class="small faint">No limit set.</p>` : ""}</section>`;
}

/**
 * The whole side panel, laid out by SidePanel: the head and tabs, the open
 * tab, and the team computer at the foot.
 * @param {TeamPanelData} d @param {string} ws @param {?string} channel @param {string} tab
 */
function teamSideHtml(d, ws, channel, tab) {
  const bodyHtml = tab === "tasks" ? teamTasksHtml(d, ws) : tab === "files" ? teamFilesHtml(d, ws) : tab === "members" ? membersCardHtml(d, ws) : overviewHtml(d, ws, channel);
  return SidePanel.shell({
    wrapClass: "team-companion",
    headHtml: teamSideHead(d, ws, tab),
    panelKey: `team-${tab}`,
    bodyHtml,
    footHtml: teamSideFoot(d),
  });
}

/** What each team computer was last seen doing, for the panel's foot. */
const TeamComputerStates = {};

const TeamPanel = {
  s: null,

  async load(ws, project) {
    return Server.workspaces.panel(ws, project);
  },

  /** The right panel beside a channel: its project's, or the workspace's. */
  async side(el, ws, project, channel = null) {
    if (!el || !el.isConnected) return;
    const [d, missions, caps] = await Promise.all([
      this.load(ws, project).catch(() => null),
      channel && Server.workspaces.missions ? Server.workspaces.missions(ws, channel).catch(() => []) : Promise.resolve([]),
      Server.workspaces.capabilities ? Server.workspaces.capabilities(ws).catch(() => []) : Promise.resolve([]),
    ]);
    if (!d || !el.isConnected) return;
    d.missions = missions || [];
    d.capabilities = caps || [];
    if (caps?.length) {
      Team.capabilities[ws] = caps;
      for (const m of caps) if (m.avatar) Team.avatars[m.id] = m.avatar;
    }
    this.paintSide(el, d, ws, channel);
  },

  /** The side panel's open tab, kept across refreshes. */
  sideTab: "activity",
  sideAt: null,

  /** Draws the side panel, and remembers what it drew so a tab redraws it. */
  paintSide(el, d, ws, channel) {
    this.sideAt = { el, d, ws, channel };
    paint(el, teamSideHtml(d, ws, channel, this.sideTab));
    if (typeof TeamRooms !== "undefined" && !el.dataset.roomsWired) {
      el.dataset.roomsWired = "1";
      TeamRooms.wire(el, ws, () => {
        const at = this.sideAt;
        if (at) this.side(at.el, at.ws, at.d?.project?.id || null, at.channel);
      });
      el.addEventListener("submit", async e => {
        const form = /** @type {HTMLFormElement} */ (e.target).closest("[data-mission-new]");
        if (!form) return;
        e.preventDefault();
        const goal = (/** @type {HTMLInputElement | null} */ (form.querySelector('input[name="goal"]'))?.value || "").trim();
        const room = /** @type {HTMLElement} */ (form).dataset.missionRoom || "";
        if (!goal || !room || !Server.workspaces.createMission) return;
        const at = this.sideAt;
        if (!at) return;
        const lanes = [{ kind: "member_agent", target_member: Team.home?.me, objective: goal, scope: "room_work" }];
        for (const cb of form.querySelectorAll('input[name="member"]:checked')) {
          const memberId = /** @type {HTMLInputElement} */ (cb).value;
          if (memberId) lanes.push({ kind: "member_agent", target_member: memberId, objective: goal, scope: "room_work" });
        }
        if (form.querySelector('input[name="open_web"]:checked')) {
          lanes.push({ kind: "open_web", objective: goal, scope: "public_research" });
        }
        await Server.workspaces.createMission(at.ws, room, { goal, lanes }).catch(() => {});
        this.side(at.el, at.ws, at.d?.project?.id || null, at.channel);
      });
      el.addEventListener("click", async e => {
        const t = /** @type {HTMLElement} */ (e.target);
        const preset = t.closest("[data-avatar-preset]");
        const def = t.closest("[data-avatar-default]");
        if (!preset && !def) return;
        const avatarVal = preset ? { kind: "preset", id: /** @type {HTMLElement} */ (preset).dataset.avatarPreset } : { kind: "default" };
        if (Server.profile?.setAvatar) {
          await Server.profile.setAvatar(avatarVal).catch(() => {});
          if (Team.home?.me) Team.avatars[Team.home.me] = avatarVal;
          const at = this.sideAt;
          if (at) this.paintSide(at.el, at.d, at.ws, at.channel);
        }
      });
    }
    this.look(el, d.computer);
  },

  /** One look at the team computer's screen, each time the panel refreshes. */
  async look(el, id) {
    if (!id) return;
    const c = await Server.computers.get(id).catch(() => null);
    if (c) {
      const awake = c.status === "running";
      TeamComputerStates[id] = awake ? "Awake" : "Asleep";
      const st = $(`[data-team-comp-state="${id}"]`, el), dot = $(`[data-team-comp-dot="${id}"]`, el);
      if (st) st.textContent = TeamComputerStates[id];
      if (dot) dot.className = `comp-dot ${awake ? "ok" : "idle"}`;
    }
    if (c && typeof screenCaption === "function") {
      TeamScreenCaptions[id] = screenCaption(c, null);
      const cap = $(`[data-team-screen-caption="${id}"]`, el);
      if (cap) cap.textContent = TeamScreenCaptions[id];
      if (c.status !== "running") return;
    }
    try {
      const { image_b64 } = await Server.computers.screen(id);
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
        ${isTeamAdmin(Team.home) ? `<h3>Invite someone new to the workspace</h3>${inviteForm()}<p class="small"><a href="${teamHref(ws, { kind: "people" })}" data-members-manage>Add or remove people in ${esc(Team.home.name)}</a></p>` : ""}`;
    }
    paint(s.center, `${head}<div class="team-scroll">${body}</div>`);
    this.paintSide(s.side, d, ws, null);
    if (s.tab === "knowledge") this.knowledge();
  },

  async knowledge() {
    const s = this.s;
    const entries = s.entries = await Server.workspaces.knowledge(s.ws, s.id).catch(() => []);
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
    try { await Server.workspaces.updateTask(s.ws, id, { status }); } catch (err) { toast(err.message); await this.refresh(); }
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
      try { const r = await Server.workspaces.addTask(s.ws, { title, project: s.id, status }); tmp.id = r.id; } catch (err) { toast(err.message); await this.refresh(); }
    }
    if (f.matches("[data-new-entry]")) {
      e.preventDefault();
      const statement = $("textarea", f).value.trim();
      if (!statement) return;
      try { await Server.workspaces.addKnowledge(s.ws, { statement, kind: $("select", f)?.value || undefined, project: s.id }); this.knowledge(); } catch (err) { toast(err.message); }
    }
    if (f.matches("[data-entry-fix]")) {
      e.preventDefault();
      const statement = $("input", f).value.trim(), id = f.closest("[data-entry]").dataset.entry;
      if (!statement) return;
      try { await Server.workspaces.correctKnowledge(s.ws, id, statement); toast("Corrected."); } catch (err) { toast(err.message); }
      this.knowledge();
    }
    if (f.matches("[data-project-add]")) {
      e.preventDefault();
      const members = [...s.data.project.members, $("select", f).value];
      try { await Server.workspaces.setProjectMembers(s.ws, s.id, members); await loadHome(s.ws); await this.refresh(); } catch (err) { toast(err.message); }
    }
  },

  async click(e) {
    const s = this.s, t = e.target;
    const file = t.closest("[data-team-file]")?.dataset.teamFile, at = file && `/v1/workspaces/${enc(s.ws)}/files/${enc(file)}`;
    const entry = t.closest("[data-entry]")?.dataset.entry;
    if (t.closest("[data-dismiss-made]")) { Team.justMade = null; this.draw(); return; }
    try {
      if (t.closest("[data-task-remove]")) {
        const id = t.closest("[data-task]").dataset.task;
        s.data.tasks = s.data.tasks.filter(x => x.id !== id); this.draw();
        await Server.workspaces.removeTask(s.ws, id);
      }
      if (t.closest("[data-file-save]")) await Magnus.download(at, file);
      if (t.closest("[data-file-delete]") && await ask(`Delete ${file}?`, "Delete")) { await Server.workspaces.removeFile(s.ws, file); await this.refresh(); }
      if (t.closest("[data-entry-flag]")) {
        const why = await askText("What is wrong with it?", "Describe what should be corrected", "Flag");
        if (why) { await Server.workspaces.flagKnowledge(s.ws, entry, why); toast("Flagged."); this.knowledge(); }
      }
      if (t.closest("[data-entry-correct]")) {
        const li = t.closest("[data-entry]"), was = s.entries?.find(x => x.id === entry)?.statement || "";
        paint($(".team-kb-head .grow", li), `<form data-entry-fix class="row"><input aria-label="Corrected statement" value="${esc(was)}"><button class="primary">Save</button></form>`);
        $("[data-entry-fix] input", li)?.focus();
      }
      if (t.closest("[data-kb-export]")) await Magnus.download(`/v1/workspaces/${enc(s.ws)}/knowledge/export`, "knowledge.json");
      if (t.closest("[data-entry-retire]") && await ask("Retire this entry?", "Retire")) { await Server.workspaces.retireKnowledge(s.ws, entry); this.knowledge(); }
      if (t.closest("[data-project-drop]")) {
        const members = s.data.project.members.filter(m => m !== t.closest("[data-project-drop]").dataset.projectDrop);
        await Server.workspaces.setProjectMembers(s.ws, s.id, members); await this.refresh();
      }
    } catch (err) { toast(err.message); await this.refresh(); }
  },
};

document.addEventListener("click", async e => {
  const b = e.target.closest?.("[data-team-side-tab]"), at = TeamPanel.sideAt;
  if (b && at && at.el.contains(b)) {
    TeamPanel.sideTab = b.dataset.teamSideTab;
    TeamPanel.paintSide(at.el, at.d, at.ws, at.channel);
    return;
  }
  const restart = e.target.closest?.("[data-routine-restart]"), remove = e.target.closest?.("[data-routine-remove]");
  if ((!restart && !remove) || !at || !at.el.contains(restart || remove)) return;
  try {
    if (restart) {
      const id = restart.dataset.routineRestart;
      await Server.workspaces.updateRoutine(at.ws, id, {});
      await TeamPanel.side(at.el, at.ws, at.d.project?.id || null, at.channel);
    }
    if (remove) {
      const id = remove.dataset.routineRemove;
      await Server.workspaces.removeRoutine(at.ws, id);
      await TeamPanel.side(at.el, at.ws, at.d.project?.id || null, at.channel);
    }
  } catch (err) { toast(err.message); }
});
document.addEventListener("submit", async e => {
  const form = e.target.closest?.("[data-routine-new]"), at = TeamPanel.sideAt;
  if (!form || !at || !at.el.contains(form)) return;
  e.preventDefault();
  const text = form.querySelector('[name="text"]')?.value.trim();
  const every_hours = Number(form.querySelector('[name="every_hours"]')?.value) || 24;
  const channel = form.querySelector('[name="channel"]')?.value;
  if (!text || !channel) return;
  try {
    await Server.workspaces.addRoutine(at.ws, { channel, text, every_hours });
    await TeamPanel.side(at.el, at.ws, at.d.project?.id || null, at.channel);
  } catch (err) { toast(err.message); }
});
document.addEventListener("drop", e => {
  const zone = e.target.closest?.("[data-drop-files]");
  if (!zone || !e.dataTransfer?.files?.length || !TeamPanel.s) return;
  e.preventDefault();
  TeamPanel.upload(e.dataTransfer.files);
});
document.addEventListener("dragover", e => { if (e.target.closest?.("[data-drop-files]")) e.preventDefault(); });
