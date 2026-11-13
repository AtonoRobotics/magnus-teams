// In a real browser against a real server: a message pops up in the lower
// corner and opens its conversation; the system is told while the app is in
// the background, with no message text; a sender deletes their message; a
// channel is archived and brought back; a direct conversation is deleted
// from one list only; people are added, removed and leave; and the owner
// deletes the workspace after typing its name, which takes it from everyone.
// alice owns workspace MAGNUS_WS; bob is a member with a direct
// conversation MAGNUS_DM; carol is on the server but not in it.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { mkdirSync } from "node:fs";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const { MAGNUS_URL: url, MAGNUS_PASSWORD: pw, MAGNUS_WS: ws, MAGNUS_DM: dm, MAGNUS_SHOTS: shots } = process.env;
const browser = await chromium.launch({ executablePath: process.env.MAGNUS_CHROMIUM, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const errors = [];
const open = async (who, options = {}) => {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, ...options });
  const page = await ctx.newPage();
  page.on("pageerror", e => errors.push(`${who}: ${e.message}`));
  await page.goto(url);
  await page.getByLabel("Username").fill(who);
  await page.getByLabel("Password").fill(pw);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.locator("#msg").waitFor();
  return page;
};
const shot = async (page, name) => { if (shots) { mkdirSync(shots, { recursive: true }); await page.screenshot({ path: join(shots, `${name}.png`) }); } };
const call = (page, m, p, b) => page.evaluate(([m, p, b]) => Magnus.api(m, p, b), [m, p, b]);
const base = `/v1/workspaces/${ws}`;
// The page's CSP forbids eval, which Playwright's polling needs: poll from here.
const until = async (page, fn, arg) => { const end = Date.now() + 15000; while (Date.now() < end) { if (await page.evaluate(fn, arg)) return; await page.waitForTimeout(50); } throw Error(`timed out: ${fn}`); };

try {
  const alice = await open("alice", { reducedMotion: "reduce", permissions: ["notifications"] });
  const bob = await open("bob");

  // A message to alice pops up in the lower right corner, wherever she is.
  await call(bob, "POST", `${base}/channels/${dm}/messages`, { text: "Can you look at the draft?" });
  const notice = alice.locator("#notices .notice").first();
  await notice.waitFor({ timeout: 15000 });
  assert.equal(await notice.getAttribute("role"), "status");
  assert.match(await notice.innerText(), /bob[^]*Can you look at the draft\?/i);
  const box = await notice.boundingBox();
  assert.ok(box.x + box.width > 1280 - 40 && box.y + box.height > 800 - 40, `the pop-up is not in the lower right: ${JSON.stringify(box)}`);
  assert.equal(await notice.evaluate(e => getComputedStyle(e).animationName), "none", "a pop-up moves for someone who asked for less motion");
  await shot(alice, "team-own-popup");
  await notice.locator(".notice-open").click();
  await until(alice, dm => location.hash.includes(`/c/${dm}`), dm);
  assert.equal(await alice.locator("#notices .notice").count(), 0, "the pop-up stayed after it was opened");

  // In the background, the system is told who and where, never what.
  await alice.evaluate(() => {
    window.__told = [];
    window.Notification = class { constructor(t, o) { window.__told.push([t, o.body]); } static permission = "granted"; };
    document.hasFocus = () => false;
    location.hash = "#/inbox";
  });
  await call(bob, "POST", `${base}/channels/${dm}/messages`, { text: "the code is 4471" });
  await until(alice, () => window.__told.length > 0);
  assert.deepEqual(await alice.evaluate(() => window.__told), [["bob", "sent you a message"]]);
  await alice.evaluate(() => { document.hasFocus = () => true; });

  // alice deletes her own message: gone for bob too.
  await alice.evaluate(h => { location.hash = h; }, `#/team/${ws}/c/${dm}`);
  // A channel is the personal chat's view (chat-view.js): its composer and list.
  await alice.locator("#msg").fill("sent to the wrong person");
  await alice.locator("#msg").press("Enter");
  const mine = alice.locator("#thread .entry.me", { hasText: "sent to the wrong person" });
  await mine.waitFor();
  await mine.hover();
  // Delete sits in the message's menu, as in the personal chat (message-controls.js).
  await mine.getByRole("button", { name: "More message actions", exact: true }).click();
  await mine.getByRole("menuitem", { name: "Delete message", exact: true }).click();
  await alice.getByRole("dialog").getByRole("button", { name: "Delete", exact: true }).click();
  await mine.waitFor({ state: "detached" });
  // It leaves alice's list at once and the server behind: wait for that.
  let bobs;
  for (let i = 0; i < 50; i++) {
    bobs = await call(bob, "GET", `${base}/channels/${dm}/view`);
    if (!JSON.stringify(bobs).includes("wrong person")) break;
    await new Promise(r => setTimeout(r, 200));
  }
  assert.ok(!JSON.stringify(bobs).includes("wrong person"), "bob still sees the deleted message");

  // The conversation deleted from alice's list only.
  await alice.getByRole("button", { name: "Delete conversation", exact: true }).click();
  await alice.getByRole("dialog").getByRole("button", { name: "Delete", exact: true }).click();
  await until(alice, dm => !location.hash.includes(dm), dm);
  assert.equal(await alice.locator(`#team-rail [data-channel="${dm}"]`).count(), 0, "the deleted conversation is still listed");
  const kept = await call(bob, "GET", `${base}/channels/${dm}/view`);
  assert.ok(JSON.stringify(kept).includes("look at the draft"), "bob lost his copy");

  // A channel archived, found under Archived, and brought back.
  const home = await call(alice, "GET", `${base}/home`);
  const general = home.channels.find(c => c.name === "general").id;
  await alice.evaluate(h => { location.hash = h; }, `#/team/${ws}/c/${general}`);
  await alice.getByRole("button", { name: "Archive", exact: true }).click();
  await alice.locator(".team-archived summary").waitFor();
  assert.equal(await alice.locator(`#team-rail nav[aria-label="Channels"] [data-channel="${general}"]`).count(), 0, "an archived channel is still listed");
  await alice.locator(".team-archived summary").click();
  await alice.locator(`.team-archived [data-channel="${general}"]`).click();
  await alice.getByRole("button", { name: "Unarchive", exact: true }).click();
  await until(alice, () => !document.querySelector(".team-archived"));

  // People: carol is added by name, bob removed (his app leaves the
  // workspace at once), then added back, then he leaves on his own.
  await alice.evaluate(h => { location.hash = h; }, `#/team/${ws}/people`);
  await alice.locator("[data-add-person]").selectOption("carol");
  await alice.getByRole("button", { name: "Add", exact: true }).click();
  await alice.locator('[data-member="carol"]').waitFor();
  await bob.evaluate(h => { location.hash = h; }, `#/team/${ws}`);
  await bob.locator("#team-rail").waitFor();
  await alice.locator('[data-member="bob"] [data-remove]').click();
  await alice.getByRole("dialog").getByRole("button", { name: "Remove", exact: true }).click();
  await until(bob, () => location.hash === "#/teams");
  await alice.locator("[data-add-person]").selectOption("bob");
  await alice.getByRole("button", { name: "Add", exact: true }).click();
  await alice.locator('[data-member="bob"]').waitFor();
  await bob.evaluate(h => { location.hash = h; }, `#/team/${ws}/people`);
  await bob.locator("[data-leave]").click();
  await bob.getByRole("dialog").getByRole("button", { name: "Leave", exact: true }).click();
  await until(bob, () => location.hash === "#/teams");
  assert.deepEqual(await call(bob, "GET", "/v1/workspaces"), [], "bob is still in the workspace he left");
  await shot(alice, "team-own-people");

  // The owner deletes the workspace once its name is typed.
  await alice.reload();
  await alice.locator("[data-delete-workspace]").click();
  const sure = alice.getByRole("dialog", { name: "Delete workspace" });
  const go = sure.getByRole("button", { name: "Delete workspace", exact: true });
  assert.ok(await go.isDisabled(), "the workspace can be deleted without typing its name");
  await sure.locator("[data-typed]").fill("Launch");
  await shot(alice, "team-own-delete");
  await go.click();
  await until(alice, () => location.hash === "#/teams");
  assert.deepEqual(await call(alice, "GET", "/v1/workspaces"), [], "the deleted workspace is still listed");

  assert.deepEqual(errors, [], "the page threw");
  console.log("team own app ok");
} finally {
  await browser.close();
}
