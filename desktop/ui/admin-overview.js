// Admin › Overview (spec rev 141): what is running now, computers live
// against capacity, and anything that needs an admin, each linking to the
// section that fixes it. Spend this month against its limits, a reached
// limit and a stopped agent belong to other sections: each registers a card
// here with registerOverviewCard(name, render), and Overview draws them
// beside its own. Until Spend registers one, Overview says spend is shown
// in Spend.
"use strict";

/**
 * @typedef {{what: string, fix: string}} AdminAlert
 * @typedef {{version: ?string, running: {tasks: ?number, waiting: ?number, signed_in: ?number, computers: ?number}, computers: {set_up: boolean, answers?: boolean, live?: number, max_live?: number, computers?: number, max_computers?: number, error?: string}, people: {total: number, invited: number}, kill_switch: boolean, recent_errors: ?Array<{run: string, task: string, person: string, reason: ?string, at: ?string}>, alerts: AdminAlert[]}} AdminOverview
 * @typedef {(el: HTMLElement) => (void|Promise<void>)} OverviewCard
 */

/** Cards other sections add to Overview, by name, in the order added. */
const AdminOverviewCards = new Map();

/** Add a card to Overview: `render` fills its element each time Overview
 * opens. Spend registers "spend" (this month against its limits, and any
 * limit reached); the kill switch levels may register "stops".
 * @param {string} name @param {OverviewCard} render */
function registerOverviewCard(name, render) {
  AdminOverviewCards.set(name, render);
}

/** A number, or a dash when the server did not say. @param {any} n */
function overviewNumber(n) {
  return typeof n === "number" ? String(n) : "–";
}

/** The computers card: live against what the machine may run.
 * @param {AdminOverview["computers"]} c */
function overviewComputers(c) {
  if (!c?.set_up) return `<p class="small muted">No computers are set up on this server.</p>`;
  if (!c.answers) return `<p class="small bad">The computer service is not answering${c.error ? `: ${esc(c.error)}` : ""}.</p>`;
  return `<p class="stat"><strong data-overview-live>${overviewNumber(c.live)}</strong> of ${overviewNumber(c.max_live)} running</p><p class="small muted">${overviewNumber(c.computers)} of ${overviewNumber(c.max_computers)} computers exist.</p>`;
}

/** @param {AdminOverview} o */
function adminOverviewHtml(o) {
  const r = o.running || {};
  const alerts = o.alerts || [];
  return `<h2>Overview</h2>
    <div class="admin-cards">
      <section class="card" data-overview-running><h3>Running now</h3>
        <p class="stat"><strong>${overviewNumber(r.tasks)}</strong> tasks running</p>
        <p class="small muted">${overviewNumber(r.waiting)} waiting · ${overviewNumber(r.signed_in)} people signed in · ${overviewNumber(o.people?.total)} people${o.people?.invited ? `, ${o.people.invited} invited` : ""}</p></section>
      <section class="card" data-overview-computers><h3>Computers</h3>${overviewComputers(o.computers)}</section>
      ${AdminOverviewCards.has("spend") ? "" : `<section class="card" data-overview-spend><h3>Spend this month</h3><p class="small muted">Spend against its limits is in <a href="#/admin/spend">Spend</a>.</p></section>`}
      ${[...AdminOverviewCards.keys()].map(n => `<section class="card" data-overview-card="${esc(n)}"></section>`).join("")}
    </div>
    <h3>Needs an admin</h3>
    ${alerts.length ? `<ul class="admin-alerts" data-overview-alerts>${alerts.map(a => `<li><span>${esc(a.what)}</span> <a href="#/admin/${esc(a.fix)}">Open ${esc(ADMIN_SECTIONS.find(([n]) => n === a.fix)?.[1] || a.fix)}</a></li>`).join("")}</ul>` : `<p class="small muted" data-overview-alerts>Nothing needs an admin now.</p>`}
    ${(o.recent_errors || []).length ? `<details><summary>Latest failed tasks</summary><table class="t"><tbody>${o.recent_errors.map(e => `<tr><td class="small">${esc(e.person)}</td><td class="small">${esc(e.reason || "failed")}</td><td class="small muted">${esc(when(e.at))}</td></tr>`).join("")}</tbody></table></details>` : ""}
    <p class="small muted">Magnus ${esc(o.version || "")}</p>`;
}

// The server's kill switch: what it revoked and sealed, and what to connect
// again once it is released (once Settings' Kill switch). Pressing and
// releasing are in the top bar.
registerOverviewCard("kill", async el => {
  const ks = await adminApi("GET", "/v1/admin/kill-switch");
  paint(el, typeof killSwitch === "function" ? killSwitch(ks) : `<h3>Kill switch</h3><p class="small muted">${ks.engaged ? "On." : "Off."}</p>`);
});

registerAdminSection("overview", async el => {
  /** @type {AdminOverview} */
  const o = await adminApi("GET", "/v1/admin/overview");
  paint(el, adminOverviewHtml(o));
  for (const [name, render] of AdminOverviewCards) {
    const box = $(`[data-overview-card="${name}"]`, el);
    if (box) await Promise.resolve(render(box)).catch(e => paint(box, `<p class="small bad">${esc(e.message)}</p>`));
  }
});
