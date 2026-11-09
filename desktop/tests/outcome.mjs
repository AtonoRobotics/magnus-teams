// The outcome page shows the deliverable in full, first, without opening a
// file or the trace: each day with its places, times and costs, drawn from
// the fixed components; a delivered file the agent did not place is shown
// too. Then the summary, the evidence, the cost, and the trace, folded. A
// verdict tapped on it (right, partly, wrong), and its note, are kept.
//
// MAGNUS_URL, MAGNUS_PERSON, MAGNUS_PASSWORD  where and who
// MAGNUS_TASK      the task to look at
// MAGNUS_CHROMIUM  Chrome to drive; MAGNUS_SHOTS  where to keep screenshots
import { createRequire } from "node:module";
import { mkdirSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, person, password, task] = ["MAGNUS_URL", "MAGNUS_PERSON", "MAGNUS_PASSWORD", "MAGNUS_TASK"].map(env);
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const win = JSON.parse(readFileSync(join(root, "desktop/src-tauri/tauri.conf.json"), "utf8")).app.windows[0];

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: win.width, height: win.height } });
const errors = [];
page.on("pageerror", e => errors.push(String(e)));
const T = 60_000;
const step = async (name, f) => {
  try { await f(); if (shots) await page.screenshot({ path: join(shots, `outcome-${name.replace(/\W+/g, "-")}.png`), fullPage: true }).catch(e => console.error(`screenshot not kept: ${e.message.split("\n")[0]}`)); console.log(`ok   ${name}`); }
  catch (e) { if (shots) await page.screenshot({ path: join(shots, "outcome-FAILED.png"), fullPage: true }).catch(e => console.error(`screenshot not kept: ${e.message.split("\n")[0]}`)); console.error(`FAIL ${name}: ${e.message}`); await browser.close(); process.exit(1); }
};

await step("sign in", async () => {
  await page.goto(base);
  await page.getByLabel("Username").fill(person);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  // Signed in: the workspace, with Needs you in the sidebar.
  await page.getByRole("link", { name: /^Needs you/ }).waitFor({ timeout: T });
});

await step("the itinerary is on the page in full", async () => {
  // A task's link opens it beside the conversation.
  await page.goto(`${base}/#/task/${task}`);
  await page.locator("#detail .detail-head .chip", { hasText: "Done" }).waitFor({ timeout: T });
  const frame = page.frameLocator("#detail iframe.composed");
  for (const s of ["Day 1: Balboa Park", "Day 2: The harbor", "Day 3: La Jolla Cove", "13:00 Check in at Mission View Inn, 641 Camino del Rio S ($67 a night)", "12:30 Lunch at Tacos El Gordo ($28)", "8:30 Sea lions at La Jolla Cove (free)", "$914"]) {
    await frame.getByText(s, { exact: false }).first().waitFor({ timeout: T });
  }
  // Its cost table is a table; the CSV it delivered but did not place is
  // shown as one too.
  await frame.locator('[data-file="San Diego itinerary.md"] table').getByRole("cell", { name: "Lodging", exact: true }).waitFor({ timeout: T });
  await frame.locator('[data-file="costs.csv"] td', { hasText: "food" }).waitFor({ timeout: T });
  // Nothing was opened to see it: no file, and the steps stay behind their tab.
  if (await page.locator("#detail .call").count()) throw new Error("the steps are shown");
  if (await page.getByRole("tab", { name: "Result" }).getAttribute("aria-selected") !== "true") throw new Error("not on the result");
});

await step("then the summary, evidence and cost; the steps a tab away", async () => {
  const order = await page.evaluate(() => ["iframe.composed", "[data-summary]", "[data-done-line]", "[data-cost]"].map(q => {
    const el = document.querySelector(`#detail ${q}`);
    return el ? el.getBoundingClientRect().top : null;
  }));
  if (order.some(y => y === null) || order.some((y, i) => i && y <= order[i - 1])) throw new Error(`out of order (deliverable, summary, evidence, cost): ${order}`);
  await page.locator("#detail [data-summary]", { hasText: "Nothing was booked." }).waitFor({ timeout: T });
  await page.getByRole("tab", { name: "Steps" }).click();
  await page.locator("#detail .call").first().waitFor({ timeout: T });
  await page.getByRole("tab", { name: "Result" }).click();
});

await step("a one-tap verdict with a note is kept", async () => {
  const bar = () => page.locator(`[data-verdict-for="${task}"]`);
  await page.locator("[data-work-feedback] > summary").click();
  await bar().getByRole("button", { name: "Partly", exact: true }).click();
  await page.getByText("Recorded.").waitFor({ timeout: T });
  await bar().locator("details.vnote > summary").click();
  await bar().getByLabel("Note").fill("Lodging is far from the coast.");
  await bar().getByRole("button", { name: "Save note" }).click();
  await page.getByText("Note saved.").waitFor({ timeout: T });
  // Opened again from its link after a restart of the page, it is kept.
  await page.reload();
  await page.goto(`${base}/#/task/${task}`);
  await page.locator("[data-work-feedback] > summary").click();
  await bar().locator('button[data-verdict="partly"][aria-pressed="true"]').waitFor({ timeout: T });
  const note = await bar().getByLabel("Note").inputValue();
  if (note !== "Lodging is far from the coast.") throw new Error(`the note reads "${note}" after a reload`);
});

if (errors.length) { console.error(`page errors: ${errors.join("; ")}`); process.exit(1); }
await browser.close();
