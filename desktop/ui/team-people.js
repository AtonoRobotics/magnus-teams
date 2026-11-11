// A workspace's people: who is in it and with what role; adding someone the
// server knows, or inviting someone new; taking a member out; leaving; and,
// for its owner or an admin of the server, deleting the whole workspace.
// Nobody changes a role above their own, and a workspace keeps an owner:
// the server holds both rules, and the page offers only what it allows.
"use strict";

/** @param {string} r */
const teamRank = r => TEAM_ROLES.indexOf(r);

/**
 * The page as `home` shows it to its viewer.
 * @param {TeamHome} home @param {{ id: string, name: string }[]} candidates @param {boolean} serverAdmin
 */
function teamPeopleHtml(home, candidates, serverAdmin) {
  const mine = home.role, admin = teamRank(mine) >= teamRank("admin"), owner = mine === "owner";
  const owners = home.members.filter(m => m.role === "owner").length;
  const roles = (selected, upto) => TEAM_ROLES.filter(r => teamRank(r) <= teamRank(upto)).map(r => `<option ${r === selected ? "selected" : ""}>${r}</option>`).join("");
  const row = m => {
    const me = m.id === home.me, manages = admin && !me && teamRank(m.role) <= teamRank(mine);
    const lastOwner = m.role === "owner" && owners <= 1;
    const tools = manages ? `<select data-role aria-label="Role of ${esc(m.name)}"${lastOwner ? " disabled" : ""}>${roles(m.role, mine)}</select>${lastOwner ? "" : `<button class="link" data-remove>Remove</button>`}`
      : `<span class="small faint">${esc(m.role)}</span>`;
    const leave = me ? lastOwner ? `<span class="small faint">To leave, make someone else an owner first.</span>` : `<button class="link" data-leave>Leave</button>` : "";
    return `<li data-member="${esc(m.id)}" data-name="${esc(m.name)}">${avatar(m.name)}<span>${esc(m.name)}${me ? " (you)" : ""}</span>${tools}${leave}</li>`;
  };
  const add = !admin ? "" : candidates.length
    ? `<form class="team-invite" data-add-people><label class="sr" for="team-add-person">Person to add</label><select id="team-add-person" data-add-person>${candidates.map(p => `<option value="${esc(p.id)}">${esc(p.name)}</option>`).join("")}</select>
      <select data-add-role aria-label="Their role">${roles("member", mine)}</select><button class="primary">Add</button></form>`
    : `<p class="small faint">Everyone with an account on this server is here already.</p>`;
  const deletes = owner || serverAdmin;
  return `<header class="team-head"><h1>People</h1></header><div class="team-scroll">
    <ul class="team-people">${home.members.map(row).join("")}
      <li>${avatar(home.agent.name, true)}<span>${esc(home.agent.name)}</span><span class="small faint">team agent, in every channel</span></li></ul>
    ${admin ? `<h2>Add people</h2><p class="small faint">Someone with an account on this server joins at once.</p>${add}<h3>Someone new to the server</h3>${inviteForm()}` : ""}
    ${deletes ? `<h2>Delete this workspace</h2><p class="small faint">Its channels, messages, files, projects and computer are deleted for everyone, from backups too within 30 days. This cannot be undone.</p><button class="danger" data-delete-workspace>Delete workspace</button>` : ""}</div>`;
}

/** Members, their roles and invites, for whoever may change them. */
async function teamPeople(center, side, ws) {
  const home = Team.home, base = `/v1/workspaces/${enc(ws)}`, admin = teamRank(home.role) >= teamRank("admin");
  const serverAdmin = !!(typeof AdminState !== "undefined" && AdminState.standing?.admin);
  const candidates = admin ? await api("GET", `${base}/people`).catch(() => []) : [];
  paint(center, teamPeopleHtml(home, candidates, serverAdmin));
  paint(side, "");
  bindInvite(center, ws);
  const again = async () => { await loadHome(ws); return teamPeople(center, side, ws); };
  const who = e => e.target.closest("[data-member]");
  center.onchange = async e => {
    if (!e.target.matches("[data-role]")) return;
    try { await api("POST", `${base}/members`, { person: who(e).dataset.member, role: e.target.value }); toast("Changed."); } catch (err) { toast(err.message); }
    await again();
  };
  center.onsubmit = async e => {
    if (!e.target.closest("[data-add-people]")) return;
    e.preventDefault();
    const person = $("[data-add-person]", center).value, role = $("[data-add-role]", center).value;
    try { await api("POST", `${base}/members`, { person, role }); toast("Added. The workspace is in their app now."); await again(); } catch (err) { toast(err.message); }
  };
  center.onclick = async e => {
    if (e.target.closest("[data-remove]")) {
      const m = who(e);
      if (!(await ask(`Take ${m.dataset.name} out of ${home.name}? They lose it at once and their running work here stops. What they wrote stays.`, "Remove"))) return;
      try { await api("DELETE", `${base}/members/${enc(m.dataset.member)}`); await again(); } catch (err) { toast(err.message); }
    }
    if (e.target.closest("[data-leave]")) {
      if (!(await ask(`Leave ${home.name}? You lose its channels and files at once. What you wrote stays.`, "Leave"))) return;
      try { await api("DELETE", `${base}/members/${enc(home.me)}`); leftWorkspaceQuietly(ws); toast(`You left ${home.name}.`); } catch (err) { toast(err.message); }
    }
    if (e.target.closest("[data-delete-workspace]")) await deleteWorkspace(home, serverAdmin);
  };
}

/** Forget a workspace the person is no longer in, and go to the list. */
function leftWorkspaceQuietly(ws) {
  delete Team.homes[ws];
  if (Team.ws === ws) { Team.ws = null; Team.home = null; }
  location.hash = "#/teams";
}

/** Delete it, once its name is typed. An owner of it asks as its owner;
 * otherwise an admin of the server asks as one, with the step-up. */
async function deleteWorkspace(home, serverAdmin) {
  if (!(await askTyped(`Delete ${home.name} for everyone? Its channels, messages, files, projects and computer go, and this cannot be undone.`, home.name, "Delete workspace"))) return;
  try {
    if (home.role === "owner" || !serverAdmin) await api("DELETE", `/v1/workspaces/${enc(home.id)}`);
    else await adminApi("DELETE", `/v1/admin/workspaces/${enc(home.id)}`);
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
