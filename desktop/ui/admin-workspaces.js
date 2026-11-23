// Admin › People › Shared projects (pure peer collaboration, 2026-11-22).
// Any regular user may start a shared project and collaborate as peers. A
// Magnus server admin sees only shared project names and counts — never
// channel names, members, or content, and can never join or modify one.
"use strict";

/**
 * @typedef {{ id: string, name: string, owner: boolean }} AdminWsAdmin
 * @typedef {{ id: string, name: string, team_admins?: AdminWsAdmin[], members: number, channels: number }} AdminWorkspace
 * @typedef {{ id: string, name: string, team_admin?: boolean }} AdminWsPerson
 */

/** @param {AdminWorkspace[]} list */
function adminWorkspacesHtml(list) {
  const one = w => `<div class="card stack" data-ws="${esc(w.id)}"><div class="spread"><strong>${esc(w.name)}</strong><span class="small muted">${w.members} ${w.members === 1 ? "person" : "people"} · ${w.channels} ${w.channels === 1 ? "conversation" : "conversations"}</span></div></div>`;
  return `<h2>Shared projects</h2><p class="small muted">Any regular user can start a shared project and invite teammates. You see only project names and counts — never conversations or content.</p>
    ${list.length ? list.map(one).join("") : `<p class="small muted">No shared projects yet.</p>`}`;
}

/** Draw the Shared projects overview of Admin › People into `box`. @param {?HTMLElement} box */
async function adminWorkspaces(box) {
  if (!box) return;
  /** @type {{ workspaces: AdminWorkspace[] }} */
  const v = await Server.admin.workspaces().catch(() => ({ workspaces: [] }));
  paint(box, adminWorkspacesHtml(v.workspaces || []));
}
