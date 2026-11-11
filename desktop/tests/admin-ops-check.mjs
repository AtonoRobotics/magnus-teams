// Admin › Operations (spec rev 141): updates, backups with their restore
// check, computers against capacity, and health; the server's own update
// channel beside the desktop apps'.
import assert from "node:assert/strict";
import { wired, load, draw, node, same } from "./admin-harness.mjs";

wired("admin-ops.js", "admin-ops-check.mjs");

const updates = { version: "1.2.3", automatic: false, latest: { version: "1.3.0", notes: null }, available: true, updating: null, last: { what: "failed", to: "1.2.9", detail: "health check", at: null }, previous: "1.2.2",
  desktop: { channels: [{ channel: "stable", published: "1.2.3", who: "everyone" }, { channel: "beta", published: null, who: "testers" }], everyone: "stable" },
  server: { channel: "test", channels: [{ channel: "development", published: null }, { channel: "test", published: "1.3.0-test.2" }, { channel: "production", published: "1.3.0" }] } };
const backups = { every_hours: 6, keep_days: 14, copy_to: null, latest: { file: "b1", restored: false, detail: "checksum mismatch", at: null }, copy: null, files: [{ file: "b1", bytes: 10 }], scrub_pending: null };
const computers = { computers: [{ id: "c1", person: "Mo", workspace: null, status: "running", controller: "Sam" }], capacity: { set_up: true, answers: true, live: 1, max_live: 4, computers: 1, max_computers: 10 } };
const health = { services: [{ name: "database", up: true }, { name: "computers", up: false, note: "refused" }], host: { cpus: 8, load: 0.5, memory_used: 0.42, disk_used: 0.9 } };
const ctx = load("admin-ops.js", { "GET /v1/admin/updates": updates, "GET /v1/admin/backups": backups, "GET /v1/admin/computers": computers, "GET /v1/admin/health": health });
const el = await draw(ctx, "operations");
assert.match(el.html, /Install 1\.3\.0/);
assert.match(el.html, /Roll back to 1\.2\.2/);
assert.match(el.html, /class="small bad">Last: failed 1\.2\.9 .*: health check/);
assert.match(el.html, /<option value="stable" selected>stable \(1\.2\.3\)/);
// The server's own channel, beside the desktop apps'.
assert.match(el.html, /This server updates from <select data-ops-server-channel aria-label="Server channel"><option value="development">development<\/option><option value="test" selected>test \(1\.3\.0-test\.2\)/);
assert.match(el.html, /Every 6 hours, each kept 14 days, on this server's disk only\./);
assert.match(el.html, /data-ops-restore-check class="bad">The latest backup did not restore .*: checksum mismatch/, "a failed restore check is not shown as failed");
assert.match(el.html, /1 of 4 running; 1 of 10 exist\./);
assert.match(el.html, /running, Sam has the screen/);
assert.match(el.html, /<span class="chip bad">Down<\/span> computers <span class="muted">refused/);
assert.match(el.html, /8 CPUs, load 0\.5 · memory 42% used · disk 90% used/);

await el.onclick({ target: node({ "data-ops-backup": "" }) });
assert.ok(ctx.calls.some(c => c[0] === "POST" && c[1] === "/v1/admin/backups"));
await el.onclick({ target: node({ "data-ops-install": "" }) });
assert.ok(ctx.calls.some(c => c[0] === "POST" && c[1] === "/v1/admin/updates/install"));
ctx.confirm = () => false;
await el.onclick({ target: node({ "data-ops-rollback": "" }) });
assert.ok(!ctx.calls.some(c => c[1] === "/v1/admin/updates/rollback"), "rolled back without asking");
await el.onchange({ target: node({ "data-ops-channel": "", value: "beta" }) });
same(ctx.calls.find(c => c[0] === "PUT"), ["PUT", "/v1/admin/updates/desktop-channel", { channel: "beta" }]);
await el.onchange({ target: node({ "data-ops-server-channel": "", value: "production" }) });
same(ctx.calls.filter(c => c[0] === "PUT").at(-1), ["PUT", "/v1/admin/updates/server-channel", { channel: "production" }]);

// A passed restore check says so.
const ok = load("admin-ops.js", { "GET /v1/admin/updates": updates, "GET /v1/admin/backups": { ...backups, latest: { file: "b2", restored: true, detail: "", at: null } }, "GET /v1/admin/computers": computers, "GET /v1/admin/health": health });
assert.match((await draw(ok, "operations")).html, /data-ops-restore-check class="">The latest backup restored in its check/);
assert.doesNotMatch(el.html, /data-ops-computerd-behind/, "a computer service that is not behind was shown as behind");

// A computer service older than the server says so, with the command that updates it.
const command = "sudo sh /home/sam/.magnus-teams/bin/install-computerd.sh /home/sam/.magnus-teams/bin/magnus-computerd /v /r sam /s/updates/appliance";
const behind = { ...computers, capacity: { ...computers.capacity, service: { checked: true, behind: true, version: null, expected: "0.2.6-test.81",
  message: "The computer service is older than this server: computers may fail to start or reset until it is updated.", command } } };
const old = load("admin-ops.js", { "GET /v1/admin/updates": updates, "GET /v1/admin/backups": backups, "GET /v1/admin/computers": behind, "GET /v1/admin/health": health });
const oldHtml = (await draw(old, "operations")).html;
assert.match(oldHtml, /The computer service is older than this server: computers may fail to start or reset until it is updated\./);
assert.match(oldHtml, /It runs a release from before it could say; this server is 0\.2\.6-test\.81\./);
assert.ok(oldHtml.includes(`<pre class="small mono">${command}</pre>`), "the command is not shown");
console.log("admin ops check passed");
