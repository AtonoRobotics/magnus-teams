// Administration in the app (spec rev 141, Administration): owners and admins
// see an Admin entry in the navigation, and an Admin view with its sections,
// the kill switch always in its top bar, and a step-up sheet whenever the
// server asks for the admin's password again. Everyone sees a banner while a
// kill switch that affects them is on, naming who pulled it and when.
//
// For the sections built on this:
// - registerAdminSection(name, render) draws one of ADMIN_SECTIONS. render
//   gets the section's element; it is called each time the section opens.
// - adminApi(method, path, body) is api() for admin calls: a change the
//   server refuses for a step-up (403 {"error":"step_up"}) opens the step-up
//   sheet and is sent again once the password is confirmed.
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
 * pressing posts /v1/admin/kill-switch/<level>/<id>/press, which needs no
 * step-up. `active` lists that level's switches that are on, each released
 * with .../release, which does.
 * @param {KillLevel} level */
function registerKillLevel(level) {
  AdminState.levels = AdminState.levels.filter(l => l.level !== level.level).concat([level]);
}

/** The path that presses or releases a switch: the server's, or a level's.
 * @param {string} level @param {?string} target @param {"press"|"release"} act */
function killPath(level, target, act) {
  return level === "server" ? `/v1/admin/kill-switch/${act}` : `/v1/admin/kill-switch/${level}/${encodeURIComponent(target || "")}/${act}`;
}

/** api() for admin calls: a change refused for a step-up asks for the
 * password and is sent again; cancelled, it throws a cancelled error.
 * @param {string} m @param {string} p @param {any} [b] */
async function adminApi(m, p, b) {
  try { return await api(m, p, b); }
  catch (e) {
    if (e?.status !== 403 || e?.body?.error !== "step_up") throw e;
    if (!(await stepUp())) throw Object.assign(new Error("The change was not made."), { cancelled: true });
    return api(m, p, b);
  }
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
    const form = $("form", d), error = $("[data-step-up-error]", d);
    let ok = false;
    $("[data-step-up-cancel]", d).onclick = () => d.close();
    form.onsubmit = async e => {
      e.preventDefault();
      const f = Object.fromEntries(new FormData(form));
      try {
        await api("POST", "/v1/admin/step-up", { password: f.password, code: f.code || null });
        ok = true; d.close();
      } catch (err) {
        if (err?.body?.error === "code_needed") { $("[data-step-up-code]", d).hidden = false; $("[name=code]", d).focus(); }
        error.textContent = err?.body?.message || err.message; error.hidden = false;
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
  const what = { person: "your agents", agent: "an agent", connector: "a connector" }[s.level] || "some work";
  return `<strong>A kill switch is on.</strong> ${esc(who)} stopped ${what}${s.target && s.level !== "person" ? ` (${esc(s.target)})` : ""}${esc(at)}.`;
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
  try { AdminState.stops = (await Magnus.api("GET", "/v1/stops")).stops || []; }
  catch { return; }
  drawStopBanner();
  if ($("[data-admin-kill]")) drawKillControl();
}

/** The Admin entry in the navigation, for owners and admins.
 * @param {string} route */
function adminNavItem(route) {
  if (!AdminState.standing?.admin) return "";
  const on = route.startsWith("#/admin");
  return `<a class="primary-nav${on ? " on" : ""}" href="#/admin" data-admin-nav ${on ? 'aria-current="page"' : ""} aria-label="Admin" title="Admin">${icon("shield")}<span class="nav-label">Admin</span></a>`;
}

/** After sign-in: whether this person administers, and the stops banner. */
async function adminStart() {
  adminStop();
  AdminState.standing = await Magnus.api("GET", "/v1/admin/standing").catch(() => null);
  if (AdminState.standing?.admin) sidebar();
  AdminState.poll = setInterval(adminPoll, ADMIN_STOPS_POLL_MS);
  await adminPoll();
}

/** At sign-out: nothing of this person's stays. */
function adminStop() {
  clearInterval(AdminState.poll);
  AdminState.poll = null; AdminState.standing = null; AdminState.stops = [];
  $("#stop-banner")?.remove();
}

/** The kill switch in the admin's top bar: the server's, and each level's. */
async function drawKillControl() {
  const box = $("[data-admin-kill]");
  if (!box) return;
  const server = AdminState.stops.find(s => s.level === "server");
  const more = (await Promise.all(AdminState.levels.map(l => l.active ? l.active().catch(() => []) : []))).flat();
  const release = s => `<button data-kill-release="${esc(s.level)}" data-kill-target="${esc(s.target || "")}">Release${s.level === "server" ? "" : ` ${esc(s.level)} ${esc(s.target || "")}`}</button>`;
  paint(box, `${server ? `<span class="chip bad">Everything is stopped</span>${release(server)}` : `<button class="danger" data-kill-press="server">Stop everything</button>`}
    ${AdminState.levels.map(l => `<button data-kill-press="${esc(l.level)}">${esc(l.label)}</button>`).join("")}
    ${more.map(release).join("")}`);
}

/** Pressing needs no question and no step-up: stopping is always safe.
 * Releasing goes through adminApi, which asks for the password when due.
 * @param {MouseEvent} e */
async function onKillClick(e) {
  const press = /** @type {HTMLElement} */ (e.target).closest("[data-kill-press]");
  const free = /** @type {HTMLElement} */ (e.target).closest("[data-kill-release]");
  try {
    if (press) {
      const level = press.dataset.killPress;
      const l = AdminState.levels.find(x => x.level === level);
      const chosen = level === "server" ? { target: null, name: "everything" } : await l?.choose();
      if (!chosen) return;
      await api("POST", killPath(level, chosen.target, "press"), {});
      toast(level === "server" ? "Everything is stopped." : `Stopped ${chosen.name}.`);
    } else if (free) {
      await adminApi("POST", killPath(free.dataset.killRelease, free.dataset.killTarget || null, "release"), {});
      toast("Released. Stopped tasks stay stopped until each is continued.");
    } else return;
  } catch (err) { if (!err.cancelled) toast(err.message); }
  await adminPoll();
  drawKillControl();
}

/** The Admin view: its top bar (sections and the kill switch) and a section.
 * @param {HTMLElement} main @param {string} [which] */
async function adminView(main, which = "overview") {
  if (!AdminState.standing) AdminState.standing = await api("GET", "/v1/admin/standing");
  if (!AdminState.standing.admin) { paint(main, `<p class="empty">Administration is for the server's owner and admins.</p>`); return; }
  const name = ADMIN_SECTIONS.some(([n]) => n === which) ? which : "overview";
  const label = ADMIN_SECTIONS.find(([n]) => n === name)?.[1];
  paint(main, `<div class="admin" data-admin>
    <header class="admin-bar"><h1>Admin</h1><div class="admin-kill" data-admin-kill aria-label="Kill switch"></div></header>
    <nav class="admin-sub" aria-label="Admin sections">${ADMIN_SECTIONS.map(([n, l]) => `<a href="#/admin/${n}" data-admin-tab="${n}"${n === name ? ' class="on" aria-current="page"' : ""}>${esc(l)}</a>`).join("")}</nav>
    <section class="admin-section" data-admin-section="${name}" aria-label="${esc(label)}"></section>
  </div>`);
  $("[data-admin-kill]", main).addEventListener("click", onKillClick);
  drawKillControl();
  drawStopBanner();
  const section = $("[data-admin-section]", main), render = AdminState.sections.get(name);
  if (render) await render(section);
  else paint(section, `<h2>${esc(label)}</h2><p class="empty">${esc(label)} is not in the app yet.</p>`);
}

route(/^#\/admin(?:\/([a-z]+))?$/, (main, which) => adminView(main, which));
