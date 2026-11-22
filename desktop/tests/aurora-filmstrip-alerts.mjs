// A filmstrip of the mockup (website/) beside the app in Aurora for
// decisions and alerts (decisions/2026-11-21-aurora-alerts.md): the mockup's
// drafts to approve in its canvas beside the app's decision opening beside
// the conversation, both answered in place; and the app's other alerts on its
// stage, which the mockup has no moment for: a decision from elsewhere in its
// box, a teammate's message as a box. Evidence, never a check.
//
//   MAGNUS_CHROMIUM=... node desktop/tests/aurora-filmstrip-alerts.mjs OUT_DIR
//
// OUT_DIR gets filmstrip.png, mockup.mp4 and app.mp4 (filmstrip-kit.mjs).
// The app's page is the one aurora-alerts.mjs checks (aurora-alerts-page.mjs).
import { createRequire } from "node:module";
import { mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { screencast, sheet } from "./filmstrip-kit.mjs";
import { page, ask } from "./aurora-alerts-page.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const exe = process.env.MAGNUS_CHROMIUM;
const out = process.argv[2];
if (!exe || !out) { console.error("MAGNUS_CHROMIUM and OUT_DIR are needed"); process.exit(2); }
mkdirSync(out, { recursive: true });

const NONE = "The mockup has no such moment: its only alert is the approval in its canvas.";
// The app's run (ms from its start), and the moments shot in it.
const APP = [
  [0, "arrive"], [900, "shot"], [1500, "message"], [2200, "shot"],
  [2700, "allow"], [3000, "shot"], [4100, "shot"],
  [4700, "allowBox"], [4950, "shot"], [6300, "shot"],
];
// Each row: its title, and which of the mockup's frames goes beside it.
const ROWS = [
  ["A decision about this conversation slides in beside it; one from elsewhere is a box", 0],
  ["A teammate's message is a box on the stage, not a pop-up", null],
  ["Allowed: the answer shows where it was; the composer's line says so", 1],
  ["The canvas slides away; the thread's line keeps the answer", 2],
  ["The decision from elsewhere, answered in its box", null],
  ["Settled: nothing waits on you, nothing is left over", 3],
];

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const size = { width: 1440, height: 900 };
/** @type {Buffer[]} */
const mockup = [], app = [];
try {
  // The mockup: its inbox run ends on the drafts to approve; approve is clicked.
  const mctx = await browser.newContext({ viewport: { width: 1280, height: 1500 } });
  const m = await mctx.newPage();
  await m.goto(`file://${join(root, "website/index.html")}`);
  await m.waitForTimeout(4500);
  const clip = await m.evaluate(() => {
    const b = document.getElementById("workspace-stage").getBoundingClientRect();
    const x = Math.max(0, b.left - 24), y = Math.max(0, b.top - 24);
    return { x, y, width: Math.min(innerWidth - x, b.width + 48), height: Math.min(innerHeight - y, b.height + 48) };
  });
  const stopMockup = await screencast(mctx, m, out, "mockup");
  await m.waitForTimeout(400);
  mockup.push(await m.screenshot({ clip }));
  await m.click("#artifact-action-btn");
  const clicked = Date.now();
  for (const at of [300, 1100, 2000]) {
    await m.waitForTimeout(Math.max(0, at - (Date.now() - clicked)));
    mockup.push(await m.screenshot({ clip }));
  }
  await stopMockup(clip);
  await mctx.close();

  // The app: the stage as decisions and a message come and are answered.
  const actx = await browser.newContext({ viewport: size });
  const p = await actx.newPage();
  const stopApp = await screencast(actx, p, out, "app");
  p.on("pageerror", e => console.error(`app page error: ${e.message}`));
  await p.setContent(page("full", "particles"));
  await p.evaluate(() => drawThread());
  await p.waitForTimeout(800);
  const start = Date.now();
  const run = {
    arrive: () => p.evaluate(([a, b]) => { FX.pending = [a, b]; drawThread(); }, [ask("i1", "c1", "Book the Lisbon flight for $412?"), ask("i2", "c9", "Pay the $912 venue deposit?")]),
    message: () => p.evaluate(() => Notices.show({ key: "team:w1:n1", title: "Elena in #launch", body: "Can you check the Northstar deck before 11?", system: "", href: "#/team/w1" })),
    allow: () => p.click('#detail .card.item[data-id="i1"] .ask-answers button.primary'),
    allowBox: () => p.click('.stage-tile[data-tile="approval:i2"] .ask-answers button.primary'),
    shot: async () => { app.push(await p.screenshot({ clip: { x: 0, y: 0, ...size } })); },
  };
  for (const [ms, what] of APP) { await p.waitForTimeout(Math.max(0, ms - (Date.now() - start))); await run[what](); }
  await p.waitForTimeout(600);
  await stopApp(null);
  await actx.close();

  await sheet(browser, out, ROWS.map(([title, m], i) => ({ title, mockup: m === null ? null : mockup[m], app: app[i], note: NONE })));
  console.log(`filmstrip written to ${out}`);
} finally {
  await browser.close();
}
