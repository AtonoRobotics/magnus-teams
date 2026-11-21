// Aurora's decisions and alerts are part of the stage, not over it
// (decisions/2026-11-21-aurora-alerts.md). In a real browser, with the app's
// theme and sheets, asks.js, notices.js and the Aurora scripts, on a page
// shaped like the app's:
// - a decision about this conversation opens beside it with its answers;
//   the thread keeps one line for it instead of the card, and it is not also
//   a box;
// - a decision from elsewhere is a box under the conversation, with its
//   answers in the box;
// - a teammate's message is a box, not a pop-up; opened, it goes;
// - answered beside the conversation, the card says the answer, the line
//   above the composer says what a toast would, and the canvas then closes
//   and the thread's line keeps the answer;
// - answered in its box, the box says the answer, then goes;
// - with no composer line to say it in, a toast is still a toast, and with
//   no conversation shown, a message is still a pop-up;
// - other styles keep their pop-ups and cards, as before.
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
try {
  const p = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  p.on("pageerror", e => failures.push(`page error: ${e.message}`));
  await p.setContent(page("full"));
  await p.waitForTimeout(200);

  // A decision here and one from elsewhere arrive.
  await p.evaluate(([a, b]) => { FX.pending = [a, b]; drawThread(); }, [ask("i1", "c1", "Book the Lisbon flight for $412?"), ask("i2", "c9", "Pay the $912 deposit?")]);
  await p.waitForTimeout(400);
  const first = await p.evaluate(() => {
    const thread = document.querySelector('#thread .card.item[data-id="i1"]');
    const line = document.querySelector("#thread .aurora-decision-line");
    const tiles = [...document.querySelectorAll(".stage-bento > .stage-tile:not(.stage-tile-leaving)")];
    const box = document.querySelector('.stage-tile[data-tile="approval:i2"]');
    return {
      opened: FX.opened,
      beside: !!document.querySelector('#detail .stage-artifact .card.item[data-id="i1"] .ask-answers button'),
      cardShown: thread ? getComputedStyle(thread).display !== "none" : null,
      line: line && { state: line.dataset.state, said: line.querySelector(".aurora-decision-said").textContent, what: line.querySelector(".aurora-decision-what").textContent, opens: line.querySelector(".aurora-decision-open")?.dataset.open },
      tiles: tiles.map(t => t.dataset.tile),
      box: box && { tag: box.tagName, answers: [...box.querySelectorAll(".ask-answers button")].map(b => b.textContent), title: box.querySelector("a.stage-tile-title")?.dataset.open },
    };
  });
  check("a decision about this conversation opens beside it, with its answers", () => assert.deepEqual([first.opened, first.beside], [["item:i1 (auto)"], true]));
  check("the thread keeps a line for it instead of the card", () => assert.deepEqual([first.cardShown, first.line], [false, { state: "waiting", said: "Needs your decision", what: "Book the Lisbon flight for $412?", opens: "item:i1" }]));
  check("the decision beside the conversation is not also a box", () => assert.ok(!first.tiles.includes("approval:i1"), JSON.stringify(first.tiles)));
  check("a decision from elsewhere is a box with its answers in it", () => assert.deepEqual(first.box, { tag: "DIV", answers: ["Allow", "Don't allow"], title: "item:i2" }));

  // A teammate writes: a box, not a pop-up.
  await p.evaluate(() => Notices.show({ key: "team:w1:n1", title: "Elena in #launch", body: "Can you check the deck?", system: "", href: "#/team/w1" }));
  await p.waitForTimeout(300);
  const note = await p.evaluate(() => ({ popups: document.querySelectorAll("#notices .notice").length, box: document.querySelector('.stage-tile[data-tile="messages"] .stage-tile-title')?.textContent }));
  check("a teammate's message is a box, not a pop-up", () => assert.deepEqual(note, { popups: 0, box: "Elena in #launch" }));
  await p.click('.stage-tile[data-tile="messages"]');
  await p.waitForTimeout(500);
  const noteGone = await p.evaluate(() => !document.querySelector('.stage-tile[data-tile="messages"]:not(.stage-tile-leaving)'));
  check("a message opened is no longer a box", () => assert.ok(noteGone));
  await p.evaluate(() => { location.hash = ""; });

  // Answered beside the conversation.
  await p.click('#detail .card.item[data-id="i1"] .ask-answers button.primary');
  await p.waitForTimeout(250);
  const beside = await p.evaluate(() => {
    const c = document.querySelector("#detail .stage-artifact .card.item");
    const told = document.querySelector(".aurora-say-state[data-told] .aurora-say-told");
    return { said: c?.querySelector(".aurora-confirmed-pill")?.textContent, buttons: c?.querySelectorAll("button").length, told: told && { text: told.textContent, role: told.getAttribute("role") }, toasts: document.querySelectorAll("#toasts .toast").length, open: Panel.what };
  });
  check("answered beside the conversation, the card says the answer where it was", () => assert.deepEqual([beside.said, beside.buttons, beside.open], ["✓ Allowed", 0, "item:i1"]));
  check("what a toast would say is said above the composer, not over the stage", () => assert.deepEqual([beside.told, beside.toasts], [{ text: "Allowed.", role: "status" }, 0]));
  await p.waitForTimeout(1000);
  const after = await p.evaluate(() => {
    const line = document.querySelector('#thread .aurora-decision-line[data-item="i1"]');
    return { open: Panel.what, line: line && { state: line.dataset.state, said: line.querySelector(".aurora-decision-said").textContent, review: !!line.querySelector(".aurora-decision-open") } };
  });
  check("then the canvas closes, and the thread's line keeps the answer", () => assert.deepEqual(after, { open: null, line: { state: "answered", said: "✓ Allowed", review: false } }));

  // Answered in its box.
  await p.click('.stage-tile[data-tile="approval:i2"] .ask-answers button.primary');
  await p.waitForTimeout(250);
  const inBox = await p.evaluate(() => { const b = document.querySelector('.stage-tile[data-tile="approval:i2"]:not(.stage-tile-leaving)'); return b && { said: b.querySelector(".aurora-confirmed-pill")?.textContent, buttons: b.querySelectorAll("button").length, tone: b.dataset.tone }; });
  await p.waitForTimeout(1400);
  const boxGone = await p.evaluate(() => !document.querySelector('.stage-tile[data-tile="approval:i2"]:not(.stage-tile-leaving)'));
  check("answered in its box, the box says the answer", () => assert.deepEqual(inBox, { said: "✓ Allowed", buttons: 0, tone: "done" }));
  check("then the box goes", () => assert.ok(boxGone));

  // No composer line to say it in: a toast is still a toast.
  await p.evaluate(() => { document.getElementById("composer").hidden = true; toast("Saved."); });
  const fallback = await p.evaluate(() => document.querySelectorAll("#toasts .toast").length);
  check("with no composer line shown, a toast is still a toast", () => assert.equal(fallback, 1));

  // Reduced motion: the same places, without the movement.
  const r = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  r.on("pageerror", e => failures.push(`reduced page error: ${e.message}`));
  await r.setContent(page("reduced"));
  await r.waitForTimeout(150);
  await r.evaluate(a => { FX.pending = [a]; drawThread(); }, ask("i3", "c1", "Send the invite?"));
  await r.waitForTimeout(300);
  const still = await r.evaluate(() => ({ beside: Panel.what, line: !!document.querySelector("#thread .aurora-decision-line") }));
  check("with reduced motion, decisions are in the same places", () => assert.deepEqual(still, { beside: "item:i3", line: true }));

  // A page with no conversation, so no stage: a message is a pop-up.
  await r.evaluate(() => { const c = document.querySelector("#main > section.conv"); const settings = document.createElement("section"); settings.textContent = "Settings"; c.replaceWith(settings); Notices.show({ key: "team:w1:n3", title: "Elena", body: "Hi", system: "", href: "#/team/w1" }); settings.replaceWith(c); });
  const noStage = await r.evaluate(() => document.querySelectorAll('#notices .notice[data-notice="team:w1:n3"]').length);
  check("with no conversation shown, a message is still a pop-up", () => assert.equal(noStage, 1));

  // Another style: pop-ups and cards as before.
  await r.evaluate(() => Theme.set({ style: "obsidian" }));
  await r.waitForTimeout(100);
  await r.evaluate(() => Notices.show({ key: "team:w1:n2", title: "Elena", body: "Hi", system: "", href: "#/team/w1" }));
  await r.waitForTimeout(100);
  const plain = await r.evaluate(() => ({ popups: document.querySelectorAll('#notices .notice[data-notice="team:w1:n2"]').length, card: getComputedStyle(document.querySelector('#thread .card.item[data-id="i3"]')).display !== "none", lines: document.querySelectorAll(".aurora-decision-line").length }));
  check("another style keeps its pop-ups and cards", () => assert.deepEqual(plain, { popups: 1, card: true, lines: 0 }));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
