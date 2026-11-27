// Aurora's rail (decisions/2026-11-21-aurora-rail.md), in the real app:
// - the navigation is a rail of icons: the avatar, then New conversation,
//   Search, Conversations and Explore, and Settings at its foot; Trash, the
//   tree and the old collapse control are not on it;
// - Conversations, or Ctrl+B, slides the full sidebar in over the stage,
//   which does not move; it closes on Esc (the focus back on Conversations),
//   on a click outside it, and when a conversation is picked (which opens);
// - its pin keeps it open beside the stage, after a reload too, and
//   unpinned it is the rail again;
// - its rows take the room their words need;
// - another style keeps the navigation as it was, and a narrow window its
//   drawer.
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
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
/** @type {string[]} */
const errors = [];
page.on("pageerror", e => errors.push(e.message));
const shots = process.env.MAGNUS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const shot = async (/** @type {string} */ name) => { if (shots) await page.screenshot({ path: join(shots, `rail-${name}.png`) }); };
const until = async (/** @type {() => boolean} */ fn, what = "UI condition") => {
  const end = Date.now() + 15000;
  while (Date.now() < end) { if (await page.evaluate(fn)) return; await page.waitForTimeout(40); }
  throw Error(`${what} timed out`);
};
const state = () => page.evaluate(() => {
  const nav = /** @type {HTMLElement} */ (document.querySelector("nav.side"));
  const d = /** @type {HTMLDialogElement} */ (document.querySelector("#navigation-drawer"));
  return {
    navigation: document.body.dataset.navigation,
    inDrawer: !!d.contains(nav),
    drawerOpen: d.open,
    drawer: d.open ? d.getBoundingClientRect().toJSON() : null,
    navWidth: nav.getBoundingClientRect().width,
    mainLeft: /** @type {HTMLElement} */ (document.querySelector("#main")).getBoundingClientRect().left,
    focus: /** @type {HTMLElement} */ (document.activeElement)?.getAttribute("aria-label") || document.activeElement?.tagName,
    sidebar: Theme.prefs().sidebar,
    hash: location.hash,
  };
});
/** Once the navigation's column has finished moving to its width. */
const settled = (/** @type {number} */ w, /** @type {string} */ what) => until(new Function(`return Math.abs(document.querySelector("nav.side").getBoundingClientRect().width - ${w}) < 0.01;`), what);
/** Once the sidebar has closed and is the rail again: the dialog's close
 * event, which puts it back, comes a moment after the dialog shuts. */
const railAgain = (/** @type {string} */ what) => until(() => document.body.dataset.navigation === "compact" && !document.querySelector("#navigation-drawer")?.contains(document.querySelector("nav.side")), what);
/** What the rail shows, top to bottom, by name. */
const rail = () => page.evaluate(() => {
  const nav = /** @type {HTMLElement} */ (document.querySelector("nav.side"));
  const shown = (/** @type {Element} */ el) => { const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== "hidden"; };
  return [...nav.querySelectorAll("a, button, .brand .logo")].filter(shown)
    .map(el => ({ name: el.matches(".logo") ? "avatar" : el.getAttribute("aria-label") || el.textContent?.trim() || "", y: Math.round(el.getBoundingClientRect().top) }))
    .sort((a, b) => a.y - b.y);
});

try {
  await page.goto(process.env.MAGNUS_URL || "");
  await page.getByLabel("Username").fill("alice");
  await page.getByLabel("Password").fill(process.env.MAGNUS_PASSWORD || "");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.locator("#msg").waitFor();
  await page.evaluate(() => Theme.set({ style: "aurora", mode: "dark", motion: "full" }));
  await until(() => document.body.dataset.navigation === "compact", "the rail");
  await settled(64, "the rail's width");

  // 1. The rail.
  let s = await state();
  assert.equal(s.navWidth, 64, `the rail is the compact width: ${JSON.stringify(s)}`);
  const items = await rail();
  const names = items.map(i => i.name);
  const at = (/** @type {string} */ n) => names.indexOf(n);
  for (const n of ["avatar", "New conversation", "Search conversations, tasks and files", "Conversations", "Explore", "Settings"]) assert.ok(at(n) >= 0, `the rail shows ${n}: ${JSON.stringify(names)}`);
  assert.ok(at("avatar") < at("New conversation") && at("New conversation") < at("Search conversations, tasks and files") && at("Search conversations, tasks and files") < at("Conversations") && at("Conversations") < at("Explore") && at("Explore") < at("Settings"), `the rail's order: ${JSON.stringify(names)}`);
  for (const n of ["Trash", "Projects and conversations", "Collapse navigation", "Expand navigation", "Pin the sidebar"]) assert.ok(at(n) < 0, `the rail has no ${n}: ${JSON.stringify(names)}`);
  assert.ok(items[items.length - 1].name === "Settings" && items[items.length - 1].y > 640, `Settings is at the rail's foot: ${JSON.stringify(items)}`);
  await shot("1-rail");

  // 2. Conversations slides the full sidebar in over the stage, which stays.
  const before = s.mainLeft;
  await page.getByRole("button", { name: "Conversations", exact: true }).click();
  await until(() => /** @type {HTMLDialogElement} */ (document.querySelector("#navigation-drawer")).open, "the sidebar opening");
  await page.waitForTimeout(320);
  s = await state();
  assert.ok(s.inDrawer && s.navigation === "expanded", `the full sidebar is in the slide-out: ${JSON.stringify(s)}`);
  assert.equal(s.mainLeft, before, "the stage does not move under the sidebar");
  assert.ok(s.drawer && s.drawer.left === 8 && s.drawer.width === 264, `the sidebar floats over the stage's edge: ${JSON.stringify(s.drawer)}`);
  assert.ok(await page.locator("#navigation-drawer .nav-tree a.conv").first().isVisible(), "its conversations are listed");
  assert.ok(await page.locator("#navigation-drawer").getByRole("link", { name: "Trash" }).isVisible(), "Trash is in the full sidebar");
  assert.ok(await page.locator("#navigation-flyout").isHidden(), "the old flyout stays shut");
  await shot("2-slid-in");

  // Its rows take the room their words need.
  const rows = await page.evaluate(() => {
    const convs = [...document.querySelectorAll("#navigation-drawer .nav-tree a.conv")].map(a => a.getBoundingClientRect());
    return { heights: convs.map(r => r.height), gaps: convs.slice(1).map((r, i) => Math.round(r.top - convs[i].bottom)) };
  });
  assert.ok(rows.heights.every(h => h <= 30), `a conversation's row is no taller than it needs: ${JSON.stringify(rows)}`);
  assert.ok(rows.gaps.every(g => g <= 2), `rows sit close: ${JSON.stringify(rows)}`);

  // 3. Esc closes it and gives the focus back to Conversations.
  await page.keyboard.press("Escape");
  await until(() => !(/** @type {HTMLDialogElement} */ (document.querySelector("#navigation-drawer")).open), "Esc closing");
  await settled(64, "the rail after Esc");
  s = await state();
  assert.ok(!s.inDrawer && s.navigation === "compact" && s.navWidth === 64, `the rail again: ${JSON.stringify(s)}`);
  assert.equal(s.focus, "Conversations", "the focus goes back to Conversations");

  // 4. Ctrl+B opens it; a click outside closes it.
  await page.keyboard.press("Control+b");
  await until(() => /** @type {HTMLDialogElement} */ (document.querySelector("#navigation-drawer")).open, "Ctrl+B");
  await page.mouse.click(900, 400);
  await until(() => !(/** @type {HTMLDialogElement} */ (document.querySelector("#navigation-drawer")).open), "a click outside closing");
  await railAgain("the rail after a click outside");

  // 5. Picking a conversation opens it and closes the sidebar.
  await page.keyboard.press("Control+b");
  await until(() => /** @type {HTMLDialogElement} */ (document.querySelector("#navigation-drawer")).open, "Ctrl+B again");
  const pick = page.locator("#navigation-drawer .nav-tree a.conv").first();
  const href = await pick.getAttribute("href");
  await pick.click();
  await until(() => !(/** @type {HTMLDialogElement} */ (document.querySelector("#navigation-drawer")).open), "a pick closing");
  await railAgain("the rail after a pick");
  s = await state();
  assert.equal(s.hash, href, "the conversation picked opens");
  assert.equal(s.navigation, "compact");

  // 6. The pin keeps it beside the stage, through a reload; unpinned, the rail.
  await page.getByRole("button", { name: "Conversations", exact: true }).click();
  await until(() => /** @type {HTMLDialogElement} */ (document.querySelector("#navigation-drawer")).open, "opening to pin");
  await page.getByRole("button", { name: "Pin the sidebar", exact: true }).click();
  await until(() => document.body.dataset.navigation === "expanded" && !document.querySelector("#navigation-drawer")?.contains(document.querySelector("nav.side")), "pinning");
  await settled(216, "the pinned sidebar's width");
  s = await state();
  assert.ok(s.sidebar === "pinned" && s.navWidth === 216 && !s.drawerOpen, `pinned beside the stage: ${JSON.stringify(s)}`);
  assert.equal(await page.getByRole("button", { name: "Unpin the sidebar", exact: true }).getAttribute("aria-pressed"), "true");
  await shot("3-pinned");
  await page.reload();
  await page.locator("#msg").waitFor();
  await until(() => document.body.dataset.navigation === "expanded", "pinned after a reload");
  await page.getByRole("button", { name: "Unpin the sidebar", exact: true }).click();
  await until(() => document.body.dataset.navigation === "compact", "unpinning");
  await settled(64, "the rail after unpinning");
  s = await state();
  assert.ok(s.sidebar === "rail" && s.navWidth === 64, `the rail again: ${JSON.stringify(s)}`);
  assert.equal(s.focus, "Conversations", "unpinned, the focus is on Conversations");

  // The avatar stands for Magnus at the rail's top and in a new conversation
  // (aurora-avatar.js).
  await page.evaluate(() => { location.hash = "#/chat/new"; });
  await page.locator(".hello-head .aurora-avatar-hero").waitFor();
  assert.ok(await page.locator("nav.side .aurora-avatar-nav").isVisible(), "the avatar is at the rail's top");
  assert.match(await page.locator(".aurora-avatar-hero").getAttribute("aria-label") || "", /^Magnus is /);
  const top = await page.evaluate(() => { const t = document.querySelector(".hello-head")?.closest(".thread"), a = document.querySelector(".aurora-avatar-hero"); return { scroll: t?.scrollTop, seen: !!a && a.getBoundingClientRect().top >= (t?.getBoundingClientRect().top || 0) }; });
  assert.deepEqual(top, { scroll: 0, seen: true }, "a new conversation opens at its top, with Magnus in view");
  await page.waitForTimeout(700);
  await shot("4-new");

  // 7. Another style keeps the navigation as it was.
  await page.evaluate(() => Theme.set({ style: "obsidian" }));
  await until(() => document.body.dataset.navigation === "expanded", "another style");
  assert.ok(await page.getByRole("button", { name: "Collapse navigation", exact: true }).isVisible(), "its collapse control");
  assert.ok(await page.locator(".aurora-rail-open, .aurora-rail-pin").evaluateAll(els => els.every(el => !(/** @type {HTMLElement} */ (el).offsetParent))), "no rail controls");
  await page.keyboard.press("Control+b");
  await page.waitForTimeout(120);
  assert.equal((await state()).drawerOpen, false, "Ctrl+B is Aurora's");

  // 8. A narrow window keeps its drawer in Aurora.
  await page.evaluate(() => Theme.set({ style: "aurora" }));
  await page.setViewportSize({ width: 390, height: 844 });
  await until(() => document.body.dataset.navigation === "drawer", "the narrow window");
  await page.getByRole("button", { name: "Open navigation", exact: true }).click();
  await until(() => /** @type {HTMLDialogElement} */ (document.querySelector("#navigation-drawer")).open, "the narrow drawer");
  assert.ok(await page.locator("#navigation-drawer .aurora-rail-pin").isHidden(), "no pin in the narrow drawer");
  await page.keyboard.press("Escape");

  assert.deepEqual(errors, []);
  console.log("aurora rail: rail, slide-out, Esc, Ctrl+B, outside click, pick, pin through reload, density, other style, narrow window passed");
} finally {
  await browser.close();
}
