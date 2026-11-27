// Nova's screens in the real app, beside its mockup
// (docs/nova-greenfield-mockup.html), at the mockup's window size:
// - every screen has the mockup's top bar beside the rail: where the person
//   is and the month's spend on the left, the companion and Light / Dark
//   controls on the right; a page has no lone close;
// - the compact rail is Nova's 60px column.
// Each screen is saved to MAGNUS_SHOTS, to be put beside the mockup's.
//
// MAGNUS_URL, MAGNUS_PASSWORD  the server and alice's password
// MAGNUS_CHROMIUM              Chrome to drive
// MAGNUS_SHOTS                 where to save the screenshots, if set
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { mkdirSync } from "node:fs";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const browser = await chromium.launch({ executablePath: process.env.MAGNUS_CHROMIUM, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const page = await browser.newPage({ viewport: { width: 1344, height: 860 } });
/** @type {string[]} */
const errors = [];
page.on("pageerror", e => errors.push(e.message));
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
/** @type {string[]} */
const failures = [];
const check = (/** @type {string} */ what, /** @type {() => void} */ fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${/** @type {Error} */ (e).message}`); console.log(`not ok - ${what}`); } };

/** The frame around the page: its bar, the controls and the rail. */
const frame = () => page.evaluate(() => {
  const shown = (/** @type {?Element} */ e) => { if (!e) return false; const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 && getComputedStyle(e).visibility !== "hidden"; };
  const bar = document.querySelector(".shell > .nova-frame-bar");
  const rail = document.querySelector(".shell > nav.side");
  return {
    hash: location.hash,
    bar: shown(bar),
    barTitle: bar?.querySelector(".nova-page-title")?.textContent?.trim() || "",
    controls: shown(bar?.querySelector(".nova-topbar-ctrls") || null),
    spend: bar?.querySelector(".nova-frame-spend")?.textContent || "",
    loneClose: [...document.querySelectorAll("#main .aurora-page-close")].some(shown),
    rail: rail ? Math.round(rail.getBoundingClientRect().width) : 0,
  };
});

const go = async (/** @type {string} */ hash) => {
  await page.evaluate(h => { document.querySelectorAll("dialog[open]").forEach(d => /** @type {HTMLDialogElement} */ (d).close()); location.hash = h; }, hash);
  await page.waitForTimeout(1500);
};

try {
  await page.goto(process.env.MAGNUS_URL || "");
  await page.getByLabel("Username").fill("alice");
  await page.getByLabel("Password").fill(process.env.MAGNUS_PASSWORD || "");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.locator("#msg").waitFor();
  for (const mode of ["dark", "light"]) {
    await page.evaluate(m => Theme.set({ style: "nova", mode: m, motion: "reduced", sidebar: "rail" }), mode);
    await go("#/chat/new");
    await page.waitForTimeout(800);
    let f = await frame();
    if (shots) await page.screenshot({ path: join(shots, `nova-${mode}-home.png`) });
    check(`${mode}: New Chat has the top bar with its controls`, () => assert.deepEqual([f.bar, f.barTitle, f.controls], [true, "New conversation", true], JSON.stringify(f)));
    check(`${mode}: the bar shows the month's spend`, () => assert.match(f.spend, /^Spend this month: \$\d+\.\d\d/, JSON.stringify(f)));
    check(`${mode}: the rail is Nova's 60px column`, () => assert.equal(f.rail, 60, JSON.stringify(f)));
    const rail = await page.evaluate(() => {
      const y = (/** @type {string} */ sel) => document.querySelector(`.shell > nav.side ${sel}`)?.getBoundingClientRect().top ?? -1;
      return { newChat: y(".new-chat"), inbox: y(".nav-inbox"), apps: y(".nav-apps"), explore: y(".nav-explore"), search: y(".rail-search"), me: y(".who > a.avatar"), admin: !!document.querySelector('nav.side a[href^="#/admin"]') };
    });
    check(`${mode}: the rail runs as the mockup's: New, Inbox, Explore then Apps, search and the person at the foot`, () => assert.ok(
      rail.newChat >= 0 && rail.newChat < rail.inbox && rail.inbox < rail.explore && rail.explore < rail.apps && rail.apps < rail.search && rail.search < rail.me, JSON.stringify(rail)));
    check(`${mode}: the navigation has no Admin entry: Admin is in Settings`, () => assert.equal(rail.admin, false));
    const titles = await page.evaluate(() => (App.threads || []).map(t => displayTitle(t.title)));
    check(`${mode}: conversation titles show no Markdown`, () => assert.ok(titles.includes("Plan the offsite") && !titles.some(t => t.includes("**")), JSON.stringify(titles)));
    await go(await page.evaluate(() => `#/chat/${encodeURIComponent(App.threads[0].thread)}`));
    f = await frame();
    if (shots) await page.screenshot({ path: join(shots, `nova-${mode}-stage.png`) });
    check(`${mode}: a conversation has the top bar with its controls`, () => assert.deepEqual([f.bar, f.barTitle, f.controls], [true, "Conversation", true], JSON.stringify(f)));
    for (const [hash, name, title] of [["#/projects", "projects", "Project Hub"], ["#/briefing", "briefing", "Morning Briefing & Routines"], ["#/explore", "explore", "Explore & Artifacts"], ["#/library", "apps", "Apps & Skills"]]) {
      await go(hash);
      f = await frame();
      if (shots) await page.screenshot({ path: join(shots, `nova-${mode}-${name}.png`) });
      check(`${mode}: ${name} has the top bar, titled ${title}`, () => assert.deepEqual([f.bar, f.barTitle], [true, title], JSON.stringify(f)));
      check(`${mode}: ${name} has the controls in its bar`, () => assert.equal(f.controls, true, JSON.stringify(f)));
      check(`${mode}: ${name} has no lone close`, () => assert.equal(f.loneClose, false, JSON.stringify(f)));
    }
    // The rail goes down item by item, clicked as a person clicks: rest on
    // one until its label shows, then click the next. On 2026-11-26 the label
    // lay over the next item in the installed app and took the click.
    const order = [[".nav-projects", "#/projects"], [".nav-briefing", "#/briefing"], [".nav-explore", "#/explore"], [".nav-apps", "#/library"]];
    await go("#/projects");
    for (let i = 1; i < order.length; i++) {
      const mid = async (/** @type {string} */ sel) => { const b = await page.locator(`.shell > nav.side ${sel}`).boundingBox(); return [(b?.x || 0) + (b?.width || 0) / 2, (b?.y || 0) + (b?.height || 0) / 2]; };
      const [ax, ay] = await mid(order[i - 1][0]);
      await page.mouse.move(ax, ay);
      await page.waitForTimeout(700);
      const [x, y] = await mid(order[i][0]);
      const hit = await page.evaluate(([px, py]) => { const e = document.elementFromPoint(px, py); return e ? `${e.tagName}#${e.id}.${e.className}` : "nothing"; }, [x, y]);
      await page.mouse.click(x, y);
      await page.waitForTimeout(1500);
      const at = await page.evaluate(() => location.hash);
      check(`${mode}: the rail goes from ${order[i - 1][1]} down to ${order[i][1]}`, () => assert.equal(at, order[i][1], `the point held ${hit}`));
    }
    // Inbox opens beside the latest conversation, as the mockup's stage does.
    await go("#/inbox");
    f = await frame();
    if (shots) await page.screenshot({ path: join(shots, `nova-${mode}-inbox.png`) });
    check(`${mode}: Inbox has the top bar with its controls`, () => assert.deepEqual([f.bar, f.controls], [true, true], JSON.stringify(f)));
    // The right panel's tabs are one row level with the conversation's head,
    // with no title row above them, as in the mockup.
    const panel = await page.evaluate(() => {
      const mid = (/** @type {?Element} */ e) => { const r = e?.getBoundingClientRect(); return r ? Math.round(r.top + r.height / 2) : -100; };
      const title = document.querySelector("aside.detail .stage-artifact .artifact-title");
      const last = [...document.querySelectorAll("aside.detail .panel-tabs > *")].pop()?.getBoundingClientRect();
      const close = document.querySelector("aside.detail .artifact-close")?.getBoundingClientRect();
      return { tabs: mid(document.querySelector("aside.detail .panel-tabs")), head: mid(document.querySelector("section.conv > .conv-head")), title: !!title && title.getBoundingClientRect().height > 0, fits: !!last && !!close && last.right <= close.left };
    });
    check(`${mode}: the panel's tabs sit level with the conversation's head, with no title row`, () => assert.ok(Math.abs(panel.tabs - panel.head) <= 4 && !panel.title, JSON.stringify(panel)));
    check(`${mode}: every panel tab shows beside Close`, () => assert.equal(panel.fits, true, JSON.stringify(panel)));
    await go("#/tasks");
    if (shots) await page.screenshot({ path: join(shots, `nova-${mode}-tasks.png`) });
    // The sidebar opens beside the stage, as in the mockup, not over it, and
    // its collapse puts the rail back.
    await page.locator(".shell > nav.side .aurora-rail-open").click();
    await page.waitForTimeout(900);
    if (shots) await page.screenshot({ path: join(shots, `nova-${mode}-sidebar.png`) });
    const side = await page.evaluate(() => ({
      navigation: document.body.dataset.navigation,
      overlay: !!document.querySelector("#navigation-drawer[open]"),
      stageLeft: Math.round(document.getElementById("main")?.getBoundingClientRect().left || 0),
      quickJump: !!document.querySelector(".shell > nav.side .nav-quick-jump")?.getBoundingClientRect().height,
      drift: [...document.querySelectorAll(".shell > nav.side :is(a, summary, span)")].filter(e => /** @type {HTMLElement} */ (e).offsetHeight && /^(Workspace|Admin|Trash)$/i.test((e.textContent || "").trim())).map(e => (e.textContent || "").trim()),
    }));
    check(`${mode}: the sidebar has no Workspace group, Admin or Trash`, () => assert.deepEqual(side.drift, []));
    check(`${mode}: the sidebar docks beside the stage with Quick Jump at its foot`, () => assert.ok(side.navigation === "expanded" && !side.overlay && side.stageLeft >= 200 && side.quickJump, JSON.stringify(side)));
    await page.locator(".shell > nav.side .aurora-rail-pin").click();
    await page.waitForTimeout(700);
    const back = await page.evaluate(() => document.body.dataset.navigation);
    check(`${mode}: collapsing the sidebar puts the rail back`, () => assert.equal(back, "compact"));
  }
  check("no page errors", () => assert.deepEqual(errors, []));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
console.log("nova screens ok");
