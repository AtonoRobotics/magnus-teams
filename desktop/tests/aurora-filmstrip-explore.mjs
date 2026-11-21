// A filmstrip of the mockup's bento (website/, its showcase) beside the app's
// Explore in Aurora (decisions/2026-11-21-aurora-explore.md): the wide card
// and the pairs, the image zooming on hover, and, which the mockup has no
// moment for, a filter change moving the playbooks. Evidence, never a check.
//
//   MAGNUS_CHROMIUM=... node desktop/tests/aurora-filmstrip-explore.mjs OUT_DIR
//
// OUT_DIR gets filmstrip.png, mockup.mp4 and app.mp4 (filmstrip-kit.mjs).
// The app's page is the one aurora-explore.mjs checks (aurora-explore-page.mjs).
import { createRequire } from "node:module";
import { mkdirSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { screencast, sheet } from "./filmstrip-kit.mjs";
import { page, uiDir } from "./aurora-explore-page.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const exe = process.env.MAGNUS_CHROMIUM;
const out = process.argv[2];
if (!exe || !out) { console.error("MAGNUS_CHROMIUM and OUT_DIR are needed"); process.exit(2); }
mkdirSync(out, { recursive: true });

const NONE = "The mockup's bento has no filter: this moment is the app's own.";
const size = { width: 1280, height: 900 };
const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
/** @type {Buffer[]} */
const mockup = [], app = [];
try {
  // The mockup: its showcase bento, then a card hovered.
  const mctx = await browser.newContext({ viewport: size });
  const m = await mctx.newPage();
  await m.goto(pathToFileURL(join(root, "website/index.html")).href);
  await m.waitForTimeout(1500);
  await m.evaluate(() => document.getElementById("showcase").scrollIntoView({ block: "start" }));
  await m.waitForTimeout(600);
  const stopMockup = await screencast(mctx, m, out, "mockup");
  await m.waitForTimeout(300);
  mockup.push(await m.screenshot());
  await m.hover(".visual-card:not(.wide)");
  await m.waitForTimeout(900);
  mockup.push(await m.screenshot());
  await m.mouse.move(5, 5);
  await m.waitForTimeout(600);
  await stopMockup(null);
  await mctx.close();

  // The app: Explore, with its images, then a card hovered, then filters.
  const file = join(out, "explore.html");
  writeFileSync(file, page("full", "aurora", `${pathToFileURL(uiDir).href}/`));
  const actx = await browser.newContext({ viewport: size });
  const p = await actx.newPage();
  const stopApp = await screencast(actx, p, out, "app");
  p.on("pageerror", e => console.error(`app page error: ${e.message}`));
  await p.goto(pathToFileURL(file).href);
  await p.waitForSelector(".explore-grid > .explore-card");
  await p.evaluate(() => document.querySelector(".explore-controls").scrollIntoView({ block: "start" }));
  await p.waitForTimeout(900);
  app.push(await p.screenshot());
  await p.hover(".explore-grid > .explore-card:nth-child(2)");
  await p.waitForTimeout(900);
  app.push(await p.screenshot());
  await p.mouse.move(5, 5);
  const cat = await p.evaluate(() => Discover.PLAYBOOKS[1].cat);
  await p.waitForTimeout(400);
  await p.click(`[data-exp-cat="${cat}"]`);
  await p.waitForTimeout(160);
  app.push(await p.screenshot());
  await p.waitForTimeout(800);
  app.push(await p.screenshot());
  await p.click('[data-exp-cat="all"]');
  await p.waitForTimeout(200);
  app.push(await p.screenshot());
  await p.waitForTimeout(900);
  await stopApp(null);
  await actx.close();

  await sheet(browser, out, [
    { title: "The bento: one wide image card, then image cards in pairs", mockup: mockup[0], app: app[0] },
    { title: "A card hovered: its image zooms in a little", mockup: mockup[1], app: app[1] },
    { title: "A filter chosen: playbooks slide to their places, the rest fade out", mockup: null, app: app[2], note: NONE },
    { title: "Settled on the filter", mockup: null, app: app[3], note: NONE },
    { title: "Back to all: the others rise in", mockup: null, app: app[4], note: NONE },
  ]);
  console.log(`filmstrip written to ${out}`);
} finally {
  await browser.close();
}
