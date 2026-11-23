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
 * @typedef {{ name: string, title: string, roles: string[], signs_in: boolean, signed_in: boolean }} TeamApp
 * @typedef {{ id: string, name: string, role: string, me: string, me_name: string, agent: { id: string, name: string }, members: TeamMember[], channels: TeamChannel[], projects: TeamProject[], computer: ?string, spend?: TeamSpend, apps?: TeamApp[] }} TeamHome
 * @typedef {{ kind: "channel"|"project"|"activity"|"search"|"people", id?: string, thread?: string, tab?: string, q?: string }} TeamPlace
 */

/** What the open workspace looks like now, kept between views so a switch paints at once. */
const Team = { ws: null, /** @type {?TeamHome} */ home: null, /** @type {TeamPlace} */ place: { kind: "channel" }, homes: {}, avatars: {}, capabilities: {}, loop: 0 };

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
  const standing = known || await Server.admin.standing().catch(() => null);
  return !!standing?.team_admin;
}

/** The Teams page: the person's workspaces, and the name field that starts
 * one for those allowed to.
 * @param {{id: string, name: string, role: string}[]} list @param {boolean} mayStart */
function teamsPageHtml(list, mayStart) {
  const none = mayStart ? "You are in no shared project yet. Type a name to start one; Magnus is in it from the start." : "You are in no shared project yet. A Team Admin starts one and invites you.";
  return `<div class="teams-page"><h1>Shared projects</h1><p class="small faint">Projects and conversations you share with other people and Magnus. Your private conversations stay yours.</p>${list.length ? `<ul class="team-list">${list.map(w => `<li data-workspace="${esc(w.id)}"><a href="#/team/${enc(w.id)}">${avatar(w.name)}<span><strong>${esc(w.name)}</strong><br><span class="small faint">${esc(teamRoleLabel(w.role))}</span></span></a></li>`).join("")}</ul>` : `<p class="empty">${none}</p>`}
    ${mayStart ? `<form id="new-team"><input id="team-name" placeholder="Type a name to start a shared project" aria-label="Shared project name" autocomplete="off"><button class="primary">Create</button></form>` : ""}</div>`;
}

async function showTeams(main) {
  const [list, mayStart] = await Promise.all([Server.workspaces.list(), mayStartWorkspace()]);
  paint(main, teamsPageHtml(list, mayStart));
  main.onsubmit = async e => {
    if (e.target.id !== "new-team") return;
    e.preventDefault();
    const name = $("#team-name", main).value.trim();
    if (!name) return;
    try { const w = await Server.workspaces.create(name); location.hash = `#/team/${enc(w.id)}`; } catch (err) { toast(err.message); }
  };
}

async function joinTeam(main, code) {
  paint(main, `<p class="empty">Joining…</p>`);
  try { const r = await Server.workspaces.join(code); location.hash = `#/team/${enc(r.workspace)}`; }
  catch (err) { paint(main, `<div class="banner bad">${esc(err.message)}</div><p><a href="#/teams">Teams</a></p>`); }
}

/** An avatar: UserAvatar (photo/preset/initials) for people, or the agent's mark. */
function avatar(name, agent = false, personId = null) {
  if (!agent && typeof UserAvatar !== "undefined") {
    const id = personId || Team.home?.members?.find(m => m.name === name)?.id;
    const av = id ? Team.avatars[id] : null;
    if (av && av.kind !== "default") return UserAvatar.html(name, av, "sm");
  }
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
 * The left rail: Obsidian Aurora mockup layout — Orb header, Rooms (+ New),
 * and People (with online dot + advertised personal apps).
 * @param {TeamHome} home @param {TeamPlace} here
 */
function railHtml(home, here) {
  const at = (kind, id) => here.kind === kind && here.id === id ? ' aria-current="page"' : "";
  const ws = home.id;
  const listed = c => here.kind === "channel" && here.id === c.id || !(c.archived || c.deleted);
  const away = home.channels.filter(c => c.archived && !c.deleted && !listed(c));
  home = { ...home, channels: home.channels.filter(listed) };
  const archived = away.length ? `<details class="team-archived"><summary class="small">Archived (${away.length})</summary><nav aria-label="Archived">${away.map(c => `<a href="${teamHref(ws, { kind: "channel", id: c.id })}" data-channel="${esc(c.id)}">${esc(channelTitle(home, c))}${railCount(c)}</a>`).join("")}</nav></details>` : "";
  const open = home.channels.filter(c => !c.direct);
  const rooms = open.map(c => `<a href="${teamHref(ws, { kind: "channel", id: c.id })}"${at("channel", c.id)} data-channel="${esc(c.id)}" class="${c.unread && c.notify !== "off" ? "unread" : ""}${c.notify === "off" ? " muted" : ""}">#${esc(c.name)}${railCount(c)}</a>`).join("");
  const capsById = new Map((Team.capabilities?.[ws] || []).map(x => [x.id, (x.apps || []).map(a => a.title || a.name).join(" · ")]));
  const dms = home.channels.filter(c => c.direct && !c.direct.agent).map(c => {
    const other = c.direct.people.find(p => p !== home.me), on = home.members.find(m => m.id === other)?.online;
    const apps = other ? capsById.get(other) : "";
    return `<a class="room-person-link" href="${teamHref(ws, { kind: "channel", id: c.id })}"${at("channel", c.id)} data-channel="${esc(c.id)}"><span class="room-person-top">${avatar(directName(home, c), false, other)}<span class="team-presence${on ? " on" : ""}" aria-label="${on ? "here" : "away"}"></span><span>${esc(directName(home, c))}</span>${railCount(c)}${on ? `<span class="room-person-dot" aria-hidden="true"></span>` : ""}</span>${apps ? `<span class="room-person-apps">${esc(apps)}</span>` : ""}</a>`;
  }).join("");
  const people = home.members.filter(m => m.id !== home.me && !home.channels.some(c => c.direct && !c.direct.agent && c.direct.people.length === 2 && c.direct.people.includes(m.id)));
  const peopleRows = people.map(m => {
    const apps = capsById.get(m.id) || "";
    return `<a class="room-person-link" href="#" data-dm="${esc(m.id)}"><span class="room-person-top">${avatar(m.name, false, m.id)}<span class="team-presence${m.online ? " on" : ""}" aria-label="${m.online ? "here" : "away"}"></span><span>${esc(m.name)}</span>${m.online ? `<span class="room-person-dot" aria-hidden="true"></span>` : ""}</span>${apps ? `<span class="room-person-apps">${esc(apps)}</span>` : ""}</a>`;
  }).join("");
  return `<div class="team-rail-head"><span class="room-orb" aria-hidden="true"></span><div class="team-rail-head-meta"><strong>${esc(home.name)}</strong><span class="small faint">${home.members.length} ${home.members.length === 1 ? "person" : "people"}</span></div></div>
    <div class="room-rail-sec"><h3>Rooms</h3><button type="button" class="room-rail-add" data-command>+ New</button></div><nav aria-label="Rooms">${rooms}</nav>
    <div class="room-rail-sec"><h3>People</h3></div><nav aria-label="People">${dms}${peopleRows}</nav>
    ${archived}
    <div class="team-rail-foot">
      <a href="${teamHref(ws, { kind: "people" })}"${here.kind === "people" ? ' aria-current="page"' : ""} data-rail-people>${peopleLabel(home)} <span class="small faint">${home.members.length}</span></a>
      <a href="#/teams" class="small faint">All teams</a>
    </div>`;
}

/** The channel a workspace opens on: the first with mentions, else general. */
function firstChannel(home) {
  const open = home.channels.filter(c => !c.direct && !c.archived);
  return (open.find(c => c.mentions) || open.find(c => c.name === "general") || open[0] || home.channels.find(c => !c.archived && !c.deleted) || home.channels[0])?.id;
}

async function loadHome(ws) {
  const [home, caps] = await Promise.all([
    Server.workspaces.home(ws),
    Server.workspaces.capabilities ? Server.workspaces.capabilities(ws).catch(() => null) : Promise.resolve(null),
  ]);
  Team.homes[ws] = home;
  if (caps?.length) {
    Team.capabilities[ws] = caps;
    for (const m of caps) if (m.avatar) Team.avatars[m.id] = m.avatar;
  }
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
  main.innerHTML = `<div class="team-shell"><aside class="team-rail" id="team-rail"></aside><section class="team-center" id="team-center"></section><div class="team-splitter" id="team-splitter" role="separator" tabindex="0" aria-label="Side panel width" aria-orientation="vertical"></div><aside class="team-side" id="team-side" aria-label="Work"></aside></div>`;
  const shellEl = $(".team-shell", main);
  if (shellEl) {
    shellEl.addEventListener("click", e => {
      const t = /** @type {HTMLElement} */ (e.target);
      if (t.closest("[data-room-work-toggle]")) {
        shellEl.classList.toggle("work-closed");
        return;
      }
      if (t.closest("[data-room-mute]")) {
        const b = t.closest("[data-room-mute]");
        b.textContent = b.textContent === "Mute" ? "Unmute" : "Mute";
        return;
      }
      if (t.closest("[data-room-call]") || t.closest("[data-room-call-leave]")) {
        const center = $("#team-center", main);
        if (!center) return;
        const existing = $(".room-video-stage", center);
        if (existing) {
          existing.remove();
          const btn = $("[data-room-call] span", center);
          if (btn) btn.textContent = "Join Call";
          return;
        }
        const members = (Team.home?.members || []).slice(0, 4);
        const stage = document.createElement("section");
        stage.className = "room-video-stage";
        stage.innerHTML = `<div class="room-video-grid">${members.map(m => `<div class="room-video-tile">${avatar(m.name, false, m.id)}<span class="room-video-tile-name">${esc(m.name)}</span></div>`).join("")}</div>
          <div class="room-video-bar">
            <span class="room-video-listening"><span class="room-orb room-orb-sm" aria-hidden="true"></span><span>Magnus listening · taking notes into Work</span></span>
            <div class="room-video-controls">
              <button type="button" data-room-mute>Mute</button>
              <button type="button" data-room-cam>Camera</button>
              <button type="button" data-room-screen>Share screen</button>
              <button type="button" class="danger" data-room-call-leave>Leave</button>
            </div>
          </div>`;
        center.prepend(stage);
        const btn = $("[data-room-call] span", center);
        if (btn) btn.textContent = "In Call";
      }
    });
  }
  const splitter = $("#team-splitter", main);
  if (splitter && typeof Theme !== "undefined") {
    const B = Theme.LAYOUT_BOUNDS || { panelMinWidthPx: 280, panelMaxWidthPx: 520, panelDefaultWidthPx: 360 };
    const setWidth = w => {
      const clamped = Math.max(B.panelMinWidthPx, Math.min(B.panelMaxWidthPx, Math.round(w)));
      Theme.set({ panelWidthPx: clamped });
      $(".shell")?.style.setProperty("--panel-width", `${clamped}px`);
    };
    splitter.onpointerdown = e => {
      if (e.button !== 0) return;
      const sideEl = $("#team-side", main), startX = e.clientX, startW = sideEl?.getBoundingClientRect().width || Theme.prefs().panelWidthPx || B.panelDefaultWidthPx;
      splitter.setPointerCapture(e.pointerId);
      splitter.onpointermove = mv => setWidth(startW + startX - mv.clientX);
      splitter.onpointerup = () => { splitter.onpointermove = null; splitter.onpointerup = null; splitter.releasePointerCapture(e.pointerId); };
    };
    splitter.ondblclick = () => setWidth(B.panelDefaultWidthPx);
  }
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
  const place = Team.place, side = $("#team-side", main);
  if (place.kind === "channel") return TeamChat.refresh();
  if (place.kind === "project") return TeamPanel.refresh();
  if (side) return TeamPanel.side(side, ws, null, null);
}

function bindRail(main, ws) {
  $("#team-rail", main).onclick = async e => {
    if (e.target.closest("[data-command]")) { TeamCommand.open(); return; }
    const dm = e.target.closest("[data-dm]"), agent = e.target.closest("[data-direct-agent]");
    if (!dm && !(agent && agent.getAttribute("href") === "#")) return;
    e.preventDefault();
    try {
      const body = dm ? { people: [dm.dataset.dm] } : { agent: true };
      const r = await Server.workspaces.direct(ws, body);
      await loadHome(ws);
      location.hash = teamHref(ws, { kind: "channel", id: r.id });
    } catch (err) { toast(err.message); }
  };
}

/** Mentions, replies and asks, newest first. */
async function teamActivity(center, side, ws) {
  const a = await Server.workspaces.activity(ws), home = Team.home;
  const name = id => home.members.find(m => m.id === id)?.name || id;
  const row = i => `<li><a href="${teamHref(ws, { kind: "channel", id: i.channel, thread: i.thread || undefined })}"><strong>${esc(name(i.sender))}</strong> in #${esc(i.channel_name || "direct")} <span class="small faint">${esc(when(i.at))}</span><div class="small">${Compose.maskedText(firstLine(i.text))}</div></a></li>`;
  const section = (title, list, empty) => `<h2>${title}</h2>${list.length ? `<ul class="team-activity-list">${list.map(row).join("")}</ul>` : `<p class="small faint">${empty}</p>`}`;
  paint(center, `<header class="team-head"><h1>Activity</h1></header><div class="team-scroll">
    ${a.asks?.length ? `<h2>Waiting on you</h2><ul class="team-activity-list">${a.asks.map(x => `<li><a href="#/inbox">The team agent needs your answer${x.reason ? `: ${esc(x.reason)}` : ""}</a></li>`).join("")}</ul>` : ""}
    ${section("Mentions", a.mentions || [], "No one has mentioned you.")}${section("Replies", a.replies || [], "No replies to your threads.")}</div>`);
  await TeamPanel.side(side, ws, null, null);
}

/** Invite inline: by email, or a link to copy. */
function inviteForm() {
  return `<form class="team-invite" data-invite><input type="email" data-invite-email placeholder="name@example.com" aria-label="Invite by email"><button class="primary">Invite</button><button type="button" data-invite-link>Copy link</button><p class="small faint" data-invite-result aria-live="polite"></p></form>`;
}

/** Listen for invites under `root`, once: the people page redraws into the
 * same element after every change, and a second listener sent each invite
 * (and its email) twice. */
function bindInvite(root, ws) {
  // The workspace is read when an invite is sent: the same element shows
  // another workspace's page after a switch, and its invites go there.
  root.dataset.inviteWs = ws;
  if (root.dataset.inviteBound) return;
  root.dataset.inviteBound = "1";
  root.addEventListener("submit", async e => {
    const form = e.target.closest("[data-invite]");
    if (!form) return;
    e.preventDefault();
    const ws = root.dataset.inviteWs || "";
    const email = $("[data-invite-email]", form).value.trim();
    if (!email) return;
    try {
      const r = await Server.workspaces.invite(ws, email);
      $("[data-invite-result]", form).textContent = inviteOutcome(email, r);
      $("[data-invite-email]", form).value = "";
      loadHome(ws).catch(() => {});
    } catch (err) { toast(err.message); }
  });
  root.addEventListener("click", async e => {
    const b = e.target.closest("[data-invite-link]");
    if (!b) return;
    const ws = root.dataset.inviteWs || "";
    try {
      const r = await Server.workspaces.invite(ws);
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
