// The app's card in an approval (connectors spec, The connector card: the
// same card in the Library, in chat, in a task and in approvals): in Needs
// you, the decision for an app's action shows the app's tile and name and,
// once the app has refused the person's sign-in, one button to sign in
// again, without protocol words; in light, dark and glass, and at phone
// width with no sideways scroll.
//
// MAGNUS_URL, MAGNUS_PERSON, MAGNUS_PASSWORD  where and who
// MAGNUS_ITEM, MAGNUS_APP  the approval, and the app whose action it is
// MAGNUS_DECIDES  set when the person signed in decides it for someone
//   else: they see where that person stands with the app, and no button
// MAGNUS_CHROMIUM  Chrome to drive; MAGNUS_SHOTS  where to keep screenshots
import { createRequire } from "node:module";
import { mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, person, password, item, app] = ["MAGNUS_URL", "MAGNUS_PERSON", "MAGNUS_PASSWORD", "MAGNUS_ITEM", "MAGNUS_APP"].map(env);
const decides = !!process.env.MAGNUS_DECIDES;
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
const errors = [];
page.on("pageerror", e => errors.push(String(e)));
let n = 0;
const shot = async name => { if (shots) await page.screenshot({ path: join(shots, `approval-app-card-${String(++n).padStart(2, "0")}-${name}.png`), fullPage: true }).catch(() => {}); };
const step = async (name, f) => {
  try { await f(); await shot(name); console.log(`ok   ${name}`); }
  catch (e) { await shot(`FAILED-${name}`); console.error(`FAIL ${name}: ${e.message}`); await browser.close(); process.exit(1); }
};
const T = 60_000;
// A network port, not a word that starts with one, such as Asana's portfolios.
const PROTOCOL = /\b(oauth|scopes?|tokens?|mcp|ports?\b|https?:)/i;

await step("sign in", async () => {
  await page.goto(base);
  await page.getByLabel("Username").fill(person);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.getByRole("link", { name: /^Needs you/ }).waitFor({ timeout: T });
});

const card = page.locator(`#detail article[data-id="${item}"] .conncard[data-connector="${app}"]`);
await step(decides ? "the approval carries the app's card, saying the person needs to sign in again" : "the approval waiting on another person remains available in Activity", async () => {
  await page.locator("nav a.needs").click();
  if(!decides){
    if(await page.locator(`#detail [data-needs="${item}"]`).count())throw new Error("a non-actionable approval appeared in Needs you");
    await page.getByRole("link",{name:"Activity",exact:true}).click();
    await page.locator('details[data-key="other"] > summary').click();
  }
  await card.waitFor({ timeout: T });
  if ((await card.getAttribute("data-state")) !== "needs_attention") throw new Error(`the card is ${await card.getAttribute("data-state")}, not needs_attention`);
  await card.locator(".ctile").waitFor({ timeout: T });
  if (decides) {
    // Only the person signs in to their own app; the one deciding sees
    // where they stand, and makes the decision.
    await card.getByText("Needs signing in again", { exact: true }).waitFor({ timeout: T });
    if (await card.locator("button").count()) throw new Error("someone else's card offers a button");
    await page.locator(`#detail [data-needs="${item}"] button[data-offer="allow"]`).waitFor({ timeout: T });
  } else {
    if(await page.locator(`#detail article[data-id="${item}"] button[data-offer="allow"]`).count())throw new Error("the requester can decide another person’s approval");
    await card.getByRole("button", { name: /^Sign in to .+ again$/ }).waitFor({ timeout: T });
  }
  const said = await card.innerText();
  if (PROTOCOL.test(said)) throw new Error(`the card shows a protocol word: ${said}`);
});

await step("in every look and at phone width", async () => {
  for (const [mode, material] of ["light","dark"].flatMap(mode=>["solid","glass"].map(material=>[mode,material]))) {
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
