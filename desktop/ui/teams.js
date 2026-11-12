// Workspaces: a team chat and project app with agents as members. Three
// panes: the rail (projects, channels, direct messages, agents), the
// conversation, and the open project's panel. Nothing is set up through a
// step-by-step form: things are made by typing their name, and the team
// agent is in every channel from the start. The conversation is in
// team-chat.js, the right panel in team-panel.js and the command menu in
// team-command.js.
UI.team = '<circle cx="9" cy="8" r="3"/><path d="M3 20a6 6 0 0 1 12 0"/><path d="M16 5.5a3 3 0 0 1 0 5M18 14.5a6 6 0 0 1 3 5.5"/>';
const TEAM_REFRESH_MS = 3000, TEAM_ROLES = ["guest", "member", "admin", "owner"];

/** A workspace role as people read it: its owner and admins are its Team Admins.
 * @param {string} r */
const teamRoleLabel = r => ({ owner: "Team Admin (owner)", admin: "Team Admin", member: "member", guest: "guest" })[r] || r;

/**
 * @typedef {{ id: string, name: string, role: string, online: boolean }} TeamMember
 * @typedef {{ id: string, name: string, project: ?string, mode: "mentions"|"when_helpful", notify: "all"|"mentions"|"off", unread: number, mentions: number, direct: ?{ people: string[], agent: boolean }, archived?: boolean, deleted?: boolean }} TeamChannel
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

/** Whether the signed-in person may start a workspace: the server lets only
 * a Team Admin, so nobody else is offered a form it would refuse. */
async function mayStartWorkspace() {
  const known = typeof AdminState !== "undefined" ? AdminState.standing : null;
  const standing = known || await api("GET", "/v1/admin/standing").catch(() => null);
  return !!standing?.team_admin;
}

/** The Teams page: the person's workspaces, and the name field that starts
 * one for those allowed to.
 * @param {{id: string, name: string, role: string}[]} list @param {boolean} mayStart */
function teamsPageHtml(list, mayStart) {
  const none = mayStart ? "You are in no workspace yet. Type a name to start one; the team agent is in it from the start." : "You are in no workspace yet. A Team Admin starts one and invites you.";
  return `<div class="teams-page"><h1>Teams</h1><p class="small faint">Workspaces you share with other people and their agents. Your own conversations stay yours.</p>${list.length ? `<ul class="team-list">${list.map(w => `<li data-workspace="${esc(w.id)}"><a href="#/team/${enc(w.id)}">${avatar(w.name)}<span><strong>${esc(w.name)}</strong><br><span class="small faint">${esc(teamRoleLabel(w.role))}</span></span></a></li>`).join("")}</ul>` : `<p class="empty">${none}</p>`}
    ${mayStart ? `<form id="new-team"><input id="team-name" placeholder="Type a name to start a workspace" aria-label="Workspace name" autocomplete="off"><button class="primary">Create</button></form>` : ""}</div>`;
}

async function showTeams(main) {
  const [list, mayStart] = await Promise.all([api("GET", "/v1/workspaces"), mayStartWorkspace()]);
  paint(main, teamsPageHtml(list, mayStart));
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
  // What the person put away or deleted for themselves stays off the list;
  // the one open now stays on it until they leave it.
  const listed = c => here.kind === "channel" && here.id === c.id || !(c.archived || c.deleted);
  const away = home.channels.filter(c => c.archived && !c.deleted && !listed(c));
  home = { ...home, channels: home.channels.filter(listed) };
  const archived = away.length ? `<details class="team-archived"><summary class="small">Archived (${away.length})</summary><nav aria-label="Archived">${away.map(c => `<a href="${teamHref(ws, { kind: "channel", id: c.id })}" data-channel="${esc(c.id)}">${esc(channelTitle(home, c))}${railCount(c)}</a>`).join("")}</nav></details>` : "";
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
    ${archived}<a href="${teamHref(ws, { kind: "people" })}" class="small"${here.kind === "people" ? ' aria-current="page"' : ""}>People and invites</a>`;
}

/** The channel a workspace opens on: the first with mentions, else general. */
function firstChannel(home) {
  const open = home.channels.filter(c => !c.direct && !c.archived);
  return (open.find(c => c.mentions) || open.find(c => c.name === "general") || open[0] || home.channels.find(c => !c.archived && !c.deleted) || home.channels[0])?.id;
}

async function loadHome(ws) {
  const home = await api("GET", `/v1/workspaces/${enc(ws)}/home`);
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
    try { await loadHome(ws); draw(); await refreshCenter(main, ws); } catch (err) {
      // Deleted, or the person was taken out of it: it is gone at once.
      if (err?.status === 404 && err?.body?.error === "no such workspace") return leftWorkspace(ws);
      /* otherwise a moment offline: the next tick tries again */
    }
    setTimeout(tick, TEAM_REFRESH_MS);
  };
  setTimeout(tick, TEAM_REFRESH_MS);
  loadHome(ws).then(draw).catch(() => {});
}

/** The workspace is no longer the person's: back to the list, saying so. */
function leftWorkspace(ws) {
  toast("That workspace is no longer yours to open: it was deleted, or you are no longer in it.", 6000);
  leftWorkspaceQuietly(ws);
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
      $("[data-invite-result]", form).textContent = inviteOutcome(email, r);
      $("[data-invite-email]", form).value = "";
      loadHome(ws).catch(() => {});
    } catch (err) { toast(err.message); }
  });
  root.addEventListener("click", async e => {
    const b = e.target.closest("[data-invite-link]");
    if (!b) return;
    try {
      const r = await api("POST", `/v1/workspaces/${enc(ws)}/invites`, {});
      const link = serverLink(r);
      try { await navigator.clipboard.writeText(link); toast("Link copied. It works for a week."); } catch { /* no clipboard here */ }
      $("[data-invite-result]", b.closest("form")).textContent = `Anyone with this link can join for a week, with or without an account here: ${link}`;
    } catch (err) { toast(err.message); }
  });
}

/**
 * @typedef {{ added?: string, already?: boolean, invited?: string, emailed?: boolean, mail?: boolean, email_error?: ?string, link?: ?string, path?: string, code?: string }} TeamInvited
 */

/** A link the server gave: whole when it knows its public address, else below the address this app talks to. @param {TeamInvited} r */
function serverLink(r) {
  if (r.link) return r.link;
  const base = (globalThis.App?.appliance || location.origin).replace(/\/$/, "");
  return `${base}${r.path || ""}`;
}

/** What inviting `email` did, said plainly: in at once, emailed, or the link to send when no email went. @param {string} email @param {TeamInvited} r */
function inviteOutcome(email, r) {
  if (r.added) return r.already ? `${email} is already in this workspace.` : `${email} is in. The workspace is in their app now.`;
  if (r.emailed) return `Invitation emailed to ${email}. They join when they choose their password.`;
  const why = r.email_error ? `The email to ${email} could not be sent (${r.email_error}).` : r.mail === false ? "This server does not send email yet, so nothing was emailed." : `No email went to ${email}.`;
  return `${why} Send them this link yourself: ${serverLink(r)}`;
}
