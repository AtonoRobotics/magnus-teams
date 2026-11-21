// Magnus as an avatar (decisions/2026-11-21-aurora-avatar.md), in a real
// browser, on a page with the navigation's mark and a new conversation's
// heading, as the app draws them:
// - the style's avatar (the orb) is mounted in both, as a button named with
//   the state in words; the mark's slot is no longer hidden;
// - the avatar is replaceable: another one named for the style is mounted
//   in its place, the orb taken down, and it is told the state;
// - it follows the state: ready, listening, thinking, planning, working,
//   waiting on you, the person in control; done shows when a run is seen
//   ending well and then ready again; failed when one ends badly; a run
//   stopped by the person, or one already ended when first seen, is ready;
// - the orb's colour follows the state;
// - clicking it opens the decision waiting on the person, else Activity;
// - it leans toward the pointer;
// - it moves in full motion, is still with reduced motion, and stops
//   drawing while the window is hidden;
// - another style has no avatar.
//
// MAGNUS_CHROMIUM  Chrome to drive
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const read = (/** @type {string} */ n) => readFileSync(join(here, "../ui", n), "utf8");
const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const exe = process.env.MAGNUS_CHROMIUM;
if (!exe) { console.error("MAGNUS_CHROMIUM is not set"); process.exit(2); }

const html = `<!doctype html><html><head><style>${["base.css", "layout.css", "aurora.css", "aurora-rail.css"].map(read).join("\n")}</style></head>
<body data-navigation="expanded"><script>${read("theme.js")}</script><script>Theme.set({ style: "aurora", mode: "dark", motion: "full" });</script>
<script>var FX = { opened: [], needs: [] }; function openPanel(what, o) { FX.opened.push(what); } function needsMe() { return FX.needs; }</script>
<div class="shell"><nav class="side"><div class="brand"><span class="logo" aria-hidden="true"><svg width="20" height="20"></svg></span><span class="brand-name">Magnus</span></div></nav>
<main id="main"><div class="hello-head"><h1 class="hello-title">What should Magnus do?</h1></div><div id="thread"></div></main></div>
<script>${read("aurora-avatar.js")}</script><script>${read("aurora-orb.js")}</script></body></html>`;

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
/** @type {string[]} */
const failures = [];
const check = (/** @type {string} */ what, /** @type {() => void} */ fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${/** @type {Error} */ (e).message}`); console.log(`not ok - ${what}`); } };
try {
  const p = await browser.newPage({ viewport: { width: 1000, height: 700 } });
  p.on("pageerror", e => failures.push(`page error: ${e.message}`));
  await p.setContent(html);
  await p.waitForTimeout(600);
  const tick = () => p.waitForTimeout(500);
  /** The avatars' state and names. */
  const seen = () => p.evaluate(() => [...document.querySelectorAll(".aurora-avatar")].map(b => ({ state: /** @type {HTMLElement} */ (b).dataset.state, name: b.getAttribute("aria-label"), canvas: !!b.querySelector("canvas") })));
  /** Puts the page's state and the last answer's pill as given. */
  const put = (/** @type {string} */ state, /** @type {Record<string, string>|null} */ pill) => p.evaluate(([s, a]) => {
    document.documentElement.dataset.auroraState = s;
    const t = document.getElementById("thread");
    let el = /** @type {HTMLElement|null} */ (t?.querySelector(".aurora-reason") || null);
    if (!a) { el?.remove(); return; }
    if (!el) { el = document.createElement("div"); el.className = "aurora-reason"; t?.append(el); }
    for (const k of ["phase", "outcome"]) { if (a[k]) el.dataset[k] = a[k]; else delete el.dataset[k]; }
    el.toggleAttribute("data-planned", !!a.planned);
  }, /** @type {const} */ ([state, pill]));
  /** The orb's colour at its middle, in the hero. */
  const colour = () => p.evaluate(() => {
    const c = /** @type {HTMLCanvasElement} */ (document.querySelector(".aurora-avatar-hero canvas")), x = c.getContext("2d");
    const d = x?.getImageData(Math.round(c.width * 0.42), Math.round(c.height * 0.42), 1, 1).data;
    return d ? [d[0], d[1], d[2]] : [];
  });
  const snapshot = () => p.evaluate(() => /** @type {HTMLCanvasElement} */ (document.querySelector(".aurora-avatar-hero canvas")).toDataURL());

  let a = await seen();
  check("the orb is mounted in the navigation and in a new conversation", () => assert.deepEqual(a.map(x => [x.state, x.canvas]), [["ready", true], ["ready", true]]));
  check("the avatar is a button named with the state in words", () => assert.equal(a[0].name, "Magnus is ready. Show what Magnus is doing"));
  const hidden = await p.evaluate(() => document.querySelector("nav.side .brand .logo")?.getAttribute("aria-hidden"));
  check("the navigation's slot is no longer aria-hidden", () => assert.equal(hidden, null));
  const names = await p.evaluate(() => [Theme.avatar("aurora"), Theme.avatar("obsidian-aurora"), Theme.avatar("obsidian")]);
  check("Aurora and Obsidian 2 Aurora name the orb; others none", () => assert.deepEqual(names, ["orb", "orb", null]));

  // States.
  const states = [];
  for (const [s, pill, want] of /** @type {Array<[string, any, string]>} */ ([
    ["listening", null, "listening"], ["thinking", null, "thinking"], ["idle", { phase: "thinking" }, "thinking"],
    ["idle", { phase: "working", planned: "1" }, "planning"], ["executing", { phase: "working" }, "working"],
    ["needs", { phase: "needs" }, "waiting"], ["takeover", null, "control"], ["idle", null, "ready"],
  ])) {
    await put(s, pill);
    await tick();
    const got = (await seen())[1];
    states.push([want, got.state, got.name]);
  }
  check("it follows each state", () => assert.deepEqual(states.map(x => x[1]), states.map(x => x[0])));
  check("its name says each state", () => assert.ok(states.every(x => x[2]?.startsWith("Magnus is ")) && states.find(x => x[0] === "waiting")?.[2] === "Magnus is waiting on you. Show what Magnus is doing", JSON.stringify(states)));

  // Colour follows the state.
  await put("idle", null); await p.waitForTimeout(1200);
  const ready = await colour();
  await put("needs", { phase: "needs" }); await p.waitForTimeout(1200);
  const waiting = await colour();
  check("ready is blue and waiting is amber", () => assert.ok(ready[2] > ready[0] && waiting[0] > waiting[2], JSON.stringify({ ready, waiting })));

  // A run's end.
  await put("executing", { phase: "working" }); await tick();
  await put("idle", { phase: "done" }); await tick();
  const done = (await seen())[1].state;
  await p.waitForTimeout(4200);
  const after = (await seen())[1].state;
  check("done shows when a run ends well, then ready", () => assert.deepEqual([done, after], ["done", "ready"]));
  await put("executing", { phase: "working" }); await tick();
  await put("idle", { phase: "stopped", outcome: "failed" }); await tick();
  const failed = (await seen())[1].state;
  await put("executing", { phase: "working" }); await tick();
  await put("idle", { phase: "stopped", outcome: "cancelled" }); await tick();
  const cancelled = (await seen())[1].state;
  check("failed shows when a run ends badly; one the person stopped is ready", () => assert.deepEqual([failed, cancelled], ["failed", "ready"]));
  await put("idle", null); await tick();
  await p.evaluate(() => { const el = document.createElement("div"); el.className = "aurora-reason"; el.dataset.phase = "done"; document.getElementById("thread")?.append(el); });
  await tick();
  const earlier = (await seen())[1].state;
  check("an answer ended before it was seen leaves the avatar ready", () => assert.equal(earlier, "ready"));
  await put("idle", null);

  // Click.
  await p.click(".aurora-avatar-hero");
  await p.evaluate(() => { FX.needs = [{ id: "i1" }]; });
  await p.click(".aurora-avatar-nav");
  const opened = await p.evaluate(() => ({ panels: FX.opened, hash: location.hash }));
  check("clicking it shows Activity, or the decision waiting on the person", () => assert.deepEqual(opened, { panels: ["item:i1"], hash: "#/activity" }));
  await p.evaluate(() => { FX.needs = []; });

  // Motion: moving in full, still when reduced, stopped while hidden.
  await tick();
  const m1 = await snapshot(); await p.waitForTimeout(300); const m2 = await snapshot();
  check("in full motion it moves", () => assert.notEqual(m1, m2));
  await p.evaluate(() => Theme.set({ motion: "reduced" })); await p.waitForTimeout(800);
  const r1 = await snapshot(); await p.waitForTimeout(400); const r2 = await snapshot();
  check("with reduced motion it is still", () => assert.equal(r1, r2));
  await put("executing", { phase: "working" }); await tick();
  const r3 = await snapshot();
  check("with reduced motion it is drawn again for a new state", () => assert.notEqual(r2, r3));
  await p.evaluate(() => Theme.set({ motion: "full" })); await tick();
  await p.evaluate(() => { Object.defineProperty(document, "hidden", { value: true, configurable: true }); document.dispatchEvent(new Event("visibilitychange")); });
  await p.waitForTimeout(200);
  const h1 = await snapshot(); await p.waitForTimeout(400); const h2 = await snapshot();
  check("while the window is hidden it does not draw", () => assert.equal(h1, h2));
  await p.evaluate(() => { Object.defineProperty(document, "hidden", { value: false, configurable: true }); document.dispatchEvent(new Event("visibilitychange")); });
  await put("idle", null); await tick();

  // Replaceable, and it leans toward the pointer.
  await p.evaluate(() => {
    /** @type {any} */ (window).calls = [];
    const calls = /** @type {any} */ (window).calls;
    AuroraAvatar.define("test", (host, o) => { calls.push(["mount", o.size]); const el = document.createElement("i"); el.className = "test-avatar"; host.append(el); return { set: s => calls.push(["set", s]), look: (x, y) => calls.push(["look", Math.sign(x), Math.sign(y)]), destroy: () => { calls.push(["destroy"]); el.remove(); } }; });
    Theme.avatar = () => "test";
    dispatchEvent(new CustomEvent("magnus-theme"));
  });
  await tick();
  const box = await p.locator(".aurora-avatar-hero").boundingBox();
  if (box) await p.mouse.move(box.x - 60, box.y + box.height + 60);
  const swapped = await p.evaluate(() => ({ calls: /** @type {any} */ (window).calls, canvases: document.querySelectorAll(".aurora-avatar canvas").length, tests: document.querySelectorAll(".aurora-avatar .test-avatar").length }));
  check("another avatar named for the style takes the orb's place", () => assert.deepEqual([swapped.canvases, swapped.tests], [0, 2]));
  check("it is mounted at each place's size and told the state", () => assert.deepEqual(swapped.calls.filter((/** @type {any[]} */ c) => c[0] !== "look").slice(0, 4), [["mount", 28], ["set", "ready"], ["mount", 72], ["set", "ready"]]));
  check("it is told where the pointer is", () => assert.ok(swapped.calls.some((/** @type {any[]} */ c) => c[0] === "look" && c[1] === -1 && c[2] === 1), JSON.stringify(swapped.calls)));

  // Another style.
  await p.evaluate(() => { Theme.avatar = s => (s === "aurora" || s === "obsidian-aurora" ? "orb" : null); Theme.set({ style: "obsidian" }); });
  await tick();
  const gone = await p.evaluate(() => ({ avatars: document.querySelectorAll(".aurora-avatar").length, mark: !!document.querySelector("nav.side .brand .logo > svg") }));
  check("another style has no avatar, and keeps its mark", () => assert.deepEqual(gone, { avatars: 0, mark: true }));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
console.log("aurora avatar: all checks passed");
