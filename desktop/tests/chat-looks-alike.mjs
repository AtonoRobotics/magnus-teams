// In a real browser against a real server: the same request in the personal
// chat and in a team channel is drawn by the one chat view. Each shows the
// person's message, the agent at work, then its answer, in the same markup
// and styles, with the same composer; screenshots of both are kept in
// MAGNUS_SHOTS to compare. alice owns workspace MAGNUS_WS.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { mkdirSync } from "node:fs";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const { MAGNUS_URL: url, MAGNUS_PASSWORD: pw, MAGNUS_WS: ws, MAGNUS_SHOTS: shots } = process.env;
const browser = await chromium.launch({ executablePath: process.env.MAGNUS_CHROMIUM, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const errors = [];
const shot = async (page, name) => { if (shots) { mkdirSync(shots, { recursive: true }); await page.screenshot({ path: join(shots, `${name}.png`) }); } };
// The page's CSP forbids eval, which Playwright's polling needs: poll from here.
const until = async (page, fn, arg) => { const end = Date.now() + 30000; while (Date.now() < end) { if (await page.evaluate(fn, arg)) return; await page.waitForTimeout(100); } throw Error(`timed out: ${fn}`); };

// What a drawn conversation looks like: the elements and the styles that
// make a chat look as it does.
const looks = page => page.evaluate(() => {
  const css = q => { const el = document.querySelector(q); if (!el) return null; const s = getComputedStyle(el); return ["font-size", "font-family", "line-height", "background-color", "border-radius", "padding-top", "padding-left", "color", "align-self", "max-width"].map(k => `${k}:${s.getPropertyValue(k)}`).join(";"); };
  return {
    mine: css("#thread .entry.me > .msg.me"), byline: css("#thread .entry.me .byline"), answer: css("#thread .msg.magnus .answer-body"),
    composer: css("#say"), box: css("#say textarea"), send: css("#say .send"), attach: css("#say [data-attach-open]"), actions: css("#thread .entry.them .msg-actions"),
  };
});

try {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, reducedMotion: "reduce" });
  const page = await ctx.newPage();
  page.on("pageerror", e => errors.push(e.message));
  await page.goto(url);
  await page.getByLabel("Username").fill("alice");
  await page.getByLabel("Password").fill(pw);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.locator("#msg").waitFor();

  // The personal chat.
  await page.evaluate(() => { location.hash = "#/chat/new"; });
  await page.locator("#msg").fill("Plan the launch");
  await page.locator("#msg").press("Enter");
  await page.locator("#thread .entry.me", { hasText: "Plan the launch" }).waitFor({ timeout: 30000 });
  await shot(page, "1-personal-working");
  await page.locator("#thread .msg.magnus .answer-body", { hasText: "three" }).waitFor({ timeout: 30000 });
  await page.mouse.move(0, 0);
  await shot(page, "2-personal-answered");
  const personal = await looks(page);

  // A team channel: the same request, asking the team agent by name.
  const home = await page.evaluate(w => Magnus.api("GET", `/v1/workspaces/${w}/home`), ws);
  const general = home.channels.find(c => c.name === "general").id;
  await page.evaluate(h => { location.hash = h; }, `#/team/${ws}/c/${general}`);
  await page.locator("#team-center #msg").waitFor();
  await page.locator("#msg").fill("@Magnus Plan the launch");
  await page.locator("#msg").press("Enter");
  await page.locator("#thread .entry.me", { hasText: "Plan the launch" }).waitFor({ timeout: 30000 });
  await until(page, () => !!document.querySelector("#thread [data-chat-work], #thread .msg.magnus"));
  await shot(page, "3-team-working");
  await page.locator("#thread .msg.magnus .answer-body", { hasText: "three" }).waitFor({ timeout: 30000 });
  await page.mouse.move(0, 0);
  await shot(page, "4-team-answered");
  const team = await looks(page);

  for (const k of Object.keys(personal)) {
    assert.ok(personal[k], `the personal chat has no ${k}`);
    assert.equal(team[k], personal[k], `a team channel's ${k} looks different from the personal chat's`);
  }
  // One composer, one list: the team channel has no other.
  assert.equal(await page.locator("#team-center form").count(), 1, "a team channel has a second composer");
  assert.deepEqual(errors, [], "the page threw");
  console.log("chat looks alike ok");
} finally {
  await browser.close();
}
