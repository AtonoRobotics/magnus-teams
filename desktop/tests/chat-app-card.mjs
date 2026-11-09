// An app Magnus offered in a conversation (connectors spec, Inside work;
// the catalog mockup's "Connecting from chat"): the same card as in the
// Library, with Magnus's reason, the app's tile and name, and one Connect
// button, without protocol words; in light, dark and glass, and at phone
// width with no sideways scroll.
//
// MAGNUS_URL, MAGNUS_PERSON, MAGNUS_PASSWORD  where and who
// MAGNUS_THREAD, MAGNUS_APP, MAGNUS_WHY  the conversation, the app offered and why
// MAGNUS_CHROMIUM  Chrome to drive; MAGNUS_SHOTS  where to keep screenshots
import { createRequire } from "node:module";
import { mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, person, password, thread, app, why] = ["MAGNUS_URL", "MAGNUS_PERSON", "MAGNUS_PASSWORD", "MAGNUS_THREAD", "MAGNUS_APP", "MAGNUS_WHY"].map(env);
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
const errors = [];
page.on("pageerror", e => errors.push(String(e)));
let n = 0;
const shot = async name => { if (shots) await page.screenshot({ path: join(shots, `chat-app-card-${String(++n).padStart(2, "0")}-${name}.png`), fullPage: true }).catch(() => {}); };
const step = async (name, f) => {
  try { await f(); await shot(name); console.log(`ok   ${name}`); }
  catch (e) { await shot(`FAILED-${name}`); console.error(`FAIL ${name}: ${e.message}`); await browser.close(); process.exit(1); }
};
const T = 60_000;
const PROTOCOL = /\b(oauth|scopes?|tokens?|mcp|port|https?:)/i;

await step("sign in", async () => {
  await page.goto(base);
  await page.getByLabel("Username").fill(person);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.getByRole("link", { name: /^Needs you/ }).waitFor({ timeout: T });
});

const card = page.locator(`.conncard[data-connector="${app}"]`);
await step("the offered app's card is in the conversation, with why and Connect", async () => {
  await page.evaluate(t => { location.hash = `#/chat/${t}`; }, thread);
  await card.waitFor({ timeout: T });
  if ((await card.getAttribute("data-state")) !== "available") throw new Error(`the card is ${await card.getAttribute("data-state")}, not available`);
  await card.getByText(why, { exact: true }).waitFor({ timeout: T });
  await card.getByRole("button", { name: /^Connect / }).waitFor({ timeout: T });
  await card.locator(".ctile").waitFor({ timeout: T });
  const said = await card.innerText();
  if (PROTOCOL.test(said)) throw new Error(`the card shows a protocol word: ${said}`);
});

await step("in every look and at phone width", async () => {
  for (const [mode, material] of [["light", "solid"], ["dark", "solid"], ["light", "glass"], ["dark", "glass"]]) {
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 800 });
      await page.evaluate(([mode, material]) => Theme.set({ mode, material }), [mode, material]);
      await card.waitFor({ timeout: T });
      const over = await page.evaluate(() => Math.max(document.documentElement.scrollWidth - document.documentElement.clientWidth, ...[...document.querySelectorAll("main")].map(m => m.scrollWidth - m.clientWidth)));
      if (over > 0) throw new Error(`${mode} ${material} at ${width}px scrolls sideways by ${over}px`);
    }
  }
  await page.evaluate(() => Theme.set({ mode: "system", material: "solid" }));
});

if (errors.length) { console.error(`FAIL page errors: ${errors.join("; ")}`); process.exit(1); }
await browser.close();
