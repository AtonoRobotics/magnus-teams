// Connecting an app from a web browser (decisions/2026-11-21-connect-on-the-web.md),
// in the real app served by the server, in a plain browser (no desktop app):
// - Connect on an app goes to the app's own sign-in page in this tab, and
//   its answer comes back to the server's /connected, where the app finishes
//   the sign-in in the person's own session: the app is connected;
// - the answer's code leaves the address at once, and the person is back
//   where they connected from;
// - an answer for another sign-in is refused and connects nothing;
// - an app whose sign-in comes back only to the desktop app says so.
//
// The server is reached at localhost and the app's sign-in at 127.0.0.1, so
// the way back is from another site, as from a real provider: the session
// cookie (SameSite=Strict) is not sent with that page, only with what the
// page then asks.
//
// MAGNUS_URL, MAGNUS_PASSWORD  the server and alice's password
// MAGNUS_CHROMIUM              Chrome to drive
// MAGNUS_APP                   the app to connect (one Magnus registers with)
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const app = process.env.MAGNUS_APP || "calendar";
const browser = await chromium.launch({ executablePath: process.env.MAGNUS_CHROMIUM, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
/** @type {string[]} */
const errors = [];
page.on("pageerror", e => errors.push(e.message));
/** Every address the tab was at, in order. @type {string[]} */
const visited = [];
page.on("request", r => { if (r.isNavigationRequest() && r.frame() === page.mainFrame()) visited.push(r.url()); });
// What the page says, on every page this tab loads.
await page.addInitScript(() => {
  const w = /** @type {any} */ (window);
  w.__said = [];
  new MutationObserver(ms => { for (const m of ms) for (const n of m.addedNodes) { const t = (n.textContent || "").trim(); if (t) w.__said.push(t); } })
    .observe(document, { childList: true, subtree: true, characterData: true });
});
/** @type {string[]} */
const failures = [];
const check = (/** @type {string} */ what, /** @type {() => void} */ fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${/** @type {Error} */ (e).message}`); console.log(`not ok - ${what}`); } };
const until = async (/** @type {() => any} */ fn, what = "UI condition") => {
  const end = Date.now() + 20000;
  while (Date.now() < end) { try { if (await page.evaluate(fn)) return; } catch { /* navigating */ } await page.waitForTimeout(100); }
  throw Error(`${what} timed out`);
};
const state = () => page.evaluate(n => Server.connectors.get(n).then(c => c.state), app);

const signIn = async () => {
  await page.getByLabel("Username").fill("alice");
  await page.getByLabel("Password").fill(process.env.MAGNUS_PASSWORD || "");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.locator("#msg").waitFor();
};

try {
  await page.goto(process.env.MAGNUS_URL || "");
  await signIn();
  await page.evaluate(() => document.querySelectorAll("dialog[open]").forEach(d => /** @type {HTMLDialogElement} */ (d).close()));
  const before = await state();
  check("before: the app is not connected", () => assert.notEqual(before, "connected"));

  // Connect, from Apps.
  await page.evaluate(() => { location.hash = "#/library"; });
  await page.locator(`#main [data-act=connect][data-app="${app}"]`).first().click();
  await page.locator("dialog[open] [data-connect-answer=yes]").click();
  await until(() => location.pathname === "/" && location.hash === "#/library" && document.readyState === "complete" && !!document.querySelector("#main [data-act=connect], #main .lc-card, #main a.lc-more"), "back in the app").catch(async e => {
    const body = await page.evaluate(() => `${location.href}\n${document.body?.innerText?.slice(0, 600)}\n${/** @type {any} */ (window).__said?.slice(-8).join(" | ")}`).catch(x => String(x));
    throw new Error(`${e.message}\nvisited:\n${visited.join("\n")}\nnow: ${body}`);
  });
  await page.waitForTimeout(1500);
  const after = await page.evaluate(n => Server.connectors.get(n).then(c => ({ state: c.state, href: location.href, hash: location.hash, said: /** @type {any} */ (window).__said.join("\n") })), app);
  const went = visited.map(u => new URL(u));
  check("Connect went to the app's own sign-in page in this tab", () => assert.ok(went.some(u => u.pathname === "/authorize"), visited.join("\n")));
  check("the app sent the tab back to the server's /connected", () => assert.ok(went.some(u => u.pathname === "/connected" && u.searchParams.has("code")), visited.join("\n")));
  check("the app is connected", () => assert.equal(after.state, "connected", after.said));
  check("the person is told it is connected, not to use the desktop app", () => assert.ok(/Connected\./.test(after.said) && !/desktop app/.test(after.said), after.said));
  check("the code is out of the address, and the person is back where they connected from", () => assert.deepEqual([/code=|state=|\/connected/.test(after.href), after.hash], [false, "#/library"], after.href));

  // An answer for another sign-in connects nothing.
  await page.evaluate(n => Server.connectors.disconnect(n), app);
  const origin = new URL(process.env.MAGNUS_URL || "").origin;
  await page.goto(`${origin}/connected?code=stolen&state=not-this-one`);
  await page.waitForTimeout(2500);
  const forged = await page.evaluate(n => Server.connectors.get(n).then(c => ({ state: c.state, href: location.href, said: /** @type {any} */ (window).__said.join("\n") })), app);
  check("an answer for another sign-in is refused and connects nothing", () => assert.ok(forged.state !== "connected" && /another request/.test(forged.said) && !/code=/.test(forged.href), JSON.stringify(forged)));
  // An app whose provider sends its sign-in back only to the desktop app
  // says so, and goes nowhere.
  await page.evaluate(() => { location.hash = "#/library"; });
  await page.waitForTimeout(800);
  const only = await page.evaluate(() => (Lib.all || []).find(c => c.signs_in && c.ready !== false && !c.web_sign_in && c.state !== "connected"));
  check("there is an app here whose sign-in comes back only to the desktop app", () => assert.ok(only, "none"));
  if (only) {
    await page.locator(`#main [data-act=connect][data-app="${only.name}"]`).first().click();
    await page.waitForTimeout(800);
    const d = await page.evaluate(() => ({ href: location.href, dialog: !!document.querySelector("dialog[open] [data-connect-answer]"), said: /** @type {any} */ (window).__said.join("\n") }));
    const words = `${only.title} can be connected only from the Magnus desktop app: its sign-in comes back only to the desktop app.`;
    check(`${only.title}, whose sign-in comes back only to the desktop app, says so and goes nowhere`, () => assert.ok(d.said.includes(words) && !d.dialog && d.href.endsWith("#/library"), JSON.stringify({ ...d, said: d.said.slice(-300) })));
  }
  check("no page errors", () => assert.deepEqual(errors, []));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
console.log("connect on the web: all checks passed");
