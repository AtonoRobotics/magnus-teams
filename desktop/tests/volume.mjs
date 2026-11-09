// Tasks at volume, in a real browser at the desktop app's window size:
// everything that needs the person is on the first screen of Tasks without
// scrolling, sections come in order, a request's tasks show as one group
// with its progress, a chat message's tasks as one card with their results
// folded into one-liners, and a schedule shows its next run.
//
// MAGNUS_URL, MAGNUS_PERSON, MAGNUS_PASSWORD  where and who
// MAGNUS_NEEDS     how many tasks wait on the person
// MAGNUS_THREAD    the thread of "Plan the team offsite." (5 tasks: 3 done, 2 need the person)
// MAGNUS_CHROMIUM  Chrome to drive; MAGNUS_SHOTS  where to keep screenshots
import { createRequire } from "node:module";
import { mkdirSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const env = k => { const v = process.env[k]; if (!v) { console.error(`${k} is not set`); process.exit(2); } return v; };
const [base, person, password, needs, thread] = ["MAGNUS_URL", "MAGNUS_PERSON", "MAGNUS_PASSWORD", "MAGNUS_NEEDS", "MAGNUS_THREAD"].map(env);
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
// One screen is the app's own window.
const win = JSON.parse(readFileSync(join(root, "desktop/src-tauri/tauri.conf.json"), "utf8")).app.windows[0];

const browser = await chromium.launch({ executablePath: env("MAGNUS_CHROMIUM"), args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: win.width, height: win.height } });
const errors = [];
page.on("pageerror", e => errors.push(String(e)));
let n = 0;
const shot = async name => { if (shots) await page.screenshot({ path: join(shots, `volume-${String(++n).padStart(2, "0")}-${name}.png`) }).catch(e => console.error(`screenshot not kept: ${e.message.split("\n")[0]}`)); };
const step = async (name, f) => {
  try { await f(); await shot(name); console.log(`ok   ${name}`); }
  catch (e) { await shot(`FAILED-${name}`); console.error(`FAIL ${name}: ${e.message}`); await browser.close(); process.exit(1); }
};
const T = 60_000;
// Poll from here: the page's CSP forbids evaluating strings, which
// waitForFunction with arguments needs.
const until = async (what, f) => {
  for (const end = Date.now() + T; Date.now() < end; await page.waitForTimeout(200)) if (await f()) return;
  throw new Error(`timed out waiting for ${what}`);
};

await step("sign in", async () => {
  await page.goto(base);
  await page.getByLabel("Username").fill(person);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  // Signed in: the workspace, with Needs you in the sidebar.
  await page.getByRole("link", { name: /^Needs you/ }).waitFor({ timeout: T });
});

await step("actionable tasks lead and stay reachable without page scrolling", async () => {
  // Tasks opens beside the conversation.
  await page.getByRole("link", { name: "Activity", exact: true }).click();
  await page.getByRole("navigation", { name: "Activity views" }).getByRole("link", { name: "Tasks", exact: true }).click();
  const rows = page.locator('#detail [data-section="needs"] .trow[data-state="needs"]');
  await until(`${needs} tasks needing you`, async () => await rows.count() === Number(needs));
  const order = await page.locator("section.tsec").evaluateAll(els => els.map(e => e.dataset.section));
  const wanted = ["needs", "running", "waiting", "done", "failed"];
  if (order.join() !== wanted.filter(s => order.includes(s)).join()) throw new Error(`sections out of order: ${order.join(", ")}`);
  if (await page.evaluate(() => window.scrollY) !== 0) throw new Error("the page scrolled");
  // UX-01 keeps readable typography and permits long lists to scroll locally.
  const reading=page.locator('#detail .detail-body');
  await rows.first().scrollIntoViewIfNeeded();
  for(const row of await rows.all()){
    await row.scrollIntoViewIfNeeded();const box=await row.boundingBox(),clip=await reading.boundingBox();
    if(box.y<clip.y-1 || box.y+box.height>Math.min(win.height,clip.y+clip.height)+1)throw new Error('an actionable task cannot be reached inside its reading region');
    if(await page.evaluate(()=>window.scrollY)!==0)throw new Error('task navigation scrolled the application shell');
  }
  const heading=await page.locator('#detail .detail-head').boundingBox();if(heading.y<0 || heading.y+heading.height>win.height)throw new Error('task header is clipped');
  const send=await page.locator('#send').boundingBox();if(send.y<0 || send.y+send.height>win.height)throw new Error('task reading hides the composer');
  const group = page.locator(".tgroup", { hasText: "Plan the team offsite." });
  const progress = await group.locator(".ghead").innerText();
  if (!/3 of 5 done · 2 need you/.test(progress)) throw new Error(`the group's progress reads ${progress}`);
  // The rest of the group is folded away until asked for.
  if (await group.locator("details[open]").count()) throw new Error("the group's other tasks are not folded");
});

await step("the other tasks unfold on request", async () => {
  const group = page.locator(".tgroup", { hasText: "Plan the team offsite." });
  await group.locator("details > summary").click();
  await group.getByText("Shortlist venues").waitFor({ timeout: T });
  // A repaint keeps it open.
  await page.waitForTimeout(4500);
  if (!(await group.locator("details[open]").count())) throw new Error("the group folded itself again");
});

await step("a chat message's tasks are one card, results one-liners", async () => {
  await page.goto(`${base}/#/chat/${thread}`);
  const card = page.locator("[data-chat-tasks]");
  await card.waitFor({ timeout: T });
  if (await card.count() !== 1) throw new Error(`${await card.count()} task cards for one message`);
  if (await card.locator("[data-chat-task]").count() !== 5) throw new Error("the card does not list the message's five tasks");
  await card.getByText("Write the invitation").waitFor({ timeout: T });
  const results = page.locator(".result > details");
  await until("three folded results", async () => await results.count() === 3);
  if (await page.locator(".result > details[open]").count()) throw new Error("a result is not folded");
  await results.first().locator(":scope > summary").click();
  await results.first().getByText("The details are in the task.").waitFor({ timeout: T });
});

await step("a schedule shows its next run", async () => {
  await page.getByRole("link", { name: /^Schedules/ }).click();
  const sched = page.locator("#detail [data-schedule]");
  await sched.first().waitFor({ timeout: T });
  await sched.first().getByText("Weekly invoice summary").waitFor({ timeout: T });
  await sched.first().getByText(/Next run /).waitFor({ timeout: T });
  await sched.first().getByText("No results yet").waitFor({ timeout: T });
});

if (errors.length) { console.error(`FAIL page errors:\n${errors.join("\n")}`); await browser.close(); process.exit(1); }
await browser.close();
console.log("volume passed");
