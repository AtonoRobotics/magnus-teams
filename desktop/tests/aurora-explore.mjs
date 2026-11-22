// Aurora's Explore is the mockup's bento (decisions/2026-11-21-aurora-explore.md).
// In a real browser, with the app's theme and sheets, discover.js and
// aurora-explore.js:
// - the first playbook is a wide card across the grid, about 21:9; the rest
//   are 4:3 cards in pairs;
// - each card's image fills it, and zooms in a little on hover;
// - a filter change moves: playbooks still shown slide to their places, ones
//   leaving fade out where they were and are then gone, ones arriving rise in;
// - with reduced motion the bento is the same and nothing moves;
// - other styles keep their grid of equal cards.
//
// MAGNUS_CHROMIUM  Chrome to drive
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { page } from "./aurora-explore-page.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const exe = process.env.MAGNUS_CHROMIUM;
if (!exe) { console.error("MAGNUS_CHROMIUM is not set"); process.exit(2); }

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
const failures = [];
const check = (what, fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${e.message}`); console.log(`not ok - ${what}`); } };
const shape = () => {
  const grid = document.querySelector(".explore-grid").getBoundingClientRect();
  return [...document.querySelectorAll(".explore-grid > .explore-card:not(.explore-leaving)")].map(c => {
    const r = c.getBoundingClientRect(), m = c.querySelector(".card-media").getBoundingClientRect();
    return { id: c.dataset.playbook, full: Math.abs(r.width - grid.width) < 2, ratio: Math.round((r.width / r.height) * 100) / 100, top: Math.round(r.top), fills: r.width - m.width <= 2.5 && r.height - m.height <= 2.5 };
  });
};
try {
  const p = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
  p.on("pageerror", e => failures.push(`page error: ${e.message}`));
  await p.setContent(page("full"));
  await p.waitForSelector(".explore-grid > .explore-card");
  const s = await p.evaluate(shape);
  check("the first playbook is a wide card across the grid, about 21:9", () => assert.deepEqual([s[0].full, s[0].ratio >= 2.2 && s[0].ratio <= 2.4], [true, true], JSON.stringify(s[0])));
  check("the rest are 4:3 cards in pairs", () => assert.ok(s.length > 3 && s.slice(1).every(c => !c.full && c.ratio >= 1.28 && c.ratio <= 1.38) && s[1].top === s[2].top, JSON.stringify(s.slice(1, 4))));
  check("each card's image fills it", () => assert.ok(s.every(c => c.fills)));

  await p.hover(".explore-grid > .explore-card:nth-child(2)");
  await p.waitForTimeout(900);
  const zoom = await p.evaluate(() => new DOMMatrix(getComputedStyle(document.querySelector(".explore-grid > .explore-card:nth-child(2) .card-media img")).transform).a);
  check("its image zooms in a little on hover", () => assert.ok(zoom > 1.01 && zoom < 1.05, String(zoom)));
  await p.mouse.move(5, 5);

  // A filter: some stay and slide, some leave, the first becomes another.
  const cats = await p.evaluate(() => Discover.PLAYBOOKS.map(x => x.cat));
  const cat = cats[1];
  await p.click(`[data-exp-cat="${cat}"]`);
  await p.waitForTimeout(120);
  const mid = await p.evaluate(() => ({
    leaving: document.querySelectorAll(".explore-grid > .explore-card.explore-leaving").length,
    leavingHidden: [...document.querySelectorAll(".explore-card.explore-leaving")].every(c => c.inert && c.getAttribute("aria-hidden") === "true"),
    moving: document.querySelectorAll(".explore-grid > .explore-card:not(.explore-leaving)").length && [...document.querySelectorAll(".explore-grid > .explore-card:not(.explore-leaving)")].filter(c => c.getAnimations().some(a => a.playState === "running")).length,
  }));
  await p.waitForTimeout(800);
  const settled = await p.evaluate(() => ({ leaving: document.querySelectorAll(".explore-card.explore-leaving").length, running: document.getAnimations().filter(a => a.playState === "running" && a.effect?.target?.closest?.(".explore-grid")).length, ids: [...document.querySelectorAll(".explore-grid > .explore-card")].map(c => c.dataset.playbook) }));
  const want = await p.evaluate(c => Discover.PLAYBOOKS.filter(x => x.cat === c).map(x => x.id), cat);
  check("a filter change moves the playbooks to their places over time", () => assert.ok(mid.moving > 0, JSON.stringify(mid)));
  check("playbooks the filter takes away fade out where they were, out of reach", () => assert.ok(mid.leaving > 0 && mid.leavingHidden, JSON.stringify(mid)));
  check("then only the filter's playbooks are left, still", () => assert.deepEqual(settled, { leaving: 0, running: 0, ids: want }));
  const after = await p.evaluate(shape);
  check("the filter's first playbook is the wide card", () => assert.ok(after[0].full));

  // Back to all: the others arrive.
  await p.click('[data-exp-cat="all"]');
  await p.waitForTimeout(100);
  const arriving = await p.evaluate(ids => [...document.querySelectorAll(".explore-grid > .explore-card")].filter(c => !ids.includes(c.dataset.playbook) && c.getAnimations().length).length, want);
  check("playbooks coming back rise in", () => assert.ok(arriving > 0));

  // Reduced motion: the same bento, nothing moves.
  const r = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
  r.on("pageerror", e => failures.push(`reduced page error: ${e.message}`));
  await r.setContent(page("reduced"));
  await r.waitForSelector(".explore-grid > .explore-card");
  await r.click(`[data-exp-cat="${cat}"]`);
  await r.waitForTimeout(50);
  const still = await r.evaluate(() => ({ wide: (() => { const g = document.querySelector(".explore-grid").getBoundingClientRect(); return Math.abs(document.querySelector(".explore-card").getBoundingClientRect().width - g.width) < 2; })(), moving: document.getAnimations().length, leaving: document.querySelectorAll(".explore-leaving").length }));
  check("with reduced motion the bento is the same and nothing moves", () => assert.deepEqual(still, { wide: true, moving: 0, leaving: 0 }));

  // Another style: equal cards.
  const o = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
  await o.setContent(page("full", "obsidian"));
  await o.waitForSelector(".explore-grid > .explore-card");
  const plain = await o.evaluate(shape);
  check("another style keeps its grid of equal cards", () => assert.ok(!plain[0].full && plain[0].top === plain[1].top));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
