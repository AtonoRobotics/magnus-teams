// What a task started from chat delivers, drawn in the conversation, in a
// real browser (tests/scratch/tests/chat_results.rs sets it up): the trip's
// result in the thread shows its timeline and its table.
//
// MAGNUS_URL, MAGNUS_PERSON, MAGNUS_PASSWORD  where and who
// MAGNUS_THREAD    the conversation; MAGNUS_TASK  the finished task started there
// MAGNUS_CHROMIUM  Chrome to drive; MAGNUS_SHOTS  optional: a screenshot
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, person, password, thread, task] = ["MAGNUS_URL", "MAGNUS_PERSON", "MAGNUS_PASSWORD", "MAGNUS_THREAD", "MAGNUS_TASK"].map(env);
const T = 60_000;

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
const errors = [];
page.on("pageerror", e => errors.push(String(e)));
// A screenshot is evidence, never a check.
const shoot = name => process.env.MAGNUS_SHOTS ? page.screenshot({ path: join(process.env.MAGNUS_SHOTS, name) }).catch(e => console.error(`screenshot not kept: ${e.message.split("\n")[0]}`)) : null;
const fail = async m => { console.error(`FAIL ${m}`); await shoot("chat-results-FAILED.png"); await browser.close(); process.exit(1); };

await page.goto(base);
await page.getByLabel("Username").fill(person);
await page.getByLabel("Password").fill(password);
await page.getByRole("button", { name: "Sign in" }).click();
await page.getByRole("link", { name: /^Needs you/ }).waitFor({ timeout: T });
// The conversation the trip was asked for in.
await page.locator(`nav.side a[href="#/chat/${thread}"]`).first().click();
const result = page.locator(`#thread [data-result="${task}"]`);
try { await result.waitFor({ timeout: T }); } catch { await fail("the trip's result is not in the conversation"); }

// Its deliverable is drawn there, in its frame.
const frame = result.locator("iframe.composed[data-shown]");
if (!(await frame.count())) await fail("the trip's result in the conversation draws no deliverable");
const drawn = await page.waitForFunction(id => {
  const f = document.querySelector(`#thread [data-result="${id}"] iframe.composed[data-shown]`);
  const d = f?.contentDocument;
  if (!d?.querySelector(".tl") || !d.querySelector("table")) return null;
  return { days: d.querySelectorAll(".tl li").length, total: [...d.querySelectorAll("td")].some(td => td.textContent.trim() === "$912"), height: f.getBoundingClientRect().height, width: f.getBoundingClientRect().width, column: (t => t.clientWidth - parseFloat(getComputedStyle(t).paddingLeft) - parseFloat(getComputedStyle(t).paddingRight))(document.querySelector("#thread")), tl: d.querySelector(".tl").getBoundingClientRect().width, card: d.querySelector(".tl").closest("figure").clientWidth };
}, task, { timeout: T }).then(h => h.jsonValue()).catch(() => null);
if (!drawn) await fail("the deliverable's frame in the conversation is empty");
if (drawn.days !== 3) await fail(`the timeline shows ${drawn.days} of its 3 days`);
if (!drawn.total) await fail("the table's total is not shown");
if (drawn.height < 100) await fail(`the deliverable's frame is ${drawn.height}px tall`);
if (drawn.tl < drawn.card - 1) await fail(`the timeline is ${drawn.tl}px wide in its ${drawn.card}px card`);
// It has the conversation's width, as the outcome page gives it the panel's.
if (drawn.width < drawn.column - 2) await fail(`the deliverable's frame is ${drawn.width}px wide in a ${drawn.column}px column`);
await shoot("chat-results.png");

// Nothing lies over the thread: its head is above it and the composer below,
// so whatever is scrolled into view in it can be clicked (the installed app's
// driver scrolls a target to the edge, and found it under the composer).
const overlap = await page.evaluate(() => {
  const r = s => document.querySelector(s).getBoundingClientRect();
  const t = r("#thread"), h = r("#conv-head"), c = r("#composer");
  return { underHead: h.bottom - t.top, underComposer: t.bottom - c.top };
});
if (overlap.underHead > 0.5 || overlap.underComposer > 0.5) await fail(`the thread lies under the conversation's head or composer: ${JSON.stringify(overlap)}`);

// Once drawn, the frame leaves the thread as it is: the thread is not drawn
// again for its sake (a redraw replaces what a person is about to click).
const still = await page.evaluate(id => new Promise(done => {
  const r = document.querySelector(`#thread [data-result="${id}"]`);
  r._seen = true;
  setTimeout(() => done(document.querySelector(`#thread [data-result="${id}"]`)?._seen === true), 5000);
}), task);
if (!still) await fail("the thread drew itself again after the deliverable loaded");

// A redraw of the thread makes the frame anew; it comes back at the height it
// had, so nothing in the conversation moves while it loads again.
const redrawn = await page.evaluate(id => new Promise(done => {
  const thread = document.querySelector("#thread");
  const before = document.querySelector(`#thread [data-result="${id}"] iframe.composed[data-shown]`).getBoundingClientRect().height;
  const seen = new MutationObserver(() => {
    const f = document.querySelector(`#thread [data-result="${id}"] iframe.composed[data-shown]`);
    if (f && !f._drawnBefore) { seen.disconnect(); done({ before, after: f.getBoundingClientRect().height }); }
  });
  document.querySelector(`#thread [data-result="${id}"] iframe.composed[data-shown]`)._drawnBefore = true;
  seen.observe(thread, { childList: true, subtree: true });
  thread._html = "";   // the next refresh draws the thread again
  setTimeout(() => { seen.disconnect(); done(null); }, 10_000);
}), task);
if (!redrawn) await fail("the thread was not drawn again");
if (Math.abs(redrawn.after - redrawn.before) > 1) await fail(`a redraw of the thread drew the deliverable's frame ${redrawn.after}px tall, not the ${redrawn.before}px it had`);

if (errors.length) await fail(`page errors: ${errors.join("; ")}`);
await browser.close();
console.log("the deliverable shows in the conversation");
