// Aurora's sizes move (decisions/2026-11-21-aurora-sizes.md). In a real
// browser, on the page aurora-alerts.mjs checks (aurora-alerts-page.mjs):
// - a box arriving under the conversation grows the bento over time, and
//   the conversation shrinks with it, instead of both jumping in one frame;
// - a conversation read to its end stays at its end while it shrinks;
// - a box that keeps its place rides with the bento, without a slide of
//   its own;
// - the last box going shrinks the bento away over time, and the
//   conversation grows back;
// - with reduced motion, sizes change at once.
//
// MAGNUS_CHROMIUM  Chrome to drive
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { page, ask } from "./aurora-alerts-page.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const exe = process.env.MAGNUS_CHROMIUM;
if (!exe) { console.error("MAGNUS_CHROMIUM is not set"); process.exit(2); }

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const failures = [];
const check = (what, fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${e.message}`); console.log(`not ok - ${what}`); } };
const conv = () => Math.round(document.querySelector("#main > section.conv").getBoundingClientRect().height);
// A long conversation, read to its end.
const long = () => {
  const t = document.getElementById("thread");
  t.innerHTML = Array.from({ length: 40 }, (_, i) => `<div class="entry me" data-entry="m${i}" data-kind="message"><div class="msg me">Message ${i}</div></div>`).join("");
  t.scrollTop = t.scrollHeight;
};
const atEnd = () => { const t = document.getElementById("thread"); return t.scrollHeight - t.scrollTop - t.clientHeight < 4; };
try {
  const p = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  p.on("pageerror", e => failures.push(`page error: ${e.message}`));
  await p.setContent(page("full"));
  await p.waitForTimeout(300);
  await p.evaluate(long);
  await p.waitForTimeout(200);
  const before = await p.evaluate(conv);

  // A decision from elsewhere: its box arrives.
  await p.evaluate(a => { FX.pending = [a]; AuroraStage.refresh(); }, ask("i2", "c9", "Pay the $912 venue deposit?"));
  await p.waitForTimeout(120);
  const mid = await p.evaluate(conv);
  const midEnd = await p.evaluate(atEnd);
  await p.waitForTimeout(700);
  const after = await p.evaluate(conv);
  check("a box arriving shrinks the conversation over time, not at once", () => assert.ok(before > mid && mid > after, JSON.stringify({ before, mid, after })));
  check("a conversation read to its end stays at its end while it shrinks", () => assert.ok(midEnd));
  const endAfter = await p.evaluate(atEnd);
  check("and is at its end once it has shrunk", () => assert.ok(endAfter));

  // A second box: the first keeps its place in the bento and rides with it.
  await p.evaluate(() => Notices.show({ key: "team:w1:n1", title: "Elena in #launch", body: "Can you check the deck?", system: "", href: "#/team/w1" }));
  await p.waitForTimeout(120);
  const rides = await p.evaluate(() => {
    const b = document.querySelector('.stage-tile[data-tile="approval:i2"]');
    return { slid: b.getAnimations().some(a => a.playState === "running" && String(a.effect?.getKeyframes?.()[0]?.transform || "").startsWith("translate")), grow: document.querySelector(".stage-bento").getAnimations().some(a => a.id === "stage-grow") };
  });
  await p.waitForTimeout(700);
  check("a box that keeps its place rides with the bento, with no slide of its own", () => assert.deepEqual(rides.slid, false, JSON.stringify(rides)));

  // Every box goes: the bento shrinks away, the conversation grows back.
  const full = await p.evaluate(conv);
  await p.evaluate(() => { FX.pending = []; AuroraStage.notes.clear(); AuroraStage.refresh(); });
  await p.waitForTimeout(120);
  const back = await p.evaluate(conv);
  await p.waitForTimeout(800);
  const whole = await p.evaluate(conv);
  check("the last box going grows the conversation back over time", () => assert.ok(full < back && back < whole && Math.abs(whole - before) <= 2, JSON.stringify({ full, back, whole, before })));
  check("its room closes after the box has faded, not before", () => assert.ok(back - full < (whole - full) / 2, JSON.stringify({ full, back, whole })));

  // Reduced motion: at once.
  const r = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  r.on("pageerror", e => failures.push(`reduced page error: ${e.message}`));
  await r.setContent(page("reduced"));
  await r.waitForTimeout(300);
  const rb = await r.evaluate(conv);
  await r.evaluate(a => { FX.pending = [a]; AuroraStage.refresh(); }, ask("i2", "c9", "Pay the $912 venue deposit?"));
  await r.waitForTimeout(80);
  const ra = await r.evaluate(conv);
  await r.waitForTimeout(600);
  const rz = await r.evaluate(conv);
  check("with reduced motion, sizes change at once", () => assert.ok(ra < rb && ra === rz, JSON.stringify({ rb, ra, rz })));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
