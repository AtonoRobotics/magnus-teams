// Nova's screens in the real app, beside its mockup
// (docs/nova-greenfield-mockup.html), at the mockup's window size:
// - every screen has the mockup's top bar beside the rail: where the person
//   is and the month's spend; the companion and Light / Dark are chosen in
//   Settings, not on every screen, and nothing is staged to show off
//   (decisions/2026-11-28-ui-without-ceremony.md); a page has no lone close;
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
import { mkdirSync, writeFileSync } from "node:fs";

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
    companion: !!bar?.querySelector("[data-nova-set-avatar], [data-nova-toggle-mode]"),
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
    check(`${mode}: New Chat has the top bar, with no companion or Light / Dark in it`, () => assert.deepEqual([f.bar, f.barTitle, f.companion], [true, "New conversation", false], JSON.stringify(f)));
    const staged = await page.evaluate(() => ({ showcase: !!document.querySelector(".nova-showcase, [data-nova-prompt]"), halo: getComputedStyle(document.body, "::before").backgroundImage }));
    check(`${mode}: a new conversation stages no showcase and no halo`, () => assert.deepEqual([staged.showcase, /gradient/.test(staged.halo)], [false, false], JSON.stringify(staged)));
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
    check(`${mode}: a conversation has the top bar`, () => assert.deepEqual([f.bar, f.barTitle, f.companion], [true, "Conversation", false], JSON.stringify(f)));
    for (const [hash, name, title] of [["#/projects", "projects", "Project Hub"], ["#/briefing", "briefing", "Morning Briefing & Routines"], ["#/explore", "explore", "Explore & Artifacts"], ["#/library", "apps", "Apps & Skills"]]) {
      await go(hash);
      f = await frame();
      if (shots) await page.screenshot({ path: join(shots, `nova-${mode}-${name}.png`) });
      check(`${mode}: ${name} has the top bar, titled ${title}`, () => assert.deepEqual([f.bar, f.barTitle], [true, title], JSON.stringify(f)));
      check(`${mode}: ${name} has no companion or Light / Dark in its bar`, () => assert.equal(f.companion, false, JSON.stringify(f)));
      check(`${mode}: ${name} has no lone close`, () => assert.equal(f.loneClose, false, JSON.stringify(f)));
    }
    // Apps in the mockup's measures (owner, 2026-11-27: "Fonts, font size,
    // padding / space and layouts don't match mockup").
    await go("#/library");
    const apps = await page.evaluate(() => {
      const q = (/** @type {string} */ sel) => document.querySelector(sel);
      const box = (/** @type {?Element} */ e) => e ? e.getBoundingClientRect() : null;
      const css = (/** @type {?Element} */ e, /** @type {string} */ p) => e ? getComputedStyle(e).getPropertyValue(p) : "";
      const h1 = q(".libpage > .lhead h1"), seg = q(".libpage > .lhead .lseg"), hero = q(".nova-featured");
      const search = q(".libpage .lsearch"), label = q("#lib-sections .lsect-head h2");
      const rows = [...document.querySelectorAll("#lib-sections .lcard")].slice(0, 2), row = rows[0] || null;
      const r0 = box(rows[0] || null), r1 = box(rows[1] || null);
      return {
        page: Math.round(box(q(".libpage"))?.width || 0),
        h1: [css(h1, "font-size"), css(h1, "font-weight")],
        segBesideTitle: !!seg && Math.abs((box(seg)?.top || 0) - (box(h1)?.top || 0)) < 40,
        hero: !!hero,
        order: [box(search)?.top || 0, box(label)?.top || 0],
        label: [css(label, "font-size"), css(label, "font-weight")],
        row: row ? {
          padding: css(row, "padding"), border: css(row, "border-top-width"), radius: css(row, "border-top-left-radius"),
          tile: Math.round(box(row.querySelector(".ctile"))?.width || 0),
          name: [css(row.querySelector(".lc-name h3"), "font-size"), css(row.querySelector(".lc-name h3"), "font-weight")],
          sum: [css(row.querySelector(".lc-sum"), "font-size"), Math.round(box(row.querySelector(".lc-sum"))?.height || 0)],
          act: Math.round(box(row.querySelector(".lc-act"))?.width || 0),
          foot: !!box(row.querySelector(".lc-foot"))?.height,
          height: Math.round(r0?.height || 0),
        } : null,
        twoColumns: !!r0 && !!r1 && Math.abs(r0.top - r1.top) < 2 && r1.left > r0.right,
      };
    });
    if (shots) await page.screenshot({ path: join(shots, `nova-${mode}-apps-measured.png`) });
    check(`${mode}: Apps is the mockup's 896px column`, () => assert.ok(apps.page > 0 && apps.page <= 896, JSON.stringify(apps)));
    check(`${mode}: Apps' title is 24px in medium weight, the segmented pill beside it`, () => assert.ok(apps.h1[0] === "24px" && apps.h1[1] === "500" && apps.segBesideTitle, JSON.stringify(apps)));
    check(`${mode}: Apps has no featured banner`, () => assert.equal(apps.hero, false, JSON.stringify(apps)));
    check(`${mode}: search, then the catalog`, () => assert.ok(apps.order[0] > 0 && apps.order[0] < apps.order[1], JSON.stringify(apps)));
    check(`${mode}: a catalog head is a 14px label`, () => assert.deepEqual(apps.label, ["14px", "500"], JSON.stringify(apps)));
    check(`${mode}: a catalog row is the mockup's: 12px by 10px, no border, a 40px tile, a 14px name over a 12px line, a 32px action`, () => assert.ok(apps.row
      && apps.row.padding === "12px 10px" && apps.row.border === "0px" && apps.row.radius === "14px" && apps.row.tile === 40
      && apps.row.name[0] === "14px" && apps.row.name[1] === "500" && apps.row.sum[0] === "12px" && apps.row.sum[1] <= 18
      && apps.row.act === 32 && !apps.row.foot && apps.row.height <= 66, JSON.stringify(apps)));
    check(`${mode}: the catalog is two columns`, () => assert.ok(apps.twoColumns, JSON.stringify(apps)));
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
    // A waiting update shows on the rail as one icon button, not its words
    // broken a few letters to a line (the installed app, 2026-11-26).
    await page.evaluate(() => { App.update = { ready: { version: "9.9.9" } }; sidebar(); });
    await page.waitForTimeout(300);
    const upd = await page.evaluate(() => {
      const b = document.querySelector(".shell > nav.side .update-ready button"), r = b?.getBoundingClientRect();
      return { w: Math.round(r?.width || 0), h: Math.round(r?.height || 0), words: !!b?.querySelector(".update-label")?.getBoundingClientRect().height };
    });
    if (shots) await page.screenshot({ path: join(shots, `nova-${mode}-update.png`) });
    check(`${mode}: a waiting update is one icon on the rail`, () => assert.ok(upd.w > 0 && upd.w <= 44 && upd.h <= 44 && !upd.words, JSON.stringify(upd)));
    await page.evaluate(() => { App.update = null; sidebar(); });
    // Inbox opens beside the latest conversation, as the mockup's stage does.
    await go("#/inbox");
    f = await frame();
    if (shots) await page.screenshot({ path: join(shots, `nova-${mode}-inbox.png`) });
    check(`${mode}: Inbox has the top bar`, () => assert.deepEqual([f.bar, f.companion], [true, false], JSON.stringify(f)));
    // The right panel's tabs are one row level with the conversation's head,
    // with no title row above them, as in the mockup.
    const panel = await page.evaluate(() => {
      const mid = (/** @type {?Element} */ e) => { const r = e?.getBoundingClientRect(); return r ? Math.round(r.top + r.height / 2) : -100; };
      const title = document.querySelector("aside.detail .stage-artifact .artifact-title");
      const last = [...document.querySelectorAll("aside.detail .panel-tabs > *")].pop()?.getBoundingClientRect();
      const close = document.querySelector("aside.detail .artifact-close")?.getBoundingClientRect();
      return { tabs: mid(document.querySelector("aside.detail .panel-tabs")), head: mid(document.querySelector("section.conv > .conv-head")), title: !!title && title.getBoundingClientRect().height > 0, fits: !!last && !!close && last.right <= close.left };
    });
    if (process.env.NOVA_DUMP && shots && mode === "dark") {
      const dump = await page.evaluate(() => {
        const cut = (/** @type {?Element} */ e) => (e?.outerHTML || "").replace(/<svg[\s\S]*?<\/svg>/g, "<svg/>").slice(0, 24000);
        const st = (/** @type {string} */ sel) => { const e = document.querySelector(sel); if (!e) return null; const c = getComputedStyle(e), r = e.getBoundingClientRect(); return { sel, x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height), bg: c.backgroundColor, bgi: c.backgroundImage.slice(0, 80), border: `${c.borderLeftWidth} ${c.borderLeftColor} / ${c.borderTopWidth} ${c.borderTopColor}`, radius: c.borderRadius, font: `${c.fontSize} ${c.fontWeight} ${c.lineHeight}`, pad: c.padding, color: c.color }; };
        return { shell: getComputedStyle(document.querySelector(".shell") || document.body).gridTemplateColumns, body: { ...document.body.dataset }, styles: [".shell > main", "section.conv", "section.conv > .conv-head", ".msg.me", ".msg.me .body", ".msg.magnus", ".msg.magnus .meta", "#workspace-splitter", "aside.detail", ".stage-artifact", ".stage-artifact > .panel-tabs", ".stage-artifact > .artifact-content", ".aurora-say-state", ".composer-slot .say"].map(st), conv: cut(document.querySelector("section.conv")), detail: cut(document.querySelector("aside.detail")) };
      });
      writeFileSync(join(shots, "dump.json"), JSON.stringify(dump, null, 1));
    }
    check(`${mode}: the panel's tabs sit level with the conversation's head, with no title row`, () => assert.ok(Math.abs(panel.tabs - panel.head) <= 4 && !panel.title, JSON.stringify(panel)));
    check(`${mode}: every panel tab shows beside Close`, () => assert.equal(panel.fits, true, JSON.stringify(panel)));
    // The chat and the panel as the mockup's stage draws them (2026-11-26).
    const look = await page.evaluate(() => {
      const cs = (/** @type {string} */ sel) => { const e = document.querySelector(sel); return e ? getComputedStyle(e) : null; };
      const w = (/** @type {string} */ sel) => document.querySelector(sel)?.getBoundingClientRect().width || 0;
      const clear = (/** @type {?CSSStyleDeclaration} */ c) => !!c && /rgba\(0, 0, 0, 0\)|transparent/.test(c.backgroundColor);
      const state = document.querySelector(".aurora-say-state");
      return {
        panelClear: clear(cs("aside.detail")) && clear(cs("aside.detail .stage-artifact")),
        panelShare: Math.round(100 * w("aside.detail") / (w("aside.detail") + w(".shell > main") + w("#workspace-splitter"))),
        queue: document.querySelector("aside.detail .appr-queue-head")?.textContent || "",
        wash: cs(".aurora-bg-canvas")?.display || "none",
        bubbleRim: cs(".msg.me")?.borderTopWidth || "",
        faces: document.querySelectorAll("#thread .msg.magnus > .byline > .aurora-avatar").length,
        lastSigned: !![...document.querySelectorAll("#thread .msg.magnus > .byline")].pop()?.querySelector(":scope > .aurora-avatar"),
        name: cs("#thread .msg.magnus > .byline .by-who")?.fontWeight || "",
        idle: state ? /** @type {HTMLElement} */ (state).offsetHeight : 0,
      };
    });
    check(`${mode}: the panel is the stage's own canvas, about 46% wide`, () => assert.ok(look.panelClear && look.panelShare >= 40 && look.panelShare <= 50, JSON.stringify(look)));
    check(`${mode}: Approvals opens on the queue head`, () => assert.match(look.queue, /^Approval queue\d+ waiting/, JSON.stringify(look)));
    check(`${mode}: no aurora wash behind the stage`, () => assert.equal(look.wash, "none"));
    check(`${mode}: the person's bubble has no rim`, () => assert.equal(look.bubbleRim, "0px"));
    check(`${mode}: Magnus signs its latest answer with its face and name`, () => assert.ok(look.faces === 1 && look.lastSigned && Number(look.name) >= 600, JSON.stringify(look)));
    check(`${mode}: at rest nothing is said over the composer`, () => assert.equal(look.idle, 0, JSON.stringify(look)));
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
      // One control collapses the open sidebar, not two of the same icon.
      sidebarButtons: [...document.querySelectorAll(".shell > nav.side :is(.aurora-rail-open, .aurora-rail-pin, [data-nav-toggle])")].filter(e => /** @type {HTMLElement} */ (e).offsetParent).map(e => e.getAttribute("aria-label")),
      drift: [...document.querySelectorAll(".shell > nav.side :is(a, summary, span)")].filter(e => /** @type {HTMLElement} */ (e).offsetHeight && /^(Workspace|Admin|Trash)$/i.test((e.textContent || "").trim())).map(e => (e.textContent || "").trim()),
    }));
    check(`${mode}: the sidebar has no Workspace group, Admin or Trash`, () => assert.deepEqual(side.drift, []));
    check(`${mode}: the open sidebar has one collapse control`, () => assert.deepEqual(side.sidebarButtons, ["Collapse the sidebar"]));
    check(`${mode}: the sidebar docks beside the stage with Quick Jump at its foot`, () => assert.ok(side.navigation === "expanded" && !side.overlay && side.stageLeft >= 200 && side.quickJump, JSON.stringify(side)));
    await page.locator(".shell > nav.side .aurora-rail-pin").click();
    await page.waitForTimeout(700);
    const back = await page.evaluate(() => document.body.dataset.navigation);
    check(`${mode}: collapsing the sidebar puts the rail back`, () => assert.equal(back, "compact"));
  }
  // On a touchscreen, a connected app's check and another app's + line up:
  // each action is one box with its mark centred (owner's touchscreen laptop,
  // 2026-11-27: the checks sat 12px left of the +).
  const touch = await browser.newContext({ hasTouch: true, viewport: { width: 1344, height: 860 } });
  const tp = await touch.newPage();
  tp.on("pageerror", e => errors.push(e.message));
  await tp.goto(process.env.MAGNUS_URL || "");
  await tp.getByLabel("Username").fill("alice");
  await tp.getByLabel("Password").fill(process.env.MAGNUS_PASSWORD || "");
  await tp.getByRole("button", { name: "Sign in", exact: true }).click();
  await tp.locator("#msg").waitFor();
  await tp.evaluate(() => { Theme.set({ style: "nova", mode: "dark", motion: "reduced", sidebar: "rail" }); location.hash = "#/library"; });
  await tp.locator("#lib-sections .lgrid .lcard").first().waitFor();
  const marks = await tp.evaluate(() => {
    const grid = /** @type {HTMLElement} */ (document.querySelector("#lib-sections .lgrid"));
    const c = App.connectors[0];
    grid.insertAdjacentHTML("afterbegin", connectorCard({ ...c, state: "connected", more_access: false }) + connectorCard({ ...c, state: "not_connected" }));
    return {
      coarse: matchMedia("(any-pointer: coarse)").matches,
      rows: [...grid.querySelectorAll(".lcard")].slice(0, 2).map(row => {
        const act = /** @type {HTMLElement} */ (row.querySelector(".lc-act")), mark = /** @type {Element} */ (act.querySelector("svg"));
        const r = row.getBoundingClientRect(), m = mark.getBoundingClientRect();
        return { tag: act.tagName, width: Math.round(act.getBoundingClientRect().width), fromRight: Math.round(r.right - (m.left + m.right) / 2) };
      }),
    };
  });
  if (shots) await tp.screenshot({ path: join(shots, "nova-dark-apps-touch.png") });
  check("on a touchscreen, a connected app's check lines up with another app's +", () => assert.ok(marks.coarse
    && marks.rows.length === 2 && marks.rows[0].tag === "A" && marks.rows[1].tag === "BUTTON"
    && marks.rows[0].width === marks.rows[1].width && Math.abs(marks.rows[0].fromRight - marks.rows[1].fromRight) <= 1, JSON.stringify(marks)));
  await touch.close();
  check("no page errors", () => assert.deepEqual(errors, []));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
console.log("nova screens ok");
