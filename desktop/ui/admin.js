// Administration in the app (spec rev 141, Administration): owners and admins
// find Admin in Settings (#/admin opens it there; the navigation has no entry
// of its own, decisions/2026-11-26-admin-lives-in-settings.md), its sections, the
// kill switch always in the app's top bar wherever they are, and a step-up
// sheet whenever the server asks for the admin's password again. Everyone
// sees a banner while a kill switch that affects them is on, naming who
// pulled it and when; an admin sees one for every switch that is on.
//
// For the sections built on this:
// - registerAdminSection(name, render) draws one of ADMIN_SECTIONS. render
//   gets the section's element; it is called each time the section opens.
// - Server.admin (server.js) makes the admin calls: a change the server
//   refuses for a step-up (403 {"error":"step_up"}) opens stepUp() below and
//   is sent again once the password is confirmed.
// - registerKillLevel({level, label, choose, active}) adds a level to the
//   top bar's kill switch (person, agent, connector). See its JSDoc.
"use strict";

/**
 * @typedef {{admin: boolean, owner: boolean, step_up_seconds: number, stepped_up_until: ?string}} AdminStanding
 * @typedef {{level: "server"|"person"|"agent"|"connector", target: ?string, by: ?string, by_name: ?string, at: ?string}} Stop
 * @typedef {(el: HTMLElement) => (void|Promise<void>)} AdminRender
 * @typedef {{level: "person"|"agent"|"connector", label: string, choose: () => Promise<?{target: string, name: string}>, active?: () => Promise<Stop[]>}} KillLevel
 */

/** How often the app asks which kill switches are on. */
const ADMIN_STOPS_POLL_MS = 5000;

/** The Admin view's sections, in the order its sub-navigation shows them. */
const ADMIN_SECTIONS = [["overview", "Overview"], ["people", "People"], ["agents", "Agents"], ["models", "Models"], ["spend", "Spend"], ["connectors", "Connectors"], ["rules", "Rules"], ["security", "Security"], ["operations", "Operations"]];

/** @type {{standing: ?AdminStanding, stops: Stop[], sections: Map<string, AdminRender>, levels: KillLevel[], poll: any, stepping: ?Promise<boolean>}} */
const AdminState = { standing: null, stops: [], sections: new Map(), levels: [], poll: null, stepping: null };

/** Draw the section `name` (one of ADMIN_SECTIONS) with `render`.
 * @param {string} name @param {AdminRender} render */
function registerAdminSection(name, render) {
  if (!ADMIN_SECTIONS.some(([n]) => n === name)) throw new Error(`no Admin section named ${name}`);
  AdminState.sections.set(name, render);
}

/** Add a kill switch level to the top bar. `choose` asks the admin what to
 * stop (a person, an agent, a connector) and gives its id and name, or null;
 * pressing (Server.admin.press) needs no step-up. `active` lists that level's switches that are on, each released
 * with .../release, which does.
 * @param {KillLevel} level */
function registerKillLevel(level) {
  AdminState.levels = AdminState.levels.filter(l => l.level !== level.level).concat([level]);
}

/** The step-up sheet: the admin's password, and their code when the server
 * asks for it. One at a time: calls made together share the one sheet.
 * @returns {Promise<boolean>} whether the password was confirmed */
function stepUp() {
  if (AdminState.stepping) return AdminState.stepping;
  AdminState.stepping = new Promise(done => {
    const d = document.createElement("dialog");
    d.className = "ask step-up";
    d.setAttribute("aria-label", "Confirm your password");
    d.innerHTML = `<form class="stack" data-step-up>
      <h2>Confirm it is you</h2>
      <p class="small muted">Admin changes ask for your password again after 5 minutes, because the server can be reached from the internet.</p>
      <label>Password <input type="password" name="password" autocomplete="current-password" required></label>
      <label data-step-up-code hidden>Code from your authenticator app <input name="code" inputmode="numeric" autocomplete="one-time-code"></label>
      <p class="small bad" role="alert" data-step-up-error hidden></p>
      <div class="row"><button type="button" data-step-up-cancel>Cancel</button><button class="primary" type="submit">Confirm</button></div>
    </form>`;
    const form = $("form", d), error = $("[data-step-up-error]", d), submit = $("[type=submit]", d);
    let ok = false, busy = false;
    $("[data-step-up-cancel]", d).onclick = () => d.close();
    form.onsubmit = async e => {
      e.preventDefault();
      if (busy) return;
      busy = true;
      if (submit) { submit.disabled = true; submit.textContent = "Confirming…"; }
      const f = Object.fromEntries(new FormData(form));
      try {
        await Server.admin.stepUp(f.password, f.code || null);
        ok = true; d.close();
      } catch (err) {
        if (err?.body?.error === "code_needed") { $("[data-step-up-code]", d).hidden = false; $("[name=code]", d).focus(); }
        error.textContent = err?.body?.message || err.message; error.hidden = false;
        busy = false;
        if (submit) { submit.disabled = false; submit.textContent = "Confirm"; }
      }
    };
    d.addEventListener("close", () => { d.remove(); AdminState.stepping = null; done(ok); });
    document.body.append(d);
    d.showModal();
    $("[name=password]", d).focus();
  });
  return AdminState.stepping;
}

/** What a stop's banner says.
 * @param {Stop} s */
function stopText(s) {
  const who = s.by_name || "An admin", at = s.at ? ` ${when(s.at)}` : "";
  if (s.level === "server") return `<strong>The kill switch is on.</strong> ${esc(who)} stopped everything on the server${esc(at)}. No task acts until an admin releases it.`;
  // An admin sees every switch, also those that stop someone else's work.
  const person = s.mine === false ? `${s.target_name || s.target}'s agents` : "your agents";
  const what = { person, agent: "an agent", connector: "a connector" }[s.level] || "some work";
  return `<strong>A kill switch is on.</strong> ${esc(who)} stopped ${esc(what)}${s.target && s.level !== "person" ? ` (${esc(s.target_name || s.target)})` : ""}${esc(at)}.`;
}

/** The banner shown while any stop affects the signed-in person. */
function drawStopBanner() {
  let el = $("#stop-banner");
  if (!AdminState.stops.length) { el?.remove(); return; }
  if (!el) { el = document.createElement("div"); el.id = "stop-banner"; el.setAttribute("role", "status"); document.body.append(el); }
  const link = AdminState.standing?.admin && !location.hash.startsWith("#/admin") ? ` <a href="#/admin">Open Admin</a>` : "";
  paint(el, AdminState.stops.map(s => `<div class="banner bad" data-stop="${esc(s.level)}">${stopText(s)}${link}</div>`).join(""));
}

/** Ask which switches are on, and redraw what shows them. */
async function adminPoll() {
  try { AdminState.stops = (await Server.stops.list({ background: true })).stops || []; }
  catch { return; }
  drawStopBanner();
  if ($("[data-admin-kill]")) drawKillControl();
}

/** After sign-in: whether this person administers, and the stops banner. */
async function adminStart() {
  adminStop();
  AdminState.standing = await Server.admin.standing({ background: true }).catch(() => null);
  if (AdminState.standing?.admin) showKillControl();
  AdminState.poll = setInterval(() => { if (!document.hidden) adminPoll(); }, ADMIN_STOPS_POLL_MS);
  await adminPoll();
}

/** Every kill-switch mount currently in the DOM (right panel + persistent top bar). */
function killBoxes() {
  if (typeof $$ === "function") {
    const all = $$("[data-admin-kill], [data-admin-kill-top]");
    if (all.length) return all;
  }
  const one = $("[data-admin-kill]");
  return one ? [one] : [];
}

/** The kill switch is always in an admin's top bar, wherever they are in
 * the app (spec rev 141, Kill switch): the app's bar holds its place, shown
 * once the person is known to administer. */
function showKillControl() {
  const boxes = killBoxes();
  if (!boxes.length) return;
  for (const box of boxes) {
    box.hidden = false;
    if (!box.dataset.wired) { box.dataset.wired = "1"; box.addEventListener("click", onKillClick); }
  }
  document.body.dataset.adminBar = "";
  drawKillControl();
}

/** At sign-out: nothing of this person's stays. */
function adminStop() {
  clearInterval(AdminState.poll);
  AdminState.poll = null; AdminState.standing = null; AdminState.stops = [];
  $("#stop-banner")?.remove();
  for (const box of killBoxes()) { box.hidden = true; paint(box, ""); }
  delete document.body.dataset?.adminBar;
}

/** The kill switch in the admin's top bar: the server's, and each level's. */
async function drawKillControl() {
  const boxes = killBoxes();
  if (!boxes.length || !AdminState.standing?.admin) return;
  const server = AdminState.stops.find(s => s.level === "server");
  const more = (await Promise.all(AdminState.levels.map(l => l.active ? l.active().catch(() => []) : []))).flat();
  const release = s => `<button data-kill-release="${esc(s.level)}" data-kill-target="${esc(s.target || "")}">Release${s.level === "server" ? "" : ` ${esc(s.level)} ${esc(s.target || "")}`}</button>`;
  const pressMenu = `<details class="kill-menu"><summary class="danger">Emergency Stop ▾</summary><div class="kill-menu-list">${server ? "" : `<button class="danger" title="Refuses all actions, suspends all tasks, revokes sign-ins and seals the vault" data-kill-press="server">Stop everything</button>`}${AdminState.levels.map(l => `<button data-kill-press="${esc(l.level)}">${esc(l.label)}</button>`).join("")}</div></details>`;
  const html = `${server ? `<span class="chip bad">Everything is stopped</span>${release(server)}` : ""}${pressMenu}${more.map(release).join("")}`;
  for (const box of boxes) paint(box, html);
}

/** Pressing needs no question and no step-up: stopping is always safe.
 * Releasing may be held for the password, which Server asks for when due.
 * @param {MouseEvent} e */
async function onKillClick(e) {
  const press = /** @type {HTMLElement} */ (e.target).closest("[data-kill-press]");
  const free = /** @type {HTMLElement} */ (e.target).closest("[data-kill-release]");
  const menu = /** @type {HTMLDetailsElement|null} */ (/** @type {HTMLElement} */ (e.target).closest("details.kill-menu"));
  try {
    if (press) {
      if (menu) menu.open = false;
      const level = press.dataset.killPress;
      const l = AdminState.levels.find(x => x.level === level);
      const chosen = level === "server" ? { target: null, name: "everything" } : await l?.choose();
      if (!chosen) return;
      await Server.admin.press(level, chosen.target);
      toast(level === "server" ? "Everything is stopped." : `Stopped ${chosen.name}.`);
    } else if (free) {
      await Server.admin.release(free.dataset.killRelease, free.dataset.killTarget || null);
      toast("Released. Stopped tasks stay stopped until each is continued.");
    } else return;
  } catch (err) { if (!err.cancelled) toast(err.message); }
  await adminPoll();
  drawKillControl();
}

/** The Admin view: its top bar (sections and the kill switch) and a section.
 * @param {HTMLElement} main @param {string} [which] */
async function adminView(main, which = "overview") {
  if (!AdminState.standing) AdminState.standing = await Server.admin.standing();
  if (!AdminState.standing.admin) { paint(main, `<p class="empty">Administration is for the server's owner and admins.</p>`); return; }
  const name = ADMIN_SECTIONS.some(([n]) => n === which) ? which : "overview";
  const label = ADMIN_SECTIONS.find(([n]) => n === name)?.[1];
  paint(main, `<div class="admin" data-admin>
    <header class="admin-bar"><h1>Admin</h1></header>
    <nav class="admin-sub" aria-label="Admin sections">${ADMIN_SECTIONS.map(([n, l]) => `<a href="#/admin/${n}" data-admin-tab="${n}"${n === name ? ' class="on" aria-current="page"' : ""}>${esc(l)}</a>`).join("")}</nav>
    <section class="admin-section" data-admin-section="${name}" aria-label="${esc(label)}"></section>
  </div>`);
  showKillControl();
  drawStopBanner();
  const section = $("[data-admin-section]", main), render = AdminState.sections.get(name);
  if (render) await render(section);
  else paint(section, `<h2>${esc(label)}</h2><p class="empty">${esc(label)} is not in the app yet.</p>`);
}

route(/^#\/admin(?:\/([a-z]+))?$/, async (main, which) => {
  if (!AdminState.standing) AdminState.standing = await Server.admin.standing().catch(() => null);
  if (AdminState.standing?.admin && typeof openSettingsModal === "function") {
    await openSettingsModal(`admin:${which || "overview"}`);
    return;
  }
  await adminView(main, which);
});
