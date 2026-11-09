// Tasks and schedules as live cards in their conversation, in a real browser
// (tests/scratch/tests/live_cards.rs sets it up): the waiting memo's card
// has Cancel, which cancels it; the confirmed schedule's card shows its next
// run, and Pause, Resume and Cancel act on it directly; an ended schedule's
// card says it ended, with why, and its one act is Delete (#452); the plan's result, which came
// after the person's newer message, quotes what they asked.
//
// MAGNUS_URL, MAGNUS_PERSON, MAGNUS_PASSWORD  where and who
// MAGNUS_THREAD  the conversation; MAGNUS_MEMO, MAGNUS_PLAN  its tasks
// MAGNUS_ENDED  an ended schedule's id, for its card
// MAGNUS_CHROMIUM  Chrome to drive; MAGNUS_SHOTS  optional: screenshots
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, person, password, thread, memo, plan, ended] = ["MAGNUS_URL", "MAGNUS_PERSON", "MAGNUS_PASSWORD", "MAGNUS_THREAD", "MAGNUS_MEMO", "MAGNUS_PLAN", "MAGNUS_ENDED"].map(env);
const T = 60_000;

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
const errors = [];
page.on("pageerror", e => errors.push(String(e)));
const dialogs = [];
// A webview may answer window.confirm no without showing it (the Windows
// app's did, 2026-11-03): here it does, and the app's own question, in
// its dialog, is answered yes.
await page.exposeFunction("magnusAsked", m => dialogs.push(m));
await page.addInitScript(() => {
  window.confirm = () => false;
  new MutationObserver(() => {
    for (const q of document.querySelectorAll("dialog.ask[open]:not([data-answered])")) {
      q.dataset.answered = "1";
      window.magnusAsked(q.querySelector("p").textContent);
      q.querySelector("[data-ask=yes]").click();
    }
  }).observe(document, { childList: true, subtree: true, attributes: true, attributeFilter: ["open"] });
});
let n = 0;
const shoot = name => process.env.MAGNUS_SHOTS ? page.screenshot({ path: join(process.env.MAGNUS_SHOTS, `live-cards-${String(++n).padStart(2, "0")}-${name}.png`), fullPage: true }).catch(() => {}) : null;
const step = async (name, f) => {
  try { await f(); await shoot(name); console.log(`ok   ${name}`); }
  catch (e) { await shoot(`FAILED-${name}`); console.error(`FAIL ${name}: ${e.message}`); await browser.close(); process.exit(1); }
};

await step("sign in and open the conversation", async () => {
  await page.goto(base);
  await page.getByLabel("Username").fill(person);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.getByRole("link", { name: /^Needs you/ }).waitFor({ timeout: T });
  await page.locator(`nav.side a[href="#/chat/${thread}"]`).first().click();
  await page.locator(`#thread [data-chat-task="${memo}"]`).waitFor({ timeout: T });
});

await step("the plan's result, after the person moved on, quotes what they asked", async () => {
  const result = page.locator(`#thread [data-result="${plan}"]`);
  await result.waitFor({ timeout: T });
  await result.locator("blockquote.asked", { hasText: "Draft the plan for the offsite." }).waitFor({ timeout: T });
});

await step("the schedule's card shows its next run, and pauses and resumes from the card", async () => {
  const card = page.locator("#thread .sched-state");
  await card.getByText(/Next run/).waitFor({ timeout: T });
  await card.getByRole("button", { name: "Pause" }).click();
  await card.locator(".chip", { hasText: "Paused" }).waitFor({ timeout: T });
  await card.getByRole("button", { name: "Resume" }).click();
  await card.locator(".chip", { hasText: "Scheduled" }).waitFor({ timeout: T });
});
await step("an ended schedule's card says it ended, with why, and offers only Delete", async () => {
  const card = page.locator(`#thread [data-schedule="${ended}"]`);
  await card.locator(".chip", { hasText: "Ended" }).waitFor({ timeout: T });
  await card.getByText(/reached its end/).waitFor({ timeout: T });
  if (await card.getByText("Cancelled").count()) throw new Error("an ended schedule's card says Cancelled");
  const acts = await card.getByRole("button").allInnerTexts();
  if (acts.length !== 1 || acts[0].trim() !== "Delete") throw new Error(`an ended schedule's card offers ${JSON.stringify(acts)}, not Delete alone`);
});

await step("Cancel on the memo's card cancels it", async () => {
  const card = page.locator(`#thread [data-chat-task="${memo}"]`);
  await card.getByRole("button", { name: "Cancel" }).click();
  await page.locator(`#thread [data-chat-task="${memo}"][data-status="cancelled"]`).waitFor({ timeout: T });
  if (await page.locator(`#thread [data-chat-task="${memo}"]`).getByRole("button", { name: "Cancel" }).count()) throw new Error("a cancelled task still offers Cancel");
});

await step("Cancel on the schedule's card cancels it, and the card says so", async () => {
  await page.locator("#thread .sched-state").getByRole("button", { name: "Cancel" }).click();
  await page.locator("#thread [data-schedule-gone]").getByText("Details unavailable", {exact:true}).waitFor({ timeout: T });
});

if (errors.length) { console.error(`FAIL page errors: ${errors.join("; ")}`); await browser.close(); process.exit(1); }
await browser.close();
console.log("live cards passed");
