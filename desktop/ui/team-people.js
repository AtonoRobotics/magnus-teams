// A workspace's people: who is in it and with what role; adding someone the
// server knows, or inviting someone new; taking a member out; leaving; and,
// for its Team Admins (its owner and its admins), deleting the whole
// workspace. A Magnus admin has no say here (the owner, 2026-11-11).
// Nobody changes a role above their own, and a workspace keeps an owner:
// the server holds both rules, and the page offers only what it allows.
"use strict";

/** @param {string} r */
const teamRank = r => TEAM_ROLES.indexOf(r);

/** Whether the viewer is one of the workspace's Team Admins (its owner or an admin). @param {TeamHome} home */
const isTeamAdmin = home => teamRank(home.role) >= teamRank("admin");

/** What the way to the people page is called: it says what a Team Admin does there. @param {TeamHome} home */
const peopleLabel = home => isTeamAdmin(home) ? "People: add or remove" : "People";

/**
 * The page as `home` shows it to its viewer.
 * @param {TeamHome} home @param {{ id: string, name: string }[]} candidates
 */
function teamPeopleHtml(home, candidates) {
  const mine = home.role, admin = isTeamAdmin(home);
  const owners = home.members.filter(m => m.role === "owner").length;
  const roles = (selected, upto) => TEAM_ROLES.filter(r => teamRank(r) <= teamRank(upto)).map(r => `<option value="${r}"${r === selected ? " selected" : ""}>${esc(teamRoleLabel(r))}</option>`).join("");
  const row = m => {
    const me = m.id === home.me, manages = admin && !me && teamRank(m.role) <= teamRank(mine);
    const lastOwner = m.role === "owner" && owners <= 1;
    const tools = manages ? `<select data-role aria-label="Role of ${esc(m.name)}"${lastOwner ? " disabled" : ""}>${roles(m.role, mine)}</select>${lastOwner ? "" : `<button class="link" data-remove>Remove</button>`}`
      : `<span class="small faint">${esc(teamRoleLabel(m.role))}</span>`;
    const leave = me ? lastOwner ? `<span class="small faint">To leave, make someone else an owner first.</span>` : `<button class="link" data-leave>Leave</button>` : "";
    return `<li data-member="${esc(m.id)}" data-name="${esc(m.name)}">${avatar(m.name)}<span>${esc(m.name)}${me ? " (you)" : ""}</span>${tools}${leave}</li>`;
  };
  const add = !admin ? "" : candidates.length
    ? `<form class="team-invite" data-add-people><label class="sr" for="team-add-person">Person to add</label><select id="team-add-person" data-add-person>${candidates.map(p => `<option value="${esc(p.id)}">${esc(p.name)}</option>`).join("")}</select>
      <select data-add-role aria-label="Their role">${roles("member", mine)}</select><button class="primary">Add</button></form>`
    : `<p class="small faint">Everyone with an account on this server is here already.</p>`;
  return `<header class="team-head"><h1>People</h1></header><div class="team-scroll">
    ${admin ? `<section data-people-add><h2>Add people</h2><p class="small faint">Someone with an account on this server joins at once.</p>${add}<h3>Someone new to the server</h3><p class="small faint">Invite them by email, or copy a link to send them.</p>${inviteForm()}</section>` : ""}
    <h2>In ${esc(home.name)}</h2>${admin ? "" : `<p class="small faint">A Team Admin adds and removes people here.</p>`}
    <ul class="team-people">${home.members.map(row).join("")}
      <li>${avatar(home.agent.name, true)}<span>${esc(home.agent.name)}</span><span class="small faint">team agent, in every channel</span></li></ul>
    ${admin ? teamAppsHtml(home) : ""}
    ${admin ? `<h2>Delete this workspace</h2><p class="small faint">Its channels, messages, files, projects and computer are deleted for everyone, from backups too within 30 days. This cannot be undone.</p><button class="danger" data-delete-workspace>Delete workspace</button>` : ""}</div>`;
}

/** The roles each choice of who may use an app grants it to. */
const APP_ROLES = { off: [], members: ["member", "admin", "owner"], admins: ["admin", "owner"] };

/** The workspace's apps, for its Team Admins: which the team agent may use,
 * for whom, and, for an app people sign in to, the team's own sign-in. A
 * member's own sign-in never serves a team thread. @param {TeamHome} home */
function teamAppsHtml(home) {
  const apps = home.apps || [];
  if (!apps.length) return "";
  const who = a => !a.roles.length ? "off" : a.roles.includes("member") ? "members" : "admins";
  const choices = [["off", "Not in this workspace"], ["members", "Members and admins"], ["admins", "Team Admins only"]];
  const row = a => {
    const w = who(a);
    const pick = `<select data-app-roles aria-label="Who may have the agent use ${esc(a.title)}">${choices.map(([v, l]) => `<option value="${v}"${v === w ? " selected" : ""}>${l}</option>`).join("")}</select>`;
    const sign = !a.signs_in || w === "off" ? "" : a.signed_in
      ? `<span class="small faint">Connected for the workspace</span><button class="link" data-app-disconnect>Disconnect</button>`
      : `<button data-app-connect>Connect for the workspace</button>`;
    return `<li data-app="${esc(a.name)}" data-title="${esc(a.title)}"><span>${esc(a.title)}</span>${pick}${sign}</li>`;
  };
  return `<section data-team-apps><h2>Apps</h2><p class="small faint">What the team agent may use here. An app people sign in to is connected with the team's own account, never a member's: whoever may use it here acts through that account.</p>
    <ul class="team-people">${apps.map(row).join("")}</ul></section>`;
}

/** A Team Admin signs an app in for the workspace, as a person connects
 * their own from the Library: the browser, then the sign-in caught here. */
async function connectWorkspaceApp(ws, name, title) {
  if (!Magnus.loopbackOpen) { toast("Connect apps from the Magnus desktop app."); return; }
  try {
    const caught = await Magnus.loopbackOpen(null);
    const r = await Server.workspaces.appSignIn(ws, name, caught);
    Magnus.openExternal(r.authorize_url).catch(() => {});
    const got = await Magnus.loopbackWait();
    if (got.state !== r.state) throw new Error("That sign-in came back for another request. Try again.");
    await Server.workspaces.appSignInComplete(ws, name, { state: got.state, code: got.code, iss: got.iss });
    toast(`${title} is connected for the workspace.`);
  } catch (err) { toast(err.body?.message || err.message); }
}

/** Members, their roles and invites, for whoever may change them. */
async function teamPeople(center, side, ws) {
  const home = Team.home, admin = isTeamAdmin(home);
  const candidates = admin ? await Server.workspaces.people(ws).catch(() => []) : [];
  paint(center, teamPeopleHtml(home, candidates));
  paint(side, "");
  bindInvite(center, ws);
  const again = async () => { await loadHome(ws); return teamPeople(center, side, ws); };
  const who = e => e.target.closest("[data-member]");
  const app = e => e.target.closest("[data-app]");
  center.onchange = async e => {
    if (e.target.matches("[data-app-roles]")) {
      try { await Server.workspaces.grantApp(ws, app(e).dataset.app, APP_ROLES[e.target.value] || []); toast("Changed."); } catch (err) { toast(err.message); }
      return again();
    }
    if (!e.target.matches("[data-role]")) return;
    try { await Server.workspaces.setMember(ws, who(e).dataset.member, e.target.value); toast("Changed."); } catch (err) { toast(err.message); }
    await again();
  };
  center.onsubmit = async e => {
    if (!e.target.closest("[data-add-people]")) return;
    e.preventDefault();
    const person = $("[data-add-person]", center).value, role = $("[data-add-role]", center).value;
    try { await Server.workspaces.setMember(ws, person, role); toast("Added. The workspace is in their app now."); await again(); } catch (err) { toast(err.message); }
  };
  center.onclick = async e => {
    if (e.target.closest("[data-remove]")) {
      const m = who(e);
      if (!(await ask(`Take ${m.dataset.name} out of ${home.name}? They lose it at once and their running work here stops. What they wrote stays.`, "Remove"))) return;
      try { await Server.workspaces.removeMember(ws, m.dataset.member); await again(); } catch (err) { toast(err.message); }
    }
    if (e.target.closest("[data-leave]")) {
      if (!(await ask(`Leave ${home.name}? You lose its channels and files at once. What you wrote stays.`, "Leave"))) return;
      try { await Server.workspaces.removeMember(ws, home.me); leftWorkspaceQuietly(ws); toast(`You left ${home.name}.`); } catch (err) { toast(err.message); }
    }
    if (e.target.closest("[data-delete-workspace]")) await deleteWorkspace(home);
    if (e.target.closest("[data-app-connect]")) {
      const a = app(e);
      await connectWorkspaceApp(ws, a.dataset.app, a.dataset.title);
      await again();
    }
    if (e.target.closest("[data-app-disconnect]")) {
      const a = app(e);
      if (!(await ask(`Disconnect ${a.dataset.title} from ${home.name}? The team agent loses it at once. Members' own connections are not touched.`, "Disconnect"))) return;
      try { await Server.workspaces.appDisconnect(ws, a.dataset.app); toast("Disconnected."); } catch (err) { toast(err.body?.message || err.message); }
      await again();
    }
  };
}

/** Forget a workspace the person is no longer in, and go to the list. */
function leftWorkspaceQuietly(ws) {
  delete Team.homes[ws];
  if (Team.ws === ws) { Team.ws = null; Team.home = null; }
  location.hash = "#/teams";
}

/** Delete it, once its name is typed: any of its Team Admins may. */
async function deleteWorkspace(home) {
  if (!(await askTyped(`Delete ${home.name} for everyone? Its channels, messages, files, projects and computer go, and this cannot be undone.`, home.name, "Delete workspace"))) return;
  try {
    await Server.workspaces.remove(home.id);
    leftWorkspaceQuietly(home.id);
    toast(`${home.name} was deleted.`);
  } catch (err) { if (!err.cancelled) toast(err.body?.message || err.message); }
}

/** A confirmation for what cannot be undone: the button works once
 * `expected` is typed. Resolves whether the person went ahead. */
function askTyped(text, expected, label) {
  return new Promise(done => {
    const d = document.createElement("dialog");
    d.className = "ask"; d.setAttribute("aria-label", label);
    d.innerHTML = `<p>${esc(text)}</p><label class="stack small">Type “${esc(expected)}” to confirm<input data-typed autocomplete="off" spellcheck="false"></label>
      <div class="row"><button data-ask="no">Cancel</button><button class="danger" data-ask="yes" disabled>${esc(label)}</button></div>`;
    const input = /** @type {HTMLInputElement} */ ($("[data-typed]", d)), go = /** @type {HTMLButtonElement} */ ($("[data-ask=yes]", d));
    const ready = () => input.value.trim() === expected.trim();
    input.addEventListener("input", () => { go.disabled = !ready(); });
    input.addEventListener("keydown", e => { if (e.key === "Enter" && ready()) d.close("yes"); });
    d.addEventListener("click", e => { const b = e.target.closest("[data-ask]"); if (b && !b.disabled) d.close(b.dataset.ask); });
    d.addEventListener("close", () => { d.remove(); done(d.returnValue === "yes"); });
    document.body.append(d);
    d.showModal();
    input.focus();
  });
}
