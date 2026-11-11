// Administration in the app (spec rev 141): owners and admins get an Admin
// entry and view with its sections, a kill switch always in its top bar, and
// a step-up sheet when the server asks for the password again (403
// {"error":"step_up"}), after which the change is sent again. Pressing a kill
// switch never asks; releasing goes through the step-up. Everyone affected
// sees a banner naming who pulled the switch, wherever they are in the app.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = join(here, "../ui");
const read = f => readFileSync(join(ui, f), "utf8");
const page = read("index.html"), app = read("app.js"), admin = read("admin.js");
const fast = readFileSync(join(here, "../../.github/workflows/fast.yml"), "utf8");

// Wiring.
const scripts = [...page.matchAll(/<script src="([^"]+)"><\/script>/g)].map(m => m[1]);
assert.ok(scripts.includes("admin.js") && scripts.indexOf("admin.js") > scripts.indexOf("app.js"), "index.html loads admin.js after app.js");
assert.match(page, /<link rel="stylesheet" href="admin\.css">/, "index.html loads admin.css");
assert.match(app, /primary\("teams","Teams","team","#\/teams"\)\}\$\{adminNavItem\(route\)\}<\/div>/, "the navigation has the Admin entry beside the others");
assert.match(app, /\n  shell\(\);\n  adminStart\(\);\n/, "signing in asks whether the person administers, and starts the stops banner");
assert.match(app, /\n  adminStop\(\);\n/, "signing out forgets it");
assert.ok(!/The kill switch is on\./.test(app), "the conversation no longer has a banner of its own that names no one");
assert.match(fast, /node desktop\/tests\/admin-check\.mjs/, "the fast tier runs this check");
assert.match(admin, /api\("POST", "\/v1\/admin\/step-up", \{ password: f\.password, code: f\.code \|\| null \}\)/, "the sheet confirms the password with the server");
assert.match(admin, /error === "code_needed"\) \{ \$\("\[data-step-up-code\]", d\)\.hidden = false;/, "the sheet asks for the code when the server needs it");

// Behaviour, with the page's helpers stubbed.
function harness({ admin: isAdmin = true, stops = [], fail = {} } = {}) {
  const calls = [], toasts = [], routes = [], els = new Map();
  const el = sel => {
    if (!els.has(sel)) els.set(sel, { html: "", hidden: false, listeners: {}, addEventListener(t, f) { this.listeners[t] = f; }, remove() { els.delete(sel); }, setAttribute() {} });
    return els.get(sel);
  };
  const ctx = {
    calls, toasts, routes, els, stepUps: 0, stepUpAnswer: true,
    location: { hash: "#/home" },
    console,
    $: (sel, root) => (root && sel === "[data-admin-section]") || els.has(sel) || root ? el(sel) : null,
    esc: s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`),
    paint: (box, html) => { box.html = html; },
    toast: m => toasts.push(m),
    when: () => "2 min ago",
    icon: k => `<svg data-icon="${k}"></svg>`,
    route: (re, view) => routes.push([re, view]),
    sidebar: () => calls.push(["sidebar"]),
    document: { createElement: () => el("#stop-banner"), body: { append() {}, dataset: {} } },
    setInterval: () => 1, clearInterval: () => {},
  };
  // The app's top bar holds the kill switch's place, hidden until the
  // person is known to administer (app.js `shell`).
  Object.assign(el("[data-admin-kill]"), { hidden: true, dataset: {} });
  const answer = (m, p) => {
    if (p === "/v1/admin/standing") return { admin: isAdmin, owner: false, step_up_seconds: 300, stepped_up_until: null };
    if (p === "/v1/stops") return { stops };
    return { ok: true };
  };
  ctx.api = async (m, p, b) => {
    calls.push([m, p, b]);
    const key = `${m} ${p}`;
    if (fail[key]?.length) { const f = fail[key].shift(); throw Object.assign(new Error(f.error), { status: f.status, body: f }); }
    return answer(m, p);
  };
  ctx.Magnus = { api: ctx.api };
  vm.createContext(ctx);
  vm.runInContext(admin, ctx);
  vm.runInContext("stepUp = async () => { globalThis.stepUps++; return globalThis.stepUpAnswer; }", ctx);
  return ctx;
}

// The entry shows for admins only.
{
  const member = harness({ admin: false });
  await vm.runInContext("adminStart()", member);
  assert.equal(vm.runInContext('adminNavItem("#/home")', member), "", "a member sees the Admin entry");
  const a = harness();
  await vm.runInContext("adminStart()", a);
  const item = vm.runInContext('adminNavItem("#/admin/people")', a);
  assert.match(item, /href="#\/admin"/);
  assert.match(item, /aria-current="page"/, "the entry is current inside Admin");
  assert.ok(a.calls.some(c => c[0] === "sidebar"), "the navigation is drawn again once the person is known to administer");
}

// The kill switch is always in the admin's top bar: the app's own bar, on
// every page, not only inside Admin. A member's stays hidden and empty.
{
  assert.match(app, /<header id="workspace-bar"[^\n]*<div class="admin-kill" data-admin-kill role="group" aria-label="Kill switch" hidden><\/div>/, "the app's top bar has no place for the kill switch");
  const a = harness();
  a.location.hash = "#/chat/new";
  await vm.runInContext("adminStart()", a);
  const bar = a.els.get("[data-admin-kill]");
  assert.equal(bar.hidden, false, "an admin outside Admin has no kill switch in the top bar");
  assert.match(bar.html, /data-kill-press="server">Stop everything/);
  assert.equal(typeof bar.listeners.click, "function", "the top bar's kill switch does nothing when pressed");
  const member = harness({ admin: false });
  await vm.runInContext("adminStart()", member);
  assert.equal(member.els.get("[data-admin-kill]").hidden, true, "a member sees a kill switch");
  assert.equal(member.els.get("[data-admin-kill]").html, "");
  assert.ok(!/data-admin-kill/.test(admin.match(/async function adminView[\s\S]*?\n}\n/)[0].replace("showKillControl", "")), "the Admin view draws a second kill switch of its own");
}

// The view: nine sections, placeholders until a section registers, and the
// kill switch in the top bar.
{
  const a = harness();
  assert.equal(a.routes.length, 1);
  assert.ok(a.routes[0][0].test("#/admin") && a.routes[0][0].test("#/admin/people") && !a.routes[0][0].test("#/admins"));
  await vm.runInContext("adminStart()", a);
  const main = { html: "" };
  await a.routes[0][1](main, "spend");
  for (const name of ["Overview", "People", "Agents", "Models", "Spend", "Connectors", "Rules", "Security", "Operations"]) assert.match(main.html, new RegExp(`>${name}</a>`), `the sub-navigation has ${name}`);
  assert.match(main.html, /href="#\/admin\/spend" data-admin-tab="spend" class="on" aria-current="page"/, "the open section is marked");
  assert.match(a.els.get("[data-admin-section]").html, /Spend is not in the app yet/);
  assert.match(a.els.get("[data-admin-kill]").html, /data-kill-press="server">Stop everything/, "the top bar has the kill switch");
  assert.throws(() => vm.runInContext('registerAdminSection("nope", () => {})', a), /no Admin section named nope/);
  vm.runInContext('registerAdminSection("people", el => { el.html = "the people section"; })', a);
  await a.routes[0][1](main, "people");
  assert.equal(a.els.get("[data-admin-section]").html, "the people section", "a registered section draws itself");
  const member = harness({ admin: false });
  await member.routes[0][1](main, "people");
  assert.match(main.html, /for the server's owner and admins/, "a member typing the address gets no Admin view");
}

// adminApi: a change refused for a step-up asks and is sent again.
{
  const a = harness({ fail: { "PUT /v1/admin/x": [{ status: 403, error: "step_up" }] } });
  const r = await vm.runInContext('adminApi("PUT", "/v1/admin/x", { y: 1 })', a);
  assert.deepEqual(r, { ok: true });
  assert.equal(a.stepUps, 1, "the step-up sheet did not open");
  assert.equal(a.calls.filter(c => c[1] === "/v1/admin/x").length, 2, "the change was not sent again");
  const cancelled = harness({ fail: { "PUT /v1/admin/x": [{ status: 403, error: "step_up" }] } });
  cancelled.stepUpAnswer = false;
  await assert.rejects(vm.runInContext('adminApi("PUT", "/v1/admin/x", {})', cancelled), e => e.cancelled === true);
  assert.equal(cancelled.calls.filter(c => c[1] === "/v1/admin/x").length, 1, "a cancelled step-up sent the change anyway");
  const other = harness({ fail: { "PUT /v1/admin/x": [{ status: 403, error: "this needs an owner or admin" }] } });
  await assert.rejects(vm.runInContext('adminApi("PUT", "/v1/admin/x", {})', other), /owner or admin/);
  assert.equal(other.stepUps, 0, "a plain refusal asked for the password");
}

// The kill switch: pressing asks nothing; releasing steps up.
{
  assert.equal(vm.runInContext('killPath("server", null, "press")', harness()), "/v1/admin/kill-switch/press");
  assert.equal(vm.runInContext('killPath("person", "ada", "release")', harness()), "/v1/admin/kill-switch/person/ada/release");
  const a = harness({ fail: { "POST /v1/admin/kill-switch/release": [{ status: 403, error: "step_up" }] } });
  const click = sel => ({ target: { closest: s => (s === sel ? { dataset: { killPress: "server", killRelease: "server", killTarget: "" } } : null) } });
  await vm.runInContext("onKillClick", a)(click("[data-kill-press]"));
  assert.ok(a.calls.some(c => c[0] === "POST" && c[1] === "/v1/admin/kill-switch/press"), "pressing did not press");
  assert.equal(a.stepUps, 0, "pressing asked for the password");
  await vm.runInContext("onKillClick", a)(click("[data-kill-release]"));
  assert.equal(a.stepUps, 1, "releasing did not ask for the password");
  assert.equal(a.calls.filter(c => c[1] === "/v1/admin/kill-switch/release").length, 2, "the release was not sent again after the step-up");
}

// The banner names who pulled it and when, for everyone it affects.
{
  const stop = { level: "server", target: null, by: "admin:lee", by_name: "Lee", at: "2026-11-11T10:00:00Z" };
  const member = harness({ admin: false, stops: [stop] });
  await vm.runInContext("adminStart()", member);
  const banner = member.els.get("#stop-banner");
  assert.ok(banner, "a member sees no banner while the server is stopped");
  assert.match(banner.html, /<strong>The kill switch is on\.<\/strong> Lee stopped everything on the server 2 min ago\./);
  assert.ok(!/Open Admin/.test(banner.html), "a member is pointed to Admin");
  const a = harness({ stops: [stop] });
  await vm.runInContext("adminStart()", a);
  assert.match(a.els.get("#stop-banner").html, /Open Admin/, "an admin is not pointed to Admin");
  const clear = harness({ stops: [] });
  await vm.runInContext("adminStart()", clear);
  assert.ok(!clear.els.has("#stop-banner"), "a banner shows with nothing stopped");
  assert.match(vm.runInContext('stopText({ level: "person", target: "ada", by_name: "Sam", at: null })', clear), /Sam stopped your agents\./);
  // An admin's banner shows a switch that stops someone else, by whom and when.
  const other = { level: "person", target: "mo", target_name: "Mo", mine: false, by: "admin:lee", by_name: "Lee", at: "2026-11-11T10:00:00Z" };
  const seen = harness({ stops: [other] });
  await vm.runInContext("adminStart()", seen);
  assert.match(seen.els.get("#stop-banner").html, /Lee stopped Mo&#39;s agents 2 min ago\./, "an admin's banner does not name who stopped someone else's agents");
}

// The step-up sheet and every admin form lay out as a column: `.stack` is a
// layout, not the 14px bar it once was, which squashed the whole sheet into
// a strip nobody could type in.
{
  const css = read("app.css");
  const rule = css.match(/(^|\n)\.stack\s*\{([^}]*)\}/);
  assert.ok(rule, "app.css has no .stack rule");
  assert.match(rule[2], /flex-direction:\s*column/, ".stack is not a column");
  assert.doesNotMatch(rule[2], /(^|;)\s*(height|overflow)\s*:/, ".stack fixes a height or hides overflow, so forms using it are cut off");
  assert.match(admin, /<form class="stack" data-step-up>/, "the step-up sheet is not laid out with .stack");
}

console.log("admin check passed");
