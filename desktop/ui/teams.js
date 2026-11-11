// Workspaces: a team chat and project app with agents as members. Three
// panes: the rail (projects, channels, direct messages, agents), the
// conversation, and the open project's panel. Nothing is set up through a
// step-by-step form: things are made by typing their name, and the team
// agent is in every channel from the start. The conversation is in
// team-chat.js, the right panel in team-panel.js and the command menu in
// team-command.js.
UI.team = '<circle cx="9" cy="8" r="3"/><path d="M3 20a6 6 0 0 1 12 0"/><path d="M16 5.5a3 3 0 0 1 0 5M18 14.5a6 6 0 0 1 3 5.5"/>';
const TEAM_REFRESH_MS = 3000, TEAM_ROLES = ["guest", "member", "admin", "owner"];

/**
 * @typedef {{ id: string, name: string, role: string, online: boolean }} TeamMember
 * @typedef {{ id: string, name: string, project: ?string, mode: "mentions"|"when_helpful", notify: "all"|"mentions"|"off", unread: number, mentions: number, direct: ?{ people: string[], agent: boolean } }} TeamChannel
 * @typedef {{ id: string, name: string, owner: string, members: string[], progress: { done: number, total: number }, channels: string[] }} TeamProject
 * @typedef {{ spent_usd: number, limit_usd: ?number, channels: Object<string, { spent_usd: number, limit_usd: ?number }> }} TeamSpend
 * @typedef {{ id: string, name: string, role: string, me: string, me_name: string, agent: { id: string, name: string }, members: TeamMember[], channels: TeamChannel[], projects: TeamProject[], computer: ?string, spend?: TeamSpend }} TeamHome
 * @typedef {{ kind: "channel"|"project"|"activity"|"search"|"people", id?: string, thread?: string, tab?: string, q?: string }} TeamPlace
 */

/** What the open workspace looks like now, kept between views so a switch paints at once. */
const Team = { ws: null, /** @type {?TeamHome} */ home: null, /** @type {TeamPlace} */ place: { kind: "channel" }, homes: {}, loop: 0 };

route(/^#\/teams$/, main => showTeams(main));
route(/^#\/join\/([^/]+)$/, (main, code) => joinTeam(main, code));
route(/^#\/team\/([^/]+)(\/.*)?$/, (main, ws, rest) => showTeam(main, ws, teamPlace(rest || "")));

/** Where a hash's tail points inside a workspace. Old links still land. */
function teamPlace(rest) {
  const decode = s => { try { return decodeURIComponent(s); } catch { return s; } };
  const [kind, id, sub, more] = rest.split("/").filter(Boolean).map(decode);
  if (kind === "c" || kind === "channels") return { kind: "channel", id, thread: sub === "t" ? more : undefined };
  if (kind === "p") return { kind: "project", id, tab: sub || "overview" };
  if (kind === "activity") return { kind: "activity" };
  if (kind === "search") return { kind: "search", q: id || "" };
  if (kind === "people" || kind === "members") return { kind: "people" };
  if (kind === "files" || kind === "knowledge") return { kind: "project", id: "", tab: kind };
  return { kind: "channel" };
}

/** The hash for a place in a workspace. */
function teamHref(ws, place) {
  const base = `#/team/${enc(ws)}`;
  if (place.kind === "channel") return place.id ? `${base}/c/${enc(place.id)}${place.thread ? `/t/${enc(place.thread)}` : ""}` : base;
  if (place.kind === "project") return `${base}/p/${enc(place.id || "")}${place.tab && place.tab !== "overview" ? `/${place.tab}` : ""}`;
  if (place.kind === "search") return `${base}/search/${enc(place.q || "")}`;
  return `${base}/${place.kind}`;
}

async function showTeams(main) {
  const list = await api("GET", "/v1/workspaces");
  paint(main, `<div class="teams-page"><h1>Teams</h1><p class="small faint">Workspaces you share with other people and their agents. Your own conversations stay yours.</p>${list.length ? `<ul class="team-list">${list.map(w => `<li data-workspace="${esc(w.id)}"><a href="#/team/${enc(w.id)}">${avatar(w.name)}<span><strong>${esc(w.name)}</strong><br><span class="small faint">${esc(w.role)}</span></span></a></li>`).join("")}</ul>` : `<p class="empty">You are in no workspace yet. Type a name to start one; the team agent is in it from the start.</p>`}
    <form id="new-team"><input id="team-name" placeholder="Type a name to start a workspace" aria-label="Workspace name" autocomplete="off"><button class="primary">Create</button></form></div>`);
  main.onsubmit = async e => {
    if (e.target.id !== "new-team") return;
    e.preventDefault();
    const name = $("#team-name", main).value.trim();
    if (!name) return;
    try { const w = await api("POST", "/v1/workspaces", { name }); location.hash = `#/team/${enc(w.id)}`; } catch (err) { toast(err.message); }
  };
}

async function joinTeam(main, code) {
  paint(main, `<p class="empty">Joining…</p>`);
  try { const r = await api("POST", `/v1/invites/${enc(code)}/join`, {}); location.hash = `#/team/${enc(r.workspace)}`; }
  catch (err) { paint(main, `<div class="banner bad">${esc(err.message)}</div><p><a href="#/teams">Teams</a></p>`); }
}

/** An avatar: initials, or the agent's mark. */
function avatar(name, agent = false) {
  const letters = agent ? "M" : String(name || "?").split(/\s+/).map(w => w[0]).join("").slice(0, 2).toUpperCase();
  return `<span class="team-avatar${agent ? " agent" : ""}" aria-hidden="true">${esc(letters)}</span>`;
}

/** A direct conversation's title: who else is in it, or the agent. */
function directName(home, c) {
  if (c.direct?.agent) return home.agent.name;
  const others = c.direct.people.filter(p => p !== home.me);
  return others.map(p => home.members.find(m => m.id === p)?.name || p).join(", ") || `${home.me_name} (you)`;
}

/** A channel's name as the rail and header show it. */
function channelTitle(home, c) { return c.direct ? directName(home, c) : `#${c.name}`; }

/** The count beside a rail entry: mentions loud, unread quiet, muted none. */
function railCount(c) {
  if (c.mentions) return `<span class="team-count loud" aria-label="${c.mentions} mentions">${c.mentions}</span>`;
  if (c.unread && c.notify !== "off") return `<span class="team-count" aria-label="${c.unread} unread">${c.unread}</span>`;
  return "";
}

/**
 * The left rail: projects, channels, agents and direct messages.
 * @param {TeamHome} home @param {TeamPlace} here
 */
function railHtml(home, here) {
  const at = (kind, id) => here.kind === kind && here.id === id ? ' aria-current="page"' : "";
  const ws = home.id;
  const open = home.channels.filter(c => !c.direct);
  const projectCount = p => open.filter(c => c.project === p.id).reduce((n, c) => n + c.mentions, 0);
  const projects = home.projects.map(p => `<a href="${teamHref(ws, { kind: "project", id: p.id })}"${at("project", p.id)} data-project="${esc(p.id)}"><span class="team-dot" aria-hidden="true"></span>${esc(p.name)}${p.progress.total ? `<span class="small faint">${p.progress.done}/${p.progress.total}</span>` : ""}${projectCount(p) ? `<span class="team-count loud">${projectCount(p)}</span>` : ""}</a>`).join("");
  const channels = open.map(c => `<a href="${teamHref(ws, { kind: "channel", id: c.id })}"${at("channel", c.id)} data-channel="${esc(c.id)}" class="${c.unread && c.notify !== "off" ? "unread" : ""}${c.notify === "off" ? " muted" : ""}">#${esc(c.name)}${railCount(c)}</a>`).join("");
  const agentDm = home.channels.find(c => c.direct?.agent);
  const agents = `<a href="${agentDm ? teamHref(ws, { kind: "channel", id: agentDm.id }) : "#"}" data-direct-agent${agentDm ? at("channel", agentDm.id) : ""}>${avatar(home.agent.name, true)}${esc(home.agent.name)} <span class="small faint">team</span>${agentDm ? railCount(agentDm) : ""}</a>
    <a href="#/chat/new" title="Your own agent: private to you">${avatar("Your Magnus", true)}Your Magnus <span class="small faint">private</span></a>`;
  const dms = home.channels.filter(c => c.direct && !c.direct.agent).map(c => {
    const other = c.direct.people.find(p => p !== home.me), on = home.members.find(m => m.id === other)?.online;
    return `<a href="${teamHref(ws, { kind: "channel", id: c.id })}"${at("channel", c.id)} data-channel="${esc(c.id)}">${avatar(directName(home, c))}<span class="team-presence${on ? " on" : ""}" aria-label="${on ? "here" : "away"}"></span>${esc(directName(home, c))}${railCount(c)}</a>`;
  }).join("");
  const people = home.members.filter(m => m.id !== home.me && !home.channels.some(c => c.direct && !c.direct.agent && c.direct.people.length === 2 && c.direct.people.includes(m.id)));
  return `<div class="team-rail-head"><a href="#/teams" class="small faint">Teams</a><strong>${esc(home.name)}</strong></div>
    <button type="button" class="team-search" data-command aria-keyshortcuts="Control+K Meta+K">${icon("search")}<span>Search or run a command</span><kbd>Ctrl K</kbd></button>
    <a href="${teamHref(ws, { kind: "activity" })}"${here.kind === "activity" ? ' aria-current="page"' : ""} class="team-activity">Activity</a>
    <h3>Projects</h3><nav aria-label="Projects">${projects || `<p class="small faint">Type a name in the command menu to start one.</p>`}</nav>
    <h3>Channels</h3><nav aria-label="Channels">${channels}</nav>
    <h3>Agents</h3><nav aria-label="Agents">${agents}</nav>
    <h3>Direct messages</h3><nav aria-label="Direct messages">${dms}${people.map(m => `<a href="#" data-dm="${esc(m.id)}">${avatar(m.name)}<span class="team-presence${m.online ? " on" : ""}" aria-label="${m.online ? "here" : "away"}"></span>${esc(m.name)}</a>`).join("")}</nav>
    <a href="${teamHref(ws, { kind: "people" })}" class="small"${here.kind === "people" ? ' aria-current="page"' : ""}>People and invites</a>`;
}

/** The channel a workspace opens on: the first with mentions, else general. */
function firstChannel(home) {
  const open = home.channels.filter(c => !c.direct);
  return (open.find(c => c.mentions) || open.find(c => c.name === "general") || open[0] || home.channels[0])?.id;
}

/**
 * What to tell the person about, from one look to the next, as each
 * channel's notification setting says: all, mentions or off.
 * @param {?TeamHome} before @param {TeamHome} now
 */
function teamNotices(before, now) {
  if (!before) return [];
  const out = [];
  for (const c of now.channels) {
    const was = before.channels.find(x => x.id === c.id) || { unread: 0, mentions: 0 };
    const title = channelTitle(now, c);
    if (c.mentions > was.mentions && c.notify !== "off") out.push({ title: `You were mentioned in ${title}`, body: now.name });
    else if (c.unread > was.unread && c.notify === "all") out.push({ title: `New in ${title}`, body: now.name });
  }
  return out;
}

async function loadHome(ws) {
  const home = await api("GET", `/v1/workspaces/${enc(ws)}/home`);
  for (const n of teamNotices(Team.homes[ws], home)) Magnus.notify?.(n.title, n.body);
  Team.homes[ws] = home;
  if (Team.ws === ws) Team.home = home;
  return home;
}

async function showTeam(main, ws, place) {
  const hash = location.hash, loop = ++Team.loop;
  Team.ws = ws;
  Team.home = Team.homes[ws] || await loadHome(ws);
  if (location.hash !== hash) return;
  if (place.kind === "channel" && !place.id) place.id = firstChannel(Team.home);
  if (place.kind === "project" && !place.id) place = { kind: "channel", id: firstChannel(Team.home) };
  Team.place = place;
  main.classList.add("team-main");
  main.innerHTML = `<div class="team-shell"><aside class="team-rail" id="team-rail"></aside><section class="team-center" id="team-center"></section><aside class="team-side" id="team-side" aria-label="Project"></aside></div>`;
  const draw = () => {
    paint($("#team-rail", main), railHtml(Team.home, Team.place));
  };
  draw();
  bindRail(main, ws);
  await drawCenter(main, ws, place);
  // Keep it live: the rail, the conversation and the panel, while it is open.
  const tick = async () => {
    if (loop !== Team.loop || !main.isConnected || location.hash !== hash) return;
    try { await loadHome(ws); draw(); await refreshCenter(main, ws); } catch (err) { /* a moment offline: the next tick tries again */ }
    setTimeout(tick, TEAM_REFRESH_MS);
  };
  setTimeout(tick, TEAM_REFRESH_MS);
  loadHome(ws).then(draw).catch(() => {});
}

/** The centre and the right panel for a place. */
async function drawCenter(main, ws, place) {
  const center = $("#team-center", main), side = $("#team-side", main);
  if (place.kind === "channel") return TeamChat.open(center, side, ws, place);
  if (place.kind === "project") return TeamPanel.project(center, side, ws, place);
  if (place.kind === "activity") return teamActivity(center, side, ws);
  if (place.kind === "search") return TeamCommand.results(center, ws, place.q);
  if (place.kind === "people") return teamPeople(center, side, ws);
}

async function refreshCenter(main, ws) {
  const place = Team.place;
  if (place.kind === "channel") return TeamChat.refresh();
  if (place.kind === "project") return TeamPanel.refresh();
}

function bindRail(main, ws) {
  $("#team-rail", main).onclick = async e => {
    if (e.target.closest("[data-command]")) { TeamCommand.open(); return; }
    const dm = e.target.closest("[data-dm]"), agent = e.target.closest("[data-direct-agent]");
    if (!dm && !(agent && agent.getAttribute("href") === "#")) return;
    e.preventDefault();
    try {
      const body = dm ? { people: [dm.dataset.dm] } : { agent: true };
      const r = await api("POST", `/v1/workspaces/${enc(ws)}/direct`, body);
      await loadHome(ws);
      location.hash = teamHref(ws, { kind: "channel", id: r.id });
    } catch (err) { toast(err.message); }
  };
}

/** Mentions, replies and asks, newest first. */
async function teamActivity(center, side, ws) {
  const a = await api("GET", `/v1/workspaces/${enc(ws)}/activity`), home = Team.home;
  const name = id => home.members.find(m => m.id === id)?.name || id;
  const row = i => `<li><a href="${teamHref(ws, { kind: "channel", id: i.channel, thread: i.thread || undefined })}"><strong>${esc(name(i.sender))}</strong> in #${esc(i.channel_name || "direct")} <span class="small faint">${esc(when(i.at))}</span><div class="small">${Compose.maskedText(firstLine(i.text))}</div></a></li>`;
  const section = (title, list, empty) => `<h2>${title}</h2>${list.length ? `<ul class="team-activity-list">${list.map(row).join("")}</ul>` : `<p class="small faint">${empty}</p>`}`;
  paint(center, `<header class="team-head"><h1>Activity</h1></header><div class="team-scroll">
    ${a.asks?.length ? `<h2>Waiting on you</h2><ul class="team-activity-list">${a.asks.map(x => `<li><a href="#/inbox">The team agent needs your answer${x.reason ? `: ${esc(x.reason)}` : ""}</a></li>`).join("")}</ul>` : ""}
    ${section("Mentions", a.mentions || [], "No one has mentioned you.")}${section("Replies", a.replies || [], "No replies to your threads.")}</div>`);
  paint(side, "");
}

/** Members, their roles and invites, for whoever may change them. */
async function teamPeople(center, side, ws) {
  const home = Team.home, base = `/v1/workspaces/${enc(ws)}`, admin = ["admin", "owner"].includes(home.role);
  const roles = selected => TEAM_ROLES.map(r => `<option ${r === selected ? "selected" : ""}>${r}</option>`).join("");
  paint(center, `<header class="team-head"><h1>People</h1></header><div class="team-scroll">
    <ul class="team-people">${home.members.map(m => `<li data-member="${esc(m.id)}">${avatar(m.name)}<span>${esc(m.name)}${m.id === home.me ? " (you)" : ""}</span>
      ${admin && m.id !== home.me ? `<select data-role aria-label="Role of ${esc(m.name)}">${roles(m.role)}</select><button class="link" data-remove>Remove</button>` : `<span class="small faint">${esc(m.role)}</span>`}</li>`).join("")}
      <li>${avatar(home.agent.name, true)}<span>${esc(home.agent.name)}</span><span class="small faint">team agent, in every channel</span></li></ul>
    ${admin ? inviteForm() : ""}</div>`);
  paint(side, "");
  bindInvite(center, ws);
  center.onchange = async e => {
    if (!e.target.matches("[data-role]")) return;
    try { await api("POST", `${base}/members`, { person: e.target.closest("[data-member]").dataset.member, role: e.target.value }); toast("Changed."); } catch (err) { toast(err.message); }
  };
  center.onclick = async e => {
    if (!e.target.closest("[data-remove]")) return;
    const p = e.target.closest("[data-member]").dataset.member;
    if (!(await ask(`Remove ${p}? Their running work here stops.`, "Remove"))) return;
    try { await api("DELETE", `${base}/members/${enc(p)}`); await loadHome(ws); teamPeople(center, side, ws); } catch (err) { toast(err.message); }
  };
}

/** Invite inline: by email, or a link to copy. */
function inviteForm() {
  return `<form class="team-invite" data-invite><input type="email" data-invite-email placeholder="name@example.com" aria-label="Invite by email"><button class="primary">Invite</button><button type="button" data-invite-link>Copy link</button><p class="small faint" data-invite-result aria-live="polite"></p></form>`;
}

function bindInvite(root, ws) {
  root.addEventListener("submit", async e => {
    const form = e.target.closest("[data-invite]");
    if (!form) return;
    e.preventDefault();
    const email = $("[data-invite-email]", form).value.trim();
    if (!email) return;
    try {
      const r = await api("POST", `/v1/workspaces/${enc(ws)}/invites`, { email });
      $("[data-invite-result]", form).textContent = r.added ? `${email} is in.` : `Send them this link: ${inviteLink(r.code)}`;
      $("[data-invite-email]", form).value = "";
      loadHome(ws).catch(() => {});
    } catch (err) { toast(err.message); }
  });
  root.addEventListener("click", async e => {
    const b = e.target.closest("[data-invite-link]");
    if (!b) return;
    try {
      const r = await api("POST", `/v1/workspaces/${enc(ws)}/invites`, {});
      const link = inviteLink(r.code);
      try { await navigator.clipboard.writeText(link); toast("Link copied. It works for a week."); } catch { /* no clipboard here */ }
      $("[data-invite-result]", b.closest("form")).textContent = link;
    } catch (err) { toast(err.message); }
  });
}

/** An invite link people open in the app. */
function inviteLink(code) { return `${location.origin}${location.pathname}#/join/${encodeURIComponent(code)}`; }
