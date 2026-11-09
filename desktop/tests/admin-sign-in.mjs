// Signing in to the admin console in a real browser (admin console spec,
// Authentication and session; no passkeys, owner's rulings 1 and 2): the
// console's Sign in opens its own form, the person types their password
// there, and comes back to the console signed in, or told the name,
// password or code is wrong. Then, if asked, a step-up: the console asks
// for the password again, and comes back to where it was.
//
// MAGNUS_ADMIN_URL   the console (https://<hostname>:<port>)
// MAGNUS_ADMIN_AT    the console's address (ip:port), for the browser to find its hostname
// MAGNUS_PERSON, MAGNUS_PASSWORD  who signs in (a local console user)
// MAGNUS_EXPECT      "admin" (signed in) or "refused" (not an admin)
// MAGNUS_STEP_UP     optional: "1" to step up after signing in
// MAGNUS_AGAIN       optional: "1" to sign in again in the same browser,
//                    which asks for the password again (no silent entry)
// MAGNUS_VISIT       optional: a screen (e.g. "#/users") to open signed in, which
//                    must render in light, dark and glass, and at phone width
//                    with no sideways scroll (admin console spec, invariant tests, UI)
// MAGNUS_SHELL       optional: "1" to try the patterns every screen uses (admin
//                    console spec, Shell): the admin's menu sets the theme; the
//                    kill switch's banner arrives live; Release opens the step-up
//                    sheet, whose sign-in window comes back to the action; a row
//                    opens its detail panel and Escape closes it; a confirm sheet
//                    cancelled changes nothing; Turn off offers Undo. Needs a
//                    member MAGNUS_OTHER (their name), and the console's step-up
//                    to lapse in MAGNUS_STEP_UP_SECONDS.
// MAGNUS_CHROMIUM    Chrome to drive; MAGNUS_SHOTS  where to keep screenshots
//
// Prints the console's session cookie (signed in) as its last line.
import { createRequire } from "node:module";
import { mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { goto } from "./goto.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, at, person, password, expect] = ["MAGNUS_ADMIN_URL", "MAGNUS_ADMIN_AT", "MAGNUS_PERSON", "MAGNUS_PASSWORD", "MAGNUS_EXPECT"].map(env);
const host = new URL(base).hostname;
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

// The organisation's DNS names the console; its certificate is the test's own.
const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage", `--host-resolver-rules=MAP ${host} ${at.split(":")[0]}`] });
const ctx = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 1280, height: 800 } });
const page = await ctx.newPage();
const errors = [];
page.on("pageerror", e => errors.push(String(e)));
let n = 0;
const shot = async name => { if (shots) await page.screenshot({ path: join(shots, `admin-sign-in-${String(++n).padStart(2, "0")}-${name}.png`), fullPage: true }).catch(() => {}); };
const step = async (name, f) => {
  try { await f(); await shot(name); console.log(`ok   ${name}`); }
  catch (e) { await shot(`FAILED-${name}`); console.error(`FAIL ${name}: ${e.message}`); await browser.close(); process.exit(1); }
};
// A busy build box was slow to answer a sign-in (run 41960251290).
const T = 120_000;
// The console's own form: a username box on sign-in, fixed text naming the
// admin on a step-up, always a password box and a submit button.
const login = async (p = page) => {
  if (await p.locator("input[name=person]").count()) await p.locator("input[name=person]").fill(person);
  await p.locator("input[name=password]").fill(password);
  await Promise.all([p.waitForEvent("load"), p.locator("button[type=submit]").click()]);
};

await step("the console asks for a sign-in", async () => {
  await goto(page, `${base}/`);
  await page.getByRole("link", { name: "Sign in" }).click();
  await page.locator("input[name=person]").waitFor({ timeout: T });
  if (new URL(page.url()).host !== new URL(base).host) throw new Error("the sign-in left the console's page");
});

await step("the password on the console's own form", login);

if (expect === "refused") {
  await step("a wrong name, password or code is refused", async () => {
    await page.getByText("The name, password or code is wrong.").waitFor({ timeout: T });
  });
} else {
  await step("back on the console, signed in", async () => {
    await page.waitForURL(u => new URL(u).host === new URL(base).host, { timeout: T });
    await page.locator("#nav").waitFor({ state: "visible", timeout: T });
    await page.locator("#me").getByText(/./).first().waitFor({ timeout: T });
  });
  if (process.env.MAGNUS_STEP_UP === "1") {
    await step("a step-up asks for the password again, and comes back", async () => {
      await goto(page, `${base}/auth/local/step-up?then=/%23/security`);
      await page.locator("input[name=password]").waitFor({ timeout: T });
      await login();
      await page.waitForURL(u => new URL(u).host === new URL(base).host && u.toString().endsWith("/#/security"), { timeout: T });
    });
  }
  if (process.env.MAGNUS_AGAIN === "1") {
    await step("signing in again asks for the password", async () => {
      // Nothing remembers this browser: without the session cookie the
      // console shows the form again, and the password signs in anew.
      await page.waitForTimeout(2500);
      await ctx.clearCookies({ name: "__Host-magnus_admin" });
      await goto(page, `${base}/auth/local`);
      await page.locator("input[name=person]").waitFor({ timeout: T });
      await login();
      await page.waitForURL(u => new URL(u).host === new URL(base).host, { timeout: T });
      await page.locator("#nav").waitFor({ state: "visible", timeout: T });
    });
  }
}

if (process.env.MAGNUS_SHELL === "1" && expect === "admin") {
  const other = env("MAGNUS_OTHER");
  const lapse = Number(env("MAGNUS_STEP_UP_SECONDS")) * 1000 + 1500;
  const csrf = async () => page.evaluate(async () => (await (await fetch("/api/v1/session")).json()).csrf);
  await step("the admin's menu sets the theme", async () => {
    await goto(page, `${base}/#/overview`);
    await page.locator("#view h1").waitFor({ timeout: T });
    await page.locator("#me-btn").click();
    await page.getByRole("menuitemradio", { name: "Dark" }).click();
    if (await page.evaluate(() => document.documentElement.dataset.mode) !== "dark") throw new Error("Dark did not turn the console dark");
    await page.getByRole("menuitemradio", { name: "Glass" }).click();
    if (await page.evaluate(() => document.documentElement.dataset.material) !== "glass") throw new Error("Glass did not make the chrome glass");
    await page.getByRole("menuitemradio", { name: "Light" }).click();
    await page.getByRole("menuitemradio", { name: "Solid" }).click();
    await page.keyboard.press("Escape");
  });
  await step("the kill switch's banner arrives live", async () => {
    // Engaged from outside this page's own buttons: only the live stream
    // can tell the page.
    const token = await csrf();
    const r = await page.evaluate(async t => (await fetch("/api/v1/kill-switch/engage", { method: "POST", headers: { "x-csrf-token": t } })).status, token);
    if (r !== 200) throw new Error(`engaging answered ${r}`);
    await page.locator("#kill-banner").waitFor({ timeout: 10_000 });
  });
  await step("Release asks for the password in a sheet, and goes on", async () => {
    await page.waitForTimeout(lapse);
    await page.locator("#kill-release").click();
    await page.getByRole("heading", { name: "Confirm it's you" }).waitFor({ timeout: T });
    const [popup] = await Promise.all([page.waitForEvent("popup"), page.locator("#step-go").click()]);
    await popup.locator("input[name=password]").waitFor({ timeout: T });
    // The window closes itself once the console has the password: no load to wait for.
    await popup.locator("input[name=password]").fill(password);
    await popup.locator("button[type=submit]").click();
    await page.locator("#kill-banner").waitFor({ state: "detached", timeout: T });
    await page.getByText("Agents may run again.").waitFor({ timeout: T });
  });
  await step("a row opens its detail panel, and Escape closes it", async () => {
    await goto(page, `${base}/#/users`);
    await page.locator("#users-rows tr.click").first().waitFor({ timeout: T });
    await page.locator("#users-rows tr.click", { hasText: other }).click();
    await page.locator("#panel h2", { hasText: other }).waitFor({ timeout: T });
    await page.locator("#panel").press("Escape");
    await page.locator("#panel").waitFor({ state: "detached", timeout: T });
    if (!page.url().endsWith("#/users")) throw new Error(`closing the panel went to ${page.url()}`);
  });
  await step("a confirm sheet, cancelled, changes nothing", async () => {
    await page.locator("#users-rows tr.click", { hasText: other }).click();
    await page.locator("#u-end").click();
    await page.getByRole("dialog").getByRole("button", { name: "Cancel" }).click();
    await page.getByRole("dialog").waitFor({ state: "detached", timeout: T });
  });
  await step("Turn off offers Undo, and Undo turns them back on", async () => {
    await page.locator("#u-disable").click();
    await page.getByRole("button", { name: "Undo" }).click();
    await page.locator("#u-disable").waitFor({ timeout: T });
  });
}

const visit = process.env.MAGNUS_VISIT;
if (visit && expect === "admin") {
  await step(`${visit} renders in every look and at phone width`, async () => {
    await goto(page, `${base}/${visit}`);
    await page.locator("#view h1").waitFor({ timeout: T });
    // Drawn: nothing on the screen is still loading.
    // (Polled with evaluate: waitForFunction compiles its predicate with
    // eval, which the console's Content-Security-Policy rightly refuses.)
    const drawn = () => page.evaluate(() => !!(document.querySelector("#main") && !document.querySelector('#main [aria-busy="true"]') && document.querySelector("#view h1, #panel h2")));
    for (const end = Date.now() + T; !(await drawn()); await page.waitForTimeout(200)) {
      if (Date.now() > end) throw new Error(`${visit} did not finish loading in ${T / 1000} seconds`);
    }
    for (const [mode, material] of [["light", "solid"], ["dark", "solid"], ["light", "glass"], ["dark", "glass"]]) {
      for (const width of [1280, 390]) {
        await page.setViewportSize({ width, height: 800 });
        await page.evaluate(([mode, material]) => Theme.set({ mode, material }), [mode, material]);
        const over = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
        if (over > 0) throw new Error(`${visit} in ${mode} ${material} at ${width}px scrolls sideways by ${over}px`);
        // The sections stay in reach: the first is on screen at the top.
        await page.evaluate(() => scrollTo(0, 0));
        const top = await page.evaluate(() => document.querySelector("#nav a").getBoundingClientRect().top);
        if (top < 0) throw new Error(`${visit} in ${mode} ${material} at ${width}px starts scrolled: the first section is ${-top}px above the window`);
        await shot(`${visit.replace(/\W/g, "")}-${mode}-${material}-${width}`);
      }
    }
  });
}

if (errors.length) { console.error(`FAIL page errors: ${errors.join("; ")}`); await browser.close(); process.exit(1); }
const cookie = (await ctx.cookies()).find(c => c.name === "__Host-magnus_admin");
await browser.close();
console.log(cookie ? cookie.value : "");
