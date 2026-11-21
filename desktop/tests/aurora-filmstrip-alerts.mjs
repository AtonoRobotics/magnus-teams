// A filmstrip of the mockup (website/) beside the app in Aurora for alerts:
// the mockup's approve button turning to "✓ Sent & verified" beside the
// app's decision answered in place, and the app's pop-ups coming and going,
// which the mockup has no equivalent of. Evidence, never a check.
//
//   MAGNUS_CHROMIUM=... node desktop/tests/aurora-filmstrip-alerts.mjs OUT_DIR
//
// OUT_DIR gets filmstrip.png, mockup.mp4 and app.mp4 (filmstrip-kit.mjs).
import { createRequire } from "node:module";
import { readFileSync, mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { screencast, sheet } from "./filmstrip-kit.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const exe = process.env.MAGNUS_CHROMIUM;
const out = process.argv[2];
if (!exe || !out) { console.error("MAGNUS_CHROMIUM and OUT_DIR are needed"); process.exit(2); }
mkdirSync(out, { recursive: true });

const NO_POPUPS = "The mockup has no pop-ups: its alerts are the approve button and the presence line.";
const sheets = ["base.css", "layout.css", "chat.css", "teams.css", "aurora.css", "aurora-widgets.css", "aurora-stage.css"].map(ui).join("\n");
const stubs = `
function $(q, r) { return (r || document).querySelector(q); }
function esc(s) { return String(s ?? "").replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]); }
function icon() { return "×"; }
var Magnus = { kind: "desktop", notifyPermission: () => "granted" };
var App = { me: { id: "p1" }, thread: "c1", tasks: [], schedules: [], connectors: [], computers: [] };
var Panel = { what: null };
function toast() {}`;
const card = (id, title, sentence) => `<article class="card item approval" data-id="${id}"><div class="spread"><strong>${title}</strong><span class="small faint">just now</span></div><div class="ask"><p class="ask-sentence">${sentence}</p><div class="row ask-answers"><button class="primary" data-item="${id}" data-decision="approve">Allow</button><button data-item="${id}" data-decision="reject">Decline</button></div></div></article>`;
const CARDS = {
  i1: card("i1", "Send 6 drafts", "Send the 6 staged replies, including the two renewal follow-ups?"),
  i2: card("i2", "Book Helios kickoff", "Hold Tue 10 AM on your calendar for the Helios kickoff?"),
};
const thread = ids => `<div class="entry me" data-entry="m1" data-kind="message"><div class="msg me">Clear my morning inbox and draft replies for urgent clients</div></div>
  <div class="entry agent" data-entry="a:t4" data-kind="tasks">${ids.map(id => CARDS[id]).join("")}</div>`;
const appPage = `<!doctype html><html><head><style>${sheets}
body { margin: 0; height: 100vh; } .shell { display: block; height: 100vh; padding: 24px; box-sizing: border-box; }
#main { max-width: 860px; height: calc(100vh - 48px); margin: 0 auto; display: flex; flex-direction: column; }
section.conv { display: flex; flex-direction: column; height: 100%; } .thread { flex: 1; overflow: auto; padding: 20px 28px; display: flex; flex-direction: column; gap: 14px; }</style></head>
<body><script>${ui("theme.js")}</script><script>Theme.set({ style: "aurora", mode: "dark", motion: "full", background: "particles" });</script>
<div class="shell"><main id="main"><section class="conv"><header class="conv-head"><div class="conv-title"><h1>Morning inbox</h1></div><div class="row"></div></header>
<div class="thread" id="thread">${thread(["i1"])}</div>
</section></main></div>
<script>${stubs}</script><script>${ui("notices.js")}</script><script>${ui("aurora.js")}</script><script>${ui("aurora-alerts.js")}</script></body></html>`;

// The app's run (ms from its start), and the moments shot in it.
const APP = [
  [0, "arrive"], [350, "shot"], [2600, "shot"], [2800, "dismiss"], [2870, "shot"],
  [3600, "answer"], [3900, "shot"], [4700, "shot"], [5600, "shot"],
];
const ROWS = [
  ["A decision arrives, and two pop-ups", true],
  ["Each pop-up shows the time it has left", false],
  ["A pop-up dismissed slides out; the other glides down", false],
  ["Allowed: the answer shows where it was", true],
  ["It folds away", true],
  ["Settled", true],
];

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const size = { width: 1280, height: 900 };
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

  // The app: a decision arriving beside one there, two pop-ups, one
  // dismissed, then the first decision allowed.
  const actx = await browser.newContext({ viewport: size });
  const p = await actx.newPage();
  const stopApp = await screencast(actx, p, out, "app");
  p.on("pageerror", e => console.error(`app page error: ${e.message}`));
  await p.setContent(appPage);
  await p.waitForTimeout(800);
  const start = Date.now();
  const run = {
    arrive: () => p.evaluate(t => {
      document.getElementById("thread").innerHTML = t;
      Notices.popup({ key: "team:w1:n1", title: "Elena in #launch", body: "Can you check the Northstar deck before 11?", system: "", href: "" });
      setTimeout(() => Notices.popup({ key: "inbox:i2", title: "Magnus needs you", body: "Hold Tue 10 AM for the Helios kickoff?", system: "", href: "" }), 150);
    }, thread(["i1", "i2"])),
    dismiss: () => p.click('[data-notice="inbox:i2"] .notice-close'),
    answer: async () => {
      await p.evaluate(t => {
        document.querySelector('[data-id="i1"] [data-decision="approve"]').addEventListener("click", () => setTimeout(() => { document.getElementById("thread").innerHTML = t; }, 80));
      }, thread(["i2"]));
      await p.click('[data-id="i1"] [data-decision="approve"]');
    },
    shot: async () => { app.push(await p.screenshot({ clip: { x: 0, y: 0, ...size } })); },
  };
  for (const [ms, what] of APP) { await p.waitForTimeout(Math.max(0, ms - (Date.now() - start))); await run[what](); }
  await p.waitForTimeout(600);
  await stopApp(null);
  await actx.close();

  let mi = 0;
  await sheet(browser, out, ROWS.map(([title, has], i) => ({ title, mockup: has ? mockup[mi++] : null, app: app[i], note: NO_POPUPS })));
  console.log(`filmstrip written to ${out}`);
} finally {
  await browser.close();
}
