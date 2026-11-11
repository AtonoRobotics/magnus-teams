// Admin › Overview (spec rev 141): what is running, computers against
// capacity, what needs an admin linked to the section that fixes it, and the
// hook other sections (Spend) use to add their card.
import assert from "node:assert/strict";
import vm from "node:vm";
import { wired, load, draw } from "./admin-harness.mjs";

wired("admin-overview.js", "admin-overview-check.mjs");

const overview = {
  version: "1.2.3", running: { tasks: 4, waiting: 1, signed_in: 3, computers: 2 },
  computers: { set_up: true, answers: true, live: 2, max_live: 6, computers: 5, max_computers: 20 },
  people: { total: 7, invited: 2 }, kill_switch: false,
  recent_errors: [{ run: "r1", task: "t1", person: "Mo", reason: "timed out", at: null }],
  alerts: [{ what: "A backup has not run in two days.", fix: "operations" }, { what: "GitHub needs its credential.", fix: "connectors" }],
  spend: null,
};
let ctx = load("admin-overview.js", { "GET /v1/admin/overview": overview });
let el = await draw(ctx, "overview");
assert.match(el.html, /<strong>4<\/strong> tasks running/);
assert.match(el.html, /1 waiting · 3 people signed in · 7 people, 2 invited/);
assert.match(el.html, /<strong data-overview-live>2<\/strong> of 6 running/);
assert.match(el.html, /5 of 20 computers exist/);
assert.match(el.html, /A backup has not run in two days\.<\/span> <a href="#\/admin\/operations">Open /);
assert.match(el.html, /href="#\/admin\/connectors"/);
assert.match(el.html, /timed out/);
assert.match(el.html, /Spend against its limits is in <a href="#\/admin\/spend">Spend<\/a>/);

// Nothing to do, and the computer service down: both said.
ctx = load("admin-overview.js", { "GET /v1/admin/overview": { ...overview, alerts: [], recent_errors: [], computers: { set_up: true, answers: false, error: "refused" } } });
el = await draw(ctx, "overview");
assert.match(el.html, /Nothing needs an admin now\./);
assert.match(el.html, /The computer service is not answering: refused\./);

// The hook: a registered spend card replaces the pointer and is drawn.
ctx = load("admin-overview.js", { "GET /v1/admin/overview": overview });
vm.runInContext(`registerOverviewCard("spend", box => { box.html = "spent 12"; });`, ctx);
el = await draw(ctx, "overview");
assert.doesNotMatch(el.html, /data-overview-spend/);
assert.match(el.html, /data-overview-card="spend"/);
assert.equal(ctx.$('[data-overview-card="spend"]').html, "spent 12");

// A card that fails says so instead of breaking Overview.
ctx = load("admin-overview.js", { "GET /v1/admin/overview": overview });
vm.runInContext(`registerOverviewCard("stops", async () => { throw new Error("no stops"); });`, ctx);
await draw(ctx, "overview");
assert.match(ctx.$('[data-overview-card="stops"]').html, /no stops/);
console.log("admin overview check passed");
