// Admin › People › Workspaces: every workspace on the server with its
// people. An admin of the server adds someone, changes a role, takes a
// member out, or deletes the workspace, each audited and after the step-up.
// The workspace's own rule still holds: it keeps an owner.
"use strict";

/**
 * @typedef {{ id: string, name: string, role: "owner"|"admin"|"member"|"guest" }} AdminWsMember
 * @typedef {{ id: string, name: string, members: AdminWsMember[], channels: number }} AdminWorkspace
 */

const ADMIN_WS_ROLES = ["guest", "member", "admin", "owner"];

/** @param {AdminWorkspace[]} list @param {{ id: string, name: string }[]} people */
function adminWorkspacesHtml(list, people) {
  if (!list.length) return `<h2>Workspaces</h2><p class="small muted">No workspaces yet.</p>`;
  const roles = sel => ADMIN_WS_ROLES.map(r => `<option${r === sel ? " selected" : ""}>${r}</option>`).join("");
  const one = w => {
    const owners = w.members.filter(m => m.role === "owner").length, inside = new Set(w.members.map(m => m.id));
    const others = people.filter(p => !inside.has(p.id));
    const row = m => {
      const last = m.role === "owner" && owners <= 1;
      return `<tr data-ws-member="${esc(m.id)}" data-name="${esc(m.name)}"><td>${esc(m.name)}</td><td><select data-ws-role aria-label="Role of ${esc(m.name)} in ${esc(w.name)}"${last ? " disabled" : ""}>${roles(m.role)}</select></td>
        <td>${last ? `<span class="small muted">Its only owner</span>` : `<button data-ws-remove>Remove</button>`}</td></tr>`;
    };
    return `<div class="card stack" data-ws="${esc(w.id)}" data-ws-name="${esc(w.name)}"><div class="spread"><strong>${esc(w.name)}</strong><span class="small muted">${w.members.length} ${w.members.length === 1 ? "person" : "people"} · ${w.channels} channels</span></div>
      <table class="t"><tbody>${w.members.map(row).join("")}</tbody></table>
      ${others.length ? `<form class="row" data-ws-add><select name="person" aria-label="Person to add to ${esc(w.name)}">${others.map(p => `<option value="${esc(p.id)}">${esc(p.name)}</option>`).join("")}</select><select name="role" aria-label="Their role">${roles("member")}</select><button>Add</button></form>` : ""}
      <div class="row"><button class="danger" data-ws-delete>Delete workspace</button></div></div>`;
  };
  return `<h2>Workspaces</h2><p class="small muted">Deleting one deletes its channels, messages, files, projects and computer for everyone, from backups too within 30 days.</p>${list.map(one).join("")}`;
}

/** Draw and bind the Workspaces part of Admin › People into `box`. @param {?HTMLElement} box */
async function adminWorkspaces(box) {
  if (!box) return;
  /** @type {{ workspaces: AdminWorkspace[], people: { id: string, name: string }[] }} */
  const v = await adminApi("GET", "/v1/admin/workspaces").catch(() => ({ workspaces: [], people: [] }));
  paint(box, adminWorkspacesHtml(v.workspaces || [], v.people || []));
  const again = () => adminWorkspaces(box);
  /** @param {any} err */
  const failed = err => { if (!err.cancelled) toast(err.body?.message || err.body?.error || err.message); };
  const where = e => /** @type {HTMLElement} */ (e.target).closest("[data-ws]");
  box.onchange = async e => {
    const s = /** @type {HTMLSelectElement} */ (e.target), w = where(e);
    if (!s.matches("[data-ws-role]") || !w) return;
    const person = s.closest("[data-ws-member]")?.getAttribute("data-ws-member");
    try { await adminApi("POST", `/v1/admin/workspaces/${enc(w.dataset.ws)}/members`, { person, role: s.value }); toast("Role changed."); } catch (err) { failed(err); }
    await again();
  };
  box.onsubmit = async e => {
    const form = /** @type {HTMLFormElement} */ (e.target), w = where(e);
    if (!form.matches("[data-ws-add]") || !w) return;
    e.preventDefault();
    const f = Object.fromEntries(new FormData(form));
    try { await adminApi("POST", `/v1/admin/workspaces/${enc(w.dataset.ws)}/members`, { person: f.person, role: f.role }); toast("Added."); } catch (err) { failed(err); }
    await again();
  };
  box.onclick = async e => {
    const t = /** @type {HTMLElement} */ (e.target), w = where(e);
    if (!w) return;
    const name = w.dataset.wsName || "";
    if (t.closest("[data-ws-remove]")) {
      const m = t.closest("[data-ws-member]");
      if (!(await ask(`Take ${m?.getAttribute("data-name")} out of ${name}? They lose it at once and their running work there stops. What they wrote stays.`, "Remove"))) return;
      try { await adminApi("DELETE", `/v1/admin/workspaces/${enc(w.dataset.ws)}/members/${enc(m?.getAttribute("data-ws-member"))}`); toast("Removed."); } catch (err) { failed(err); }
      await again();
    }
    if (t.closest("[data-ws-delete]")) {
      if (!(await askTyped(`Delete ${name} for everyone? Its channels, messages, files, projects and computer go, and this cannot be undone.`, name, "Delete workspace"))) return;
      try { await adminApi("DELETE", `/v1/admin/workspaces/${enc(w.dataset.ws)}`); toast(`${name} was deleted.`); delete Team.homes[w.dataset.ws]; } catch (err) { failed(err); }
      await again();
    }
  };
}
