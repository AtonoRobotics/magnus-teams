// Admin › Operations (spec rev 141): updates (channel, install, roll back),
// backups and their restore checks, computers and capacity, and health.
// Every backup is restored in memory as soon as it is written, and the
// result of that check is shown with it.
"use strict";

/**
 * @typedef {{version: string, automatic: boolean, latest: ?{version: string, notes: ?string}, available: boolean, updating: ?{from: string, to: string, rollback: boolean}, last: ?{what: string, to: string, detail: string, at: string}, previous: ?string, desktop: {channels: Array<{channel: string, published: ?string, who: string}>, everyone: string}}} AdminUpdates
 * @typedef {{every_hours: number, keep_days: number, copy_to: ?string, latest: ?{file: string, restored: boolean, detail: string, at: string}, copy: ?{copied: boolean, detail: string, at: string}, files: Array<{file: string, bytes: number}>, scrub_pending: ?{files: string[], detail: string}}} AdminBackups
 * @typedef {{computers: Array<{id: string, person: ?string, workspace: ?string, status: string, controller: ?string}>, capacity: {set_up: boolean, answers?: boolean, live?: number, max_live?: number, computers?: number, max_computers?: number, error?: string}}} AdminComputers
 * @typedef {{services: Array<{name: string, up: boolean, note?: ?string}>, host: {cpus: number, load: ?number, memory_used: ?number, disk_used: ?number}}} AdminHealth
 */

/** A fraction as a whole percentage. @param {?number} f */
const opsPercent = f => typeof f === "number" ? `${Math.round(f * 100)}%` : "–";

/** @param {AdminUpdates} u */
function opsUpdates(u) {
  const last = u.last ? `<p class="small${u.last.what === "failed" ? " bad" : " muted"}">Last: ${esc(u.last.what)} ${esc(u.last.to)} ${esc(when(u.last.at))}${u.last.detail ? `: ${esc(u.last.detail)}` : ""}</p>` : "";
  return `<section class="card" data-ops-updates><h3>Updates</h3>
    <p>Magnus ${esc(u.version)}${u.updating ? ` <span class="chip">${u.updating.rollback ? "Rolling back" : "Updating"} to ${esc(u.updating.to)}</span>` : ""}</p>
    ${u.available ? `<p>${esc(u.latest?.version)} is available.</p><button class="primary" data-ops-install>Install ${esc(u.latest?.version)}</button>` : `<p class="small muted">Up to date${u.automatic ? "; updates install on their own" : ""}.</p>`}
    ${u.previous ? `<button data-ops-rollback>Roll back to ${esc(u.previous)}</button>` : ""}
    ${last}
    <label class="small">Desktop apps of everyone else follow <select data-ops-channel aria-label="Desktop channel">${(u.desktop?.channels || []).map(c => `<option value="${esc(c.channel)}"${c.channel === u.desktop.everyone ? " selected" : ""}>${esc(c.channel)}${c.published ? ` (${esc(c.published)})` : ""}</option>`).join("")}</select></label>
  </section>`;
}

/** @param {AdminBackups} b */
function opsBackups(b) {
  const l = b.latest;
  return `<section class="card" data-ops-backups><h3>Backups</h3>
    <p class="small muted">Every ${esc(b.every_hours)} hours, each kept ${esc(b.keep_days)} days${b.copy_to ? `, copied to ${esc(b.copy_to)}` : ", on this server's disk only"}.</p>
    ${l ? `<p data-ops-restore-check class="${l.restored ? "" : "bad"}">${l.restored ? "The latest backup restored in its check" : "The latest backup did not restore"} (${esc(when(l.at))})${l.restored ? "." : `: ${esc(l.detail)}`}</p>` : `<p class="small muted" data-ops-restore-check>No backup yet.</p>`}
    ${b.copy && !b.copy.copied ? `<p class="small bad">The copy off the server failed: ${esc(b.copy.detail)}</p>` : ""}
    ${b.scrub_pending ? `<p class="small bad">${b.scrub_pending.files.length} backups still hold deleted content: ${esc(b.scrub_pending.detail)}</p>` : ""}
    <p class="small">${b.files.length} backup${b.files.length === 1 ? "" : "s"} kept.</p>
    <button data-ops-backup>Back up now</button></section>`;
}

/** @param {AdminComputers} c */
function opsComputers(c) {
  const cap = c.capacity;
  const head = !cap.set_up ? "No computers are set up on this server." : !cap.answers ? `The computer service is not answering: ${esc(cap.error || "")}` : `${cap.live} of ${cap.max_live} running; ${cap.computers} of ${cap.max_computers} exist.`;
  return `<section class="card" data-ops-computers><h3>Computers and capacity</h3><p class="small">${head}</p>
    ${c.computers.length ? `<table class="t"><tbody>${c.computers.map(x => `<tr><td class="small mono">${esc(x.id)}</td><td class="small">${esc(x.person || x.workspace || "")}</td><td class="small">${esc(x.status)}${x.controller ? `, ${esc(x.controller)} has the screen` : ""}</td></tr>`).join("")}</tbody></table>` : ""}</section>`;
}

/** @param {AdminHealth} h */
function opsHealth(h) {
  return `<section class="card" data-ops-health><h3>Health</h3>
    <ul class="small">${h.services.map(s => `<li><span class="chip${s.up ? "" : " bad"}">${s.up ? "Up" : "Down"}</span> ${esc(s.name)}${s.note ? ` <span class="muted">${esc(s.note)}</span>` : ""}</li>`).join("")}</ul>
    <p class="small muted">${esc(h.host.cpus)} CPUs, load ${esc(h.host.load ?? "–")} · memory ${opsPercent(h.host.memory_used)} used · disk ${opsPercent(h.host.disk_used)} used</p></section>`;
}

registerAdminSection("operations", async el => {
  const [u, b, c, h] = await Promise.all([adminApi("GET", "/v1/admin/updates"), adminApi("GET", "/v1/admin/backups"), adminApi("GET", "/v1/admin/computers"), adminApi("GET", "/v1/admin/health")]);
  paint(el, `<h2>Operations</h2><div class="admin-cards">${opsUpdates(u)}${opsBackups(b)}${opsComputers(c)}${opsHealth(h)}</div>`);
  const again = () => AdminState.sections.get("operations")?.(el);
  /** @param {any} err */
  const failed = err => { if (!err.cancelled) toast(err.body?.error || err.message); };
  el.onclick = async e => {
    const t = /** @type {HTMLElement} */ (e.target);
    try {
      if (t.matches("[data-ops-install]")) { if (!confirm("Install the update now? Magnus restarts, and running tasks continue after.")) return; await adminApi("POST", "/v1/admin/updates/install", {}); toast("Installing."); }
      else if (t.matches("[data-ops-rollback]")) { if (!confirm("Go back to the release kept? Magnus restarts.")) return; await adminApi("POST", "/v1/admin/updates/rollback", {}); toast("Rolling back."); }
      else if (t.matches("[data-ops-backup]")) { t.setAttribute("disabled", ""); await adminApi("POST", "/v1/admin/backups", {}); toast("Backed up."); }
      else return;
    } catch (err) { failed(err); }
    await again();
  };
  el.onchange = async e => {
    const s = /** @type {HTMLSelectElement} */ (e.target);
    if (!s.matches("[data-ops-channel]")) return;
    try { await adminApi("PUT", "/v1/admin/updates/desktop-channel", { channel: s.value }); toast("Saved."); }
    catch (err) { failed(err); }
    await again();
  };
});
