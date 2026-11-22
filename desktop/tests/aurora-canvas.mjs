// A decision on Aurora's canvas is built around the decision
// (decisions/2026-11-21-aurora-canvas.md). In a real browser, on the page
// aurora-alerts.mjs checks, with a decision drawn as app.js's card() draws it:
// - its question is the canvas's title and name, what kind of decision it is
//   above it; no Back, only Close;
// - what it would mean is under the title; the app it uses and when and for
//   what it was asked are behind the one disclosure, shut at first;
// - its answers sit at the canvas's foot, and stay in reach there however
//   long its details are; answered, it says the answer there;
// - the card is still the decision's card for what reads it (the question
//   in it, its answers in it), and answering it still answers it;
// - an answered decision, and anything else on the canvas, is drawn as before.
//
// MAGNUS_CHROMIUM  Chrome to drive
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { page, ask } from "./aurora-alerts-page.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const { chromium } = createRequire(import.meta.url)(join(here, "../../browser/node_modules/playwright-core"));
const exe = process.env.MAGNUS_CHROMIUM;
if (!exe) { console.error("MAGNUS_CHROMIUM is not set"); process.exit(2); }

// app.js's card() for an open approval: its head (kind and tier; who waits,
// when, for which task), the app it uses, and the ask.
const card = `function card(i) {
  if (i.status === "answered") return '<div class="card flat item approval" data-id="' + esc(i.id) + '"><div class="spread"><span class="kind">Approval</span></div><p class="ask-sentence">' + esc(i.ask.sentence) + '</p><p class="small muted">Allowed by you</p></div>';
  return '<article class="card item approval" data-id="' + esc(i.id) + '"><div class="spread"><span class="kind">Approval · Cannot be undone</span>'
    + '<span class="small faint"><strong>A task is waiting</strong> · just now · <a href="#/task/t1" data-open="task:t1">task</a></span></div>'
    + '<div class="confirm chatcard conncard"><strong>Gmail</strong><span class="small">Sends as you</span></div>'
    + askCard(i, true) + '</article>';
}`;

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
/** @type {string[]} */
const failures = [];
const check = (/** @type {string} */ what, /** @type {() => void} */ fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${/** @type {Error} */ (e).message}`); console.log(`not ok - ${what}`); } };
try {
  const p = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  p.on("pageerror", e => failures.push(`page error: ${e.message}`));
  await p.setContent(page("off").replace("</body>", `<script>${card}</script></body>`));
  const sentence = "Send an email to sales@tubes.example asking for a quote on 40 tubes?";
  /** Draws the canvas for `what`, as app.js does. */
  const show = (/** @type {string} */ what, /** @type {string} */ back = "") => p.evaluate(([w, b]) => {
    Panel.what = w;
    const d = /** @type {HTMLElement} */ (document.getElementById("detail"));
    d.hidden = false; document.body.dataset.workspace = "split";
    const id = w.slice(5), i = FX.pending.find(x => x.id === id);
    d.innerHTML = w.startsWith("item:")
      ? AuroraStage.shell({ kind: "item", title: "Item", backBtn: b, body: i ? card(i) : '<p class="empty">This decision is gone.</p>' })
      : AuroraStage.shell({ kind: "task", title: "Plan the trip", head: null, backBtn: b, body: "<p>The plan</p>" });
  }, /** @type {const} */ ([what, back]));
  const open = { ...ask("i1", "c1", sentence), consequence: "It is sent as you and cannot be taken back.", ask: { sentence, offers: ["allow", "dont_allow"], content: { to: "sales@tubes.example", body: "Please quote 40 tubes." } } };
  await p.evaluate(i => { FX.pending = [i]; }, open);
  await show("item:i1", '<button type="button" class="back">← Back</button>');
  await p.waitForTimeout(200);

  const top = await p.evaluate(() => {
    const c = document.querySelector("#detail .stage-artifact");
    return { name: c?.getAttribute("aria-label"), title: c?.querySelector(".artifact-title h2")?.textContent, kind: c?.querySelector(".artifact-title .decision-kind")?.textContent, buttons: [...(c?.querySelectorAll(".artifact-top button") || [])].map(b => b.textContent) };
  });
  check("the question is the canvas's title and name", () => assert.deepEqual([top.name, top.title], [sentence, sentence]));
  check("what kind of decision it is stands above the question", () => assert.equal(top.kind, "Approval · Cannot be undone"));
  check("the canvas has Close and no Back", () => assert.deepEqual(top.buttons, ["Close"]));

  const body = await p.evaluate(() => {
    const c = /** @type {HTMLElement} */ (document.querySelector("#detail .stage-artifact"));
    const more = [...c.querySelectorAll("details")];
    const seen = (/** @type {Element|null} */ el) => !!el && /** @type {HTMLElement} */ (el).offsetParent !== null && el.getBoundingClientRect().height > 0;
    return {
      disclosures: more.length,
      shut: more.every(d => !d.open),
      inside: more[0] ? [!!more[0].querySelector(".conncard"), more[0].querySelector(".decision-meta")?.textContent?.includes("A task is waiting"), !!more[0].querySelector('a[data-open="task:t1"]')] : [],
      head: !!c.querySelector(".card.item > .spread"),
      consequence: seen(c.querySelector(".consequence")),
      sentenceInCard: c.querySelector('.card.item[data-id="i1"] .ask-sentence')?.textContent,
      sentenceSeen: seen(c.querySelector(".ask-sentence")),
      previewSeen: seen(c.querySelector(".decision-preview")),
      previewText: c.querySelector(".decision-preview")?.textContent || "",
      decision: !!c.querySelector(".card.item:has(.ask-answers)"),
    };
  });
  check("everything else is behind one disclosure, shut at first: the app, who waits, when, the task", () => assert.deepEqual([body.disclosures, body.shut, body.inside], [1, true, [true, true, true]]));
  check("the card's own head is gone from the canvas", () => assert.equal(body.head, false));
  check("what it would mean is seen under the title", () => assert.equal(body.consequence, true));
  check("the card keeps the question, unseen, for what reads it", () => assert.deepEqual([body.sentenceInCard, body.sentenceSeen], [sentence, false]));
  check("a clean read-only draft preview surfaces key fields above the disclosure", () => assert.ok(body.previewSeen && body.previewText.includes("sales@tubes.example") && body.previewText.includes("Please quote 40 tubes."), body.previewText));
  check("it is still the decision's card, its answers in it", () => assert.equal(body.decision, true));

  /** Where the answers are against the canvas's foot. */
  const foot = () => p.evaluate(() => {
    const c = /** @type {HTMLElement} */ (document.querySelector("#detail .stage-artifact")).getBoundingClientRect();
    const a = /** @type {HTMLElement} */ (document.querySelector("#detail .ask-answers")).getBoundingClientRect();
    return { gap: Math.round(c.bottom - a.bottom), inside: a.top >= c.top && a.bottom <= c.bottom + 0.5, primary: document.querySelector("#detail .ask-answers button.primary")?.textContent };
  });
  const short = await foot();
  check("its answers sit at the canvas's foot", () => assert.ok(short.inside && short.gap >= 0 && short.gap <= 32, JSON.stringify(short)));
  // Long details open: the answers are still at the foot, in reach.
  await p.evaluate(() => {
    const d = /** @type {HTMLDetailsElement} */ (document.querySelector("#detail details.ask-content"));
    d.insertAdjacentHTML("beforeend", "<pre>" + "A line of the email.\n".repeat(120) + "</pre>");
    d.open = true;
  });
  await p.waitForTimeout(100);
  const long = await foot();
  const scrolls = await p.evaluate(() => { const b = /** @type {HTMLElement} */ (document.querySelector("#detail .artifact-content")); return b.scrollHeight > b.clientHeight + 100; });
  check("with long details open, the answers stay at the foot, in reach", () => assert.ok(scrolls && long.inside && long.gap >= 0 && long.gap <= 32, JSON.stringify({ scrolls, long })));

  // Answering it still answers it.
  await p.click("#detail .ask-answers button.primary");
  await p.waitForTimeout(300);
  const after = await p.evaluate(() => ({ pending: FX.pending.length, said: document.querySelector("#detail .aurora-confirmed-pill")?.textContent?.startsWith("✓"), title: document.querySelector("#detail .artifact-title h2")?.textContent }));
  check("answering it answers it, and the canvas says so under the question", () => assert.deepEqual(after, { pending: 0, said: true, title: sentence }));

  // An answered decision, and anything else, are drawn as before.
  await p.evaluate(i => { FX.pending = [i]; }, { ...ask("i2", "c1", sentence), status: "answered" });
  await show("item:i2", '<button type="button" class="back">← Back</button>');
  const answered = await p.evaluate(() => ({ title: document.querySelector("#detail .artifact-title h2")?.textContent, back: !!document.querySelector("#detail .artifact-top .back"), decision: !!document.querySelector("#detail .stage-decision") }));
  check("an answered decision is drawn as before", () => assert.deepEqual(answered, { title: "Item", back: true, decision: false }));
  await show("task:t1", '<button type="button" class="back">← Back</button>');
  const task = await p.evaluate(() => ({ title: document.querySelector("#detail .artifact-title h2")?.textContent, back: !!document.querySelector("#detail .artifact-top .back"), decision: !!document.querySelector("#detail .stage-decision") }));
  check("anything else on the canvas is drawn as before", () => assert.deepEqual(task, { title: "Plan the trip", back: true, decision: false }));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
console.log("aurora canvas: all checks passed");
