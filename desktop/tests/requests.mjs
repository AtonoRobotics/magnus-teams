// A request set compared across models, in a real browser: the Experiments
// view lists each request against each model with its choice, stops, cost
// and time; a rerun shows one summary against the run before it (improved,
// unchanged, regressed); a verdict the person taps, and its note, are kept.
//
// MAGNUS_URL, MAGNUS_PERSON, MAGNUS_PASSWORD  where and who (a developer)
// MAGNUS_EXPERIMENT  a rerun, with one request left to judge; MAGNUS_MODELS  how many models ran it
// MAGNUS_CHROMIUM  Chrome to drive; MAGNUS_SHOTS  where to keep screenshots
import { createRequire } from "node:module";
import { mkdirSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, person, password, experiment, models] = ["MAGNUS_URL", "MAGNUS_PERSON", "MAGNUS_PASSWORD", "MAGNUS_EXPERIMENT", "MAGNUS_MODELS"].map(env);
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const win = JSON.parse(readFileSync(join(root, "desktop/src-tauri/tauri.conf.json"), "utf8")).app.windows[0];

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: win.width, height: win.height } });
const errors = [];
page.on("pageerror", e => errors.push(String(e)));
let n = 0;
const shot = async name => { if (shots) await page.screenshot({ path: join(shots, `requests-${String(++n).padStart(2, "0")}-${name}.png`), fullPage: true }).catch(e => console.error(`screenshot not kept: ${e.message.split("\n")[0]}`)); };
const step = async (name, f) => {
  try { await f(); await shot(name); console.log(`ok   ${name}`); }
  catch (e) { await shot(`FAILED-${name}`); console.error(`FAIL ${name}: ${e.message}`); await browser.close(); process.exit(1); }
};
const T = 60_000;

await step("sign in", async () => {
  await page.goto(base);
  await page.getByLabel("Username").fill(person);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  // Signed in: the workspace, with Needs you in the sidebar.
  await page.getByRole("link", { name: /^Needs you/ }).waitFor({ timeout: T });
});

await step("each request against each model", async () => {
  await page.goto(`${base}/#/dev/experiment/${experiment}`);
  await page.locator("table[data-summary] tbody tr").first().waitFor({ timeout: T });
  const summary = await page.locator("table[data-summary] tbody tr").count();
  if (summary !== Number(models)) throw new Error(`${summary} models in the summary, not ${models}`);
  const cards = page.locator("[data-case]");
  for (let i = 0; i < await cards.count(); i++) {
    const rows = await cards.nth(i).locator("tr[data-request]").count();
    if (rows !== Number(models)) throw new Error(`request ${i + 1} shows ${rows} models`);
  }
  await page.locator("[data-case]", { hasText: "Email Bob the agenda." }).getByText("Started tasks").first().waitFor({ timeout: T });
  await page.locator("[data-case]", { hasText: "Every Monday" }).getByText("Proposed a schedule").first().waitFor({ timeout: T });
});

const compare = () => page.evaluate(async x => (await (await fetch(`/v1/dev/experiments/${x}`)).json()).compare, experiment);
const shown = async () => Object.fromEntries(await Promise.all(["improved", "unchanged", "regressed"].map(async k => [k, Number(await page.locator(`[data-compare-summary] [data-${k}]`).innerText())])));

await step("the rerun against the run before", async () => {
  await page.locator("[data-compare-summary]").waitFor({ timeout: T });
  const c = await compare();
  const ui = await shown();
  for (const k of Object.keys(ui)) if (ui[k] !== c.summary[k]) throw new Error(`${k}: the page shows ${ui[k]}, the log has ${c.summary[k]}`);
  if (!c.summary.regressed) throw new Error("the rerun has no regression to show first");
  const first = page.locator("table[data-compare] tr[data-pair]").first();
  if (await first.getAttribute("data-outcome") !== "regressed") throw new Error("a regression is not listed first");
  await first.locator('[data-changed="verdict"]').waitFor({ timeout: T });
  await page.getByText(`${c.summary.unjudged} not judged yet`).waitFor({ timeout: T });
});

// The request left to judge.
const open = async () => {
  const c = await compare();
  const p = c.pairs.find(p => p.outcome === "unjudged");
  if (!p) throw new Error("no request is left to judge");
  return { c, row: page.locator(`[data-case="${p.case}"] tr[data-request]`).filter({ hasText: p.model }) };
};

await step("a verdict is kept and the summary counts it", async () => {
  const { c, row } = await open();
  await row.getByRole("button", { name: "Right", exact: true }).click();
  await row.locator('button[data-verdict="right"][aria-pressed="true"]').waitFor({ timeout: T });
  const after = await compare();
  if (after.summary.unjudged !== c.summary.unjudged - 1) throw new Error(`still ${after.summary.unjudged} to judge`);
  await page.locator(`[data-compare-summary] [data-unchanged]`).filter({ hasText: String(after.summary.unchanged) }).waitFor({ timeout: T });
  await page.reload();
  await page.locator(`[data-compare-summary] [data-unchanged]`).filter({ hasText: String(after.summary.unchanged) }).waitFor({ timeout: T });
  const ui = await shown();
  for (const k of Object.keys(ui)) if (ui[k] !== after.summary[k]) throw new Error(`${k}: the page shows ${ui[k]}, the log has ${after.summary[k]}`);
});

await step("a note is kept", async () => {
  const p = (await compare()).pairs.find(p => p.after.verdict === "right" && !p.after.note && p.after.verdict_from === "answer");
  const row = page.locator(`[data-case="${p.case}"] tr[data-request]`).filter({ hasText: p.model });
  await row.locator("details.vnote > summary").click();
  await row.getByLabel("Note").fill("Fine, but slow.");
  await row.getByRole("button", { name: "Save note" }).click();
  await page.getByText("Note saved.").waitFor({ timeout: T });
  await page.reload();
  await page.locator("[data-compare-summary]").waitFor({ timeout: T });
  const kept = await page.locator(`[data-case="${p.case}"] tr[data-request]`).filter({ hasText: p.model }).getByLabel("Note").inputValue();
  if (kept !== "Fine, but slow.") throw new Error(`the note reads "${kept}" after a reload`);
});

if (errors.length) { console.error(`FAIL page errors:\n${errors.join("\n")}`); await browser.close(); process.exit(1); }
await browser.close();
console.log("requests passed");
