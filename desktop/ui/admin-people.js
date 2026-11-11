// Admin › People (spec rev 141): invite by email or link, set each person's
// role (owner, admin, member or guest), turn an account off, sign them out
// of every device, and remove them. Removing asks whether their team
// projects and agents go to someone else or are deleted; their own data is
// deleted either way, and the last owner cannot be removed. Opening a
// person's data is recorded and they are told, which the page says before
// anything is opened.
"use strict";

/**
 * @typedef {{id: string, name: string, email: ?string, role: "owner"|"admin"|"member"|"guest", status: "invited"|"active"|"disabled", last_sign_in: ?string, sessions: number}} AdminPerson
 * @typedef {{id: string, email: ?string, invite_token: ?string, link: ?string, emailed: boolean, email_error?: string}} AdminInvite
 */

const ADMIN_ROLES = [["owner", "Owner"], ["admin", "Admin"], ["member", "Member"], ["guest", "Guest: reads and chats only"]];
const ADMIN_DATA_KINDS = [["tasks", "Tasks"], ["chats", "Chats"], ["files", "Files"], ["memory", "Memory"]];

/** What the admin passes on after an invitation: the link, and whether it
 * went by email. @param {AdminInvite} r */
function peopleInvited(r) {
  const how = r.emailed ? "We emailed them the link." : r.email_error ? `The email did not go out (${esc(r.email_error)}).` : "Send them this link yourself.";
  const link = r.link || (r.invite_token ? `Invitation code: ${r.invite_token}` : "");
  return `<div class="banner" data-people-invited><p>${how} It works once.</p>${link ? `<p><input readonly class="mono" value="${esc(link)}" aria-label="Invitation link" data-people-link> <button type="button" data-people-copy>Copy</button></p>` : ""}</div>`;
}

/** @param {AdminPerson} p @param {boolean} owner whether the viewer is an owner */
function peopleRow(p, owner) {
  const roles = ADMIN_ROLES.filter(([r]) => owner || r !== "owner" || p.role === "owner");
  const lock = p.role === "owner" && !owner ? " disabled" : "";
  const state = { invited: "Invited", active: "Active", disabled: "Turned off" }[p.status] || p.status;
  return `<tr data-person="${esc(p.id)}">
    <td><strong>${esc(p.name)}</strong><div class="small muted">${esc(p.email || p.id)}</div></td>
    <td><select data-people-role aria-label="Role of ${esc(p.name)}"${lock}>${roles.map(([r, l]) => `<option value="${r}"${r === p.role ? " selected" : ""}>${esc(l)}</option>`).join("")}</select></td>
    <td><span class="chip${p.status === "disabled" ? " bad" : ""}">${esc(state)}</span><div class="small muted">${p.last_sign_in ? `Signed in ${esc(when(p.last_sign_in))}` : "Never signed in"} · ${p.sessions} device${p.sessions === 1 ? "" : "s"}</div></td>
    <td class="row">
      ${p.status === "invited" ? `<button data-people-act="resend">New link</button><button data-people-act="cancel">Withdraw</button>` : ""}
      ${p.status === "active" ? `<button data-people-act="sign-out">Sign out everywhere</button><button data-people-act="disable">Turn off</button>` : ""}
      ${p.status === "disabled" ? `<button data-people-act="enable">Turn on</button>` : ""}
      <button data-people-act="data">Their data</button>
      <button class="danger" data-people-act="remove">Remove</button>
    </td></tr>`;
}

/** @param {AdminPerson[]} people @param {boolean} owner */
function peopleHtml(people, owner) {
  return `<h2>People</h2>
    <form class="row" data-people-invite>
      <input name="name" placeholder="Name" aria-label="Name" required>
      <input name="email" type="email" placeholder="Email address" aria-label="Email address" required>
      <select name="role" aria-label="Role">${ADMIN_ROLES.filter(([r]) => owner || r !== "owner").map(([r, l]) => `<option value="${r}"${r === "member" ? " selected" : ""}>${esc(l)}</option>`).join("")}</select>
      <label class="small"><input type="checkbox" name="send" checked> Email the link</label>
      <button class="primary">Invite</button>
    </form>
    <div data-people-note></div>
    <table class="t" data-people><thead><tr><th>Person</th><th>Role</th><th>Status</th><th></th></tr></thead>
      <tbody>${people.map(p => peopleRow(p, owner)).join("")}</tbody></table>
    <div data-people-detail></div>`;
}

/** The removal sheet: transfer to someone, or delete. Resolves to the body
 * to send, or null when cancelled.
 * @param {AdminPerson} p @param {AdminPerson[]} people */
function peopleRemoveSheet(p, people) {
  return new Promise(done => {
    const d = document.createElement("dialog");
    d.className = "ask";
    d.setAttribute("aria-label", `Remove ${p.name}`);
    const others = people.filter(x => x.id !== p.id && x.status === "active");
    d.innerHTML = `<form class="stack" data-people-remove>
      <h2>Remove ${esc(p.name)}</h2>
      <p class="small">Their own chats, tasks, files and memory are deleted, and their devices signed out. This cannot be undone.</p>
      <fieldset><legend>Their team projects, routines and workspaces</legend>
        <label><input type="radio" name="projects" value="transfer" checked> Give them to <select name="to" aria-label="Who receives them">${others.map(x => `<option value="${esc(x.id)}">${esc(x.name)}</option>`).join("")}</select></label>
        <label><input type="radio" name="projects" value="delete"> Delete them</label>
      </fieldset>
      <div class="row"><button type="button" data-cancel>Cancel</button><button class="danger" type="submit">Remove</button></div></form>`;
    let out = null;
    $("[data-cancel]", d).onclick = () => d.close();
    $("form", d).onsubmit = e => {
      e.preventDefault();
      const f = Object.fromEntries(new FormData(/** @type {HTMLFormElement} */ (e.target)));
      out = { projects: f.projects, to: f.to || null };
      d.close();
    };
    d.addEventListener("close", () => { d.remove(); done(out); });
    document.body.append(d);
    d.showModal();
  });
}

/** Their data, after saying the read is recorded and they are told.
 * @param {HTMLElement} box @param {AdminPerson} p */
function peopleData(box, p) {
  paint(box, `<section class="card" data-people-data="${esc(p.id)}"><h3>${esc(p.name)}'s data</h3>
    <p class="small">Opening it is recorded in the audit log, and ${esc(p.name)} is told who looked and what.</p>
    <div class="row">${ADMIN_DATA_KINDS.map(([k, l]) => `<button data-people-read="${k}">${esc(l)}</button>`).join("")}</div>
    <pre class="small" data-people-out hidden></pre></section>`);
  box.onclick = async e => {
    const b = /** @type {HTMLElement} */ (e.target).closest("[data-people-read]");
    if (!b) return;
    try {
      const v = await adminApi("POST", `/v1/admin/people/${enc(p.id)}/data/${b.dataset.peopleRead}`, {});
      const out = $("[data-people-out]", box);
      out.textContent = JSON.stringify(v[b.dataset.peopleRead], null, 2);
      out.hidden = false;
    } catch (err) { if (!err.cancelled) toast(err.body?.message || err.message); }
  };
}

registerAdminSection("people", async el => {
  /** @type {{people: AdminPerson[]}} */
  const v = await adminApi("GET", "/v1/admin/people");
  const owner = !!AdminState.standing?.owner;
  const people = v.people || [];
  paint(el, peopleHtml(people, owner));
  const note = $("[data-people-note]", el);
  const again = () => AdminState.sections.get("people")?.(el);
  /** @param {any} err */
  const failed = err => { if (!err.cancelled) toast(err.body?.message || err.message); };
  $("[data-people-invite]", el).onsubmit = async e => {
    e.preventDefault();
    const form = /** @type {HTMLFormElement} */ (e.target), f = Object.fromEntries(new FormData(form));
    try {
      const r = await adminApi("POST", "/v1/admin/people", { name: f.name, email: f.email, role: f.role, send: f.send === "on" });
      await again();
      paint($("[data-people-note]", el), peopleInvited(r));
    } catch (err) { failed(err); }
  };
  el.onchange = async e => {
    const s = /** @type {HTMLSelectElement} */ (e.target);
    if (!s.matches("[data-people-role]")) return;
    const id = s.closest("[data-person]")?.getAttribute("data-person");
    try { await adminApi("PATCH", `/v1/admin/people/${enc(id)}`, { role: s.value }); toast("Role changed."); }
    catch (err) { failed(err); }
    await again();
  };
  el.onclick = async e => {
    const t = /** @type {HTMLElement} */ (e.target);
    if (t.matches("[data-people-copy]")) { navigator.clipboard?.writeText($("[data-people-link]", el)?.value || ""); toast("Copied."); return; }
    const b = t.closest("[data-people-act]");
    if (!b) return;
    const id = b.closest("[data-person]")?.getAttribute("data-person");
    const p = people.find(x => x.id === id);
    if (!p) return;
    const base = `/v1/admin/people/${enc(id)}`;
    try {
      switch (b.dataset.peopleAct) {
        case "resend": paint(note, peopleInvited(await adminApi("POST", `${base}/invite`, { send: true }))); return;
        case "cancel": await adminApi("POST", `${base}/cancel-invite`, {}); toast("Invitation withdrawn."); break;
        case "sign-out": { const r = await adminApi("POST", `${base}/sign-out`, {}); toast(`Signed out of ${r.ended ?? 0} device${r.ended === 1 ? "" : "s"}.`); break; }
        case "disable": if (!confirm(`Turn off ${p.name}? They are signed out everywhere and their schedules pause.`)) return; await adminApi("POST", `${base}/disable`, {}); toast(`${p.name} is turned off.`); break;
        case "enable": await adminApi("POST", `${base}/enable`, {}); toast(`${p.name} is turned on.`); break;
        case "data": peopleData($("[data-people-detail]", el), p); return;
        case "remove": {
          const body = await peopleRemoveSheet(p, people);
          if (!body) return;
          await adminApi("DELETE", base, body);
          toast(`${p.name} is removed.`);
          break;
        }
        default: return;
      }
    } catch (err) { failed(err); }
    await again();
  };
});
