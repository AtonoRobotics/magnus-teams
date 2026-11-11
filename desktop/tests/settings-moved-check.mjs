// Administration lives in Admin, not Settings (spec rev 141): Settings keeps
// what a person sets for themselves, and the server's models, the
// organisation's connectors, people's roles, server updates, backups and the
// kill switch are Admin's, where each change takes the step-up. Any admin
// change the server holds for the password again opens the step-up sheet
// and is sent again, wherever in the app it is made.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const read = f => readFileSync(join(here, "../ui", f), "utf8");
const app = read("app.js"), spend = read("admin-spend.js"), overview = read("admin-overview.js");
const fast = readFileSync(join(here, "../../.github/workflows/fast.yml"), "utf8");
assert.match(fast, /node desktop\/tests\/settings-moved-check\.mjs/, "the fast tier runs this check");

// Settings has no admin sections, and its old addresses open Admin.
const settings = app.match(/const SETTINGS = \[([\s\S]*?)\n\];/);
assert.ok(settings, "app.js has its SETTINGS list");
for (const gone of ["models", "connectors", "safety"]) assert.ok(!settings[1].includes(`["${gone}"`), `Settings still has its ${gone} section`);
assert.match(app, /const MOVED_TO_ADMIN = \{ models: "#\/admin\/models", connectors: "#\/admin\/connectors", safety: "#\/admin\/overview" \};/);
assert.match(app, /if \(MOVED_TO_ADMIN\[which\]\) \{ location\.hash = administers\(\) \? MOVED_TO_ADMIN\[which\] : "#\/settings"; return; \}/, "an old Settings address does not open Admin");

// No admin change is made from Settings any more.
for (const [what, re] of [
  ["a role", /api\("PUT", `\/v1\/people\/\$\{[^}]+\}\/roles`/],
  ["the server's update", /api\("POST", "\/v1\/updates\/check"/],
  ["a rollback", /api\("POST", "\/v1\/updates\/rollback"/],
  ["the kill switch", /api\("POST", "\/v1\/kill-switch"/],
  ["a backup", /api\("POST", "\/v1\/backups"/],
  ["an organisation's connector", /api\("DELETE", `\/v1\/connectors\/\$\{enc\(app\)\}`\)/],
]) assert.ok(!re.test(app), `Settings still changes ${what}`);
assert.match(app, /People's roles are set in <a href="#\/admin\/people">Admin › People<\/a>\./);

// Connecting model providers moved into Admin › Models, for the owner.
assert.match(spend, /<div data-provider-setup><\/div>/);
assert.match(spend, /if \(setup && owner && typeof modelsSection === "function"\) await modelsSection\(setup, \{ connectOnly: true \}\);/);
// What the kill switch revoked is Overview's.
assert.match(overview, /registerOverviewCard\("kill", async el => \{\s*const ks = await adminApi\("GET", "\/v1\/admin\/kill-switch"\);/);

// api(): a change refused for a step-up asks for the password, then is sent again.
const apiSrc = app.match(/const api = async \(m,p,b\) => \{[\s\S]*?\n\};/)[0];
async function run(stepUpAnswer) {
  const calls = [];
  let first = true;
  const ctx = {
    App: { me: { id: "sam" }, appliance: "a", sessionGeneration: 1, seq: 1 },
    Magnus: { api: async (m, p, b) => { calls.push([m, p]); if (first) { first = false; throw Object.assign(new Error("Confirm your password"), { status: 403, body: { error: "step_up" } }); } return { ok: true }; } },
    stepUp: async () => stepUpAnswer, offerMoreSpend: undefined, Object, Error,
  };
  vm.createContext(ctx);
  vm.runInContext(`${apiSrc}\nglobalThis.api = api;`, ctx);
  try { return { r: await ctx.api("PUT", "/v1/adapters/a", {}), calls }; } catch (e) { return { e, calls }; }
}
const yes = await run(true);
assert.deepEqual(yes.r, { ok: true }, "a change held for a step-up was not sent again");
assert.equal(yes.calls.length, 2);
const no = await run(false);
assert.equal(no.e?.cancelled, true, "a cancelled step-up did not say the change was not made");
assert.equal(no.calls.length, 1, "a cancelled step-up sent the change anyway");

console.log("settings moved check passed");
