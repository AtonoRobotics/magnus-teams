// Admin › People › Workspaces and Team Admins (the owner, 2026-11-11). A
// Magnus admin supports the server, not its teams: they see each workspace
// by name, its Team Admins and how many people and channels it has, and
// make someone a Team Admin (who may start workspaces) or take it back,
// audited and after the step-up. They neither join, change nor delete a
// workspace: its Team Admins do that from the workspace's own settings.
"use strict";

/**
 * @typedef {{ id: string, name: string, owner: boolean }} AdminWsAdmin
 * @typedef {{ id: string, name: string, team_admins: AdminWsAdmin[], members: number, channels: number }} AdminWorkspace
 * @typedef {{ id: string, name: string, team_admin: boolean }} AdminWsPerson
 */

/** @param {AdminWorkspace[]} list @param {AdminWsPerson[]} people */
function adminWorkspacesHtml(list, people) {
  const one = w => `<div class="card stack" data-ws="${esc(w.id)}"><div class="spread"><strong>${esc(w.name)}</strong><span class="small muted">${w.members} ${w.members === 1 ? "person" : "people"} · ${w.channels} channels</span></div>
    <p class="small">Team Admins: ${w.team_admins.map(a => `${esc(a.name)}${a.owner ? " (owner)" : ""}`).join(", ") || "none"}</p></div>`;
  const person = p => `<tr data-ws-person="${esc(p.id)}"><td>${esc(p.name)}</td><td><label class="small"><input type="checkbox" data-team-admin${p.team_admin ? " checked" : ""}> Team Admin</label></td></tr>`;
  return `<h2>Team Admins</h2><p class="small muted">A Team Admin starts workspaces and owns those they start. What is in a workspace is its team's: you see only its name and counts.</p>
    <table class="t"><tbody>${people.map(person).join("")}</tbody></table>
    <h2>Workspaces</h2>${list.length ? list.map(one).join("") : `<p class="small muted">No workspaces yet.</p>`}`;
}

/** Draw and bind the Workspaces part of Admin › People into `box`. @param {?HTMLElement} box */
async function adminWorkspaces(box) {
  if (!box) return;
  /** @type {{ workspaces: AdminWorkspace[], people: AdminWsPerson[] }} */
  const v = await adminApi("GET", "/v1/admin/workspaces").catch(() => ({ workspaces: [], people: [] }));
  paint(box, adminWorkspacesHtml(v.workspaces || [], v.people || []));
  box.onchange = async e => {
    const c = /** @type {HTMLInputElement} */ (e.target);
    const id = c.closest("[data-ws-person]")?.getAttribute("data-ws-person");
    if (!c.matches("[data-team-admin]") || !id) return;
    try { await adminApi("PUT", `/v1/admin/people/${enc(id)}/team-admin`, { on: c.checked }); toast(c.checked ? "Made a Team Admin." : "No longer a Team Admin."); }
    catch (err) { if (!err.cancelled) toast(err.body?.message || err.body?.error || err.message); }
    await adminWorkspaces(box);
  };
}
