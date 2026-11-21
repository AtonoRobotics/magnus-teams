// A filmstrip of Aurora's sizes moving (decisions/2026-11-21-aurora-sizes.md):
// the stage before this change beside it, at the same moments, as boxes come
// under a conversation read to its end and go again. The mockup's
// conversation never changes size, so it has no such moment. Evidence,
// never a check.
//
//   MAGNUS_CHROMIUM=... node desktop/tests/aurora-filmstrip-sizes.mjs OUT_DIR BEFORE_REF
//
// BEFORE_REF is the commit whose aurora-stage.js and .css are filmed on the
// left. OUT_DIR gets filmstrip.png, before.mp4 and after.mp4
// (filmstrip-kit.mjs). The page is the one aurora-sizes.mjs checks
// (aurora-alerts-page.mjs).
import { createRequire } from "node:module";
import { mkdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { screencast, sheet } from "./filmstrip-kit.mjs";
import { page, ask } from "./aurora-alerts-page.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const exe = process.env.MAGNUS_CHROMIUM;
const [out, ref] = process.argv.slice(2);
if (!exe || !out || !ref) { console.error("MAGNUS_CHROMIUM, OUT_DIR and BEFORE_REF are needed"); process.exit(2); }
mkdirSync(out, { recursive: true });
const before = Object.fromEntries(["aurora-stage.js", "aurora-stage.css"].map(f => [f, execFileSync("git", ["-C", root, "show", `${ref}:desktop/ui/${f}`], { encoding: "utf8" })]));

// The run (ms from its start), and the moments shot in it.
const RUN = [
  [0, "arrive"], [90, "shot"], [200, "shot"], [900, "shot"],
  [1300, "message"], [1420, "shot"], [2200, "shot"],
  [2600, "clear"], [2720, "shot"], [3600, "shot"],
];
// Each row's title, given how long after its change the shot was taken
// (a shot takes time, so it is measured, not assumed).
const ROWS = [
  ms => `A decision from elsewhere arrives: ${ms} ms in`,
  ms => `${ms} ms in: the conversation gives up its room as the box rises`,
  () => "Settled: the conversation is still at its last message",
  ms => `A teammate's message, ${ms} ms in: the box already there rides with the bento`,
  () => "Settled with two boxes",
  ms => `Nothing waits on you, ${ms} ms in: the bento folds away`,
  () => "Settled: the conversation has its whole height back",
];

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const size = { width: 1440, height: 900 };
/** @param {string} name @param {Record<string, string>} files */
async function film(name, files) {
  const ctx = await browser.newContext({ viewport: size });
  const p = await ctx.newPage();
  p.on("pageerror", e => console.error(`${name} page error: ${e.message}`));
  await p.setContent(page("full", "particles", files));
  await p.evaluate(() => {
    const t = document.getElementById("thread");
    const say = ["Can you plan the Lisbon offsite for the first week of March?", "On it. I'll find flights, a venue near Alfama and a dinner for twelve.", "Keep the flights under $450 a person.", "Found a TAP flight at $412 and the Palácio venue at $912 for the deposit."];
    t.innerHTML = Array.from({ length: 16 }, (_, i) => `<div class="entry ${i % 2 ? "magnus" : "me"}" data-entry="m${i}" data-kind="message"><div class="msg ${i % 2 ? "magnus" : "me"}">${say[i % say.length]}</div></div>`).join("");
    t.scrollTop = t.scrollHeight;
  });
  await p.waitForTimeout(700);
  const stop = await screencast(ctx, p, out, name);
  const shots = [];
  const start = Date.now();
  let changed = 0;
  const run = {
    arrive: () => p.evaluate(a => { FX.pending = [a]; AuroraStage.refresh(); }, ask("i2", "c9", "Pay the $912 venue deposit?")),
    message: () => p.evaluate(() => Notices.show({ key: "team:w1:n1", title: "Elena in #launch", body: "Can you check the Northstar deck before 11?", system: "", href: "#/team/w1" })),
    clear: () => p.evaluate(() => { FX.pending = []; AuroraStage.notes.clear(); AuroraStage.refresh(); }),
    shot: async () => { const at = await p.evaluate(() => performance.now()); shots.push({ png: await p.screenshot({ clip: { x: 0, y: 0, ...size } }), at }); },
  };
  for (const [ms, what] of RUN) {
    await p.waitForTimeout(Math.max(0, ms - (Date.now() - start)));
    if (what !== "shot") changed = await p.evaluate(() => performance.now());
    await run[what]();
    if (what === "shot") shots[shots.length - 1].ms = Math.round(shots[shots.length - 1].at - changed);
  }
  await p.waitForTimeout(400);
  await stop(null);
  await ctx.close();
  return shots;
}
try {
  const was = await film("before", before);
  const now = await film("after", {});
  await sheet(browser, out, ROWS.map((title, i) => ({ title: `${title(now[i].ms)} (before: ${was[i].ms} ms)`, mockup: was[i].png, app: now[i].png })), [`Before (${ref})`, "After: sizes move"]);
  console.log(`filmstrip written to ${out}`);
} finally {
  await browser.close();
}
