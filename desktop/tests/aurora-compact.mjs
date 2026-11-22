// Aurora, compact (decisions/2026-11-21-aurora-compact.md), in a real
// browser, with the app's sheets, theme.js, chat-composer.js's composer and
// app.js's own model picker:
// - scrollbars are thin, unseen until what they scroll is pointed at, then
//   in the theme's colours; other styles keep the system's;
// - the model and the reasoning level are one chip saying both; it opens a
//   menu above it with the two, shuts on Esc (the focus back on the chip) or
//   a click elsewhere, and says a new choice; the choices are still the
//   form's, as app.js reads them to send; other styles show the two menus;
// - the mic: with voice input on the server, what is said is recorded and
//   the server writes it into the message, even where the browser has its
//   own recognition (so it stays on the server); without it, the browser's
//   own; with neither, no mic.
//
// MAGNUS_CHROMIUM  Chrome to drive
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const read = (/** @type {string} */ n) => readFileSync(join(here, "../ui", n), "utf8");
const { chromium } = createRequire(import.meta.url)(join(here, "../../browser/node_modules/playwright-core"));
const exe = process.env.MAGNUS_CHROMIUM;
if (!exe) { console.error("MAGNUS_CHROMIUM is not set"); process.exit(2); }

// app.js's model picker, as it is.
const app = read("app.js");
const picker = app.match(/const REASONING_LEVELS[^]*?\nasync function modelPicker\(\) \{[^]*?\n\}\n/)?.[0];
if (!picker) { console.error("app.js's modelPicker was not found"); process.exit(1); }

const stubs = `
var UI = {};
var App = { me: { id: "p1", voice: false } };
var FX = { said: [], toasts: [] };
var Server = {
  models: { list: async () => ({ default: { adapter: "anthropic", model: "claude-sonnet" }, adapters: [{ adapter: "anthropic", models: ["claude-sonnet", "claude-haiku"] }] }) },
  transcribe: async b64 => { FX.said.push(b64); return { text: "book the hotel" }; },
};
function $(q, r) { return (r || document).querySelector(q); }
function esc(s) { return String(s ?? "").replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]); }
function icon(k) { return '<svg class="ic" data-icon="' + k + '"></svg>'; }
function attachButton() { return ""; }
function toast(m) { FX.toasts.push(m); }
function load(k, d) { return d; }
function save() {}`;

const html = `<!doctype html><html><head><style>${["base.css", "layout.css", "chat.css", "aurora.css", "aurora-compact.css"].map(read).join("\n")}
.scroller { height: 120px; width: 300px; overflow: auto; }</style></head>
<body><script>${read("theme.js")}</script><script>Theme.set({ style: "aurora", mode: "dark", motion: "reduced" });</script>
<div class="scroller" id="scroller"><div style="height:900px">Lines</div></div><div id="away" style="height:60px">Elsewhere</div>
<div id="composer" style="margin-top:260px;width:640px"></div>
<script>${stubs}</script><script>${picker}</script><script>${read("chat-composer.js")}</script><script>${read("aurora-compact.js")}</script></body></html>`;

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
/** @type {string[]} */
const failures = [];
const check = (/** @type {string} */ what, /** @type {() => void} */ fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${/** @type {Error} */ (e).message}`); console.log(`not ok - ${what}`); } };
try {
  const p = await browser.newPage({ viewport: { width: 1000, height: 800 } });
  p.on("pageerror", e => failures.push(`page error: ${e.message}`));
  await p.setContent(html);
  /** Draws the composer as app.js does, with its model picker. */
  const draw = () => p.evaluate(async () => { document.getElementById("composer").innerHTML = composer("Reply", await modelPicker()); await new Promise(r => setTimeout(r, 50)); });
  await draw();

  // ---------- Scrollbars ----------
  const bar = () => p.evaluate(() => { const s = getComputedStyle(document.getElementById("scroller")); return { width: s.scrollbarWidth, colour: s.scrollbarColor }; });
  await p.mouse.move(500, 120);
  const idle = await bar();
  await p.hover("#scroller");
  const over = await bar();
  check("scrollbars are thin and unseen until what they scroll is pointed at", () => assert.deepEqual([idle.width, /^transparent|rgba\(0, 0, 0, 0\)/.test(idle.colour)], ["thin", true], JSON.stringify(idle)));
  check("pointed at, the scrollbar shows in the theme's colours", () => assert.ok(over.width === "thin" && !/^(transparent|rgba\(0, 0, 0, 0\)) /.test(over.colour) && over.colour !== "auto", JSON.stringify(over)));

  // ---------- The chip ----------
  const chip = () => p.evaluate(() => {
    const b = document.querySelector(".say-bar");
    const c = /** @type {?HTMLDetailsElement} */ (b?.querySelector(":scope > .model-chip") || null);
    const menu = c?.querySelector(".model-chip-menu"), sum = c?.querySelector("summary");
    const m = menu?.getBoundingClientRect(), s = sum?.getBoundingClientRect();
    return {
      chips: b?.querySelectorAll(":scope > .model-chip").length, labels: b?.querySelectorAll(":scope > label.model").length,
      inside: menu?.querySelectorAll(":scope > label.model").length, words: c?.querySelector(".model-chip-words")?.textContent,
      name: sum?.getAttribute("aria-label"), open: !!c?.open, shown: !!m && m.height > 0 && menu?.checkVisibility(),
      above: !!m && !!s && m.bottom <= s.top, focus: document.activeElement === sum,
      reasoning: /** @type {?HTMLSelectElement} */ (document.querySelector("form.say select[data-reasoning]"))?.value,
    };
  });
  let c = await chip();
  check("the model and reasoning are one chip saying both", () => assert.deepEqual([c.chips, c.labels, c.inside, c.words], [1, 0, 2, "claude-sonnet · Auto"]));
  check("the chip is named for both, as a control to change them", () => assert.equal(c.name, "Model claude-sonnet, reasoning Auto. Change"));
  check("its menu is shut at first", () => assert.deepEqual([c.open, c.shown], [false, false]));
  await p.click(".model-chip > summary");
  c = await chip();
  check("clicked, its menu opens above it with the two choices", () => assert.deepEqual([c.open, c.shown, c.above], [true, true, true]));
  await p.selectOption("select[data-reasoning]", "high");
  c = await chip();
  check("a new choice is said on the chip and is still the form's", () => assert.deepEqual([c.words, c.reasoning], ["claude-sonnet · High", "high"]));
  await p.keyboard.press("Escape");
  c = await chip();
  check("Esc shuts it and gives the focus back to the chip", () => assert.deepEqual([c.open, c.focus], [false, true]));
  await p.click(".model-chip > summary");
  await p.click("#away");
  c = await chip();
  check("a click elsewhere shuts it", () => assert.equal(c.open, false));
  await p.evaluate(() => Theme.set({ style: "obsidian" }));
  await p.waitForTimeout(50);
  c = await chip();
  const other = await bar();
  check("another style shows the two menus, and the system's scrollbars", () => assert.deepEqual([c.chips, c.labels, c.reasoning, other.width], [0, 2, "high", "auto"]));
  await p.evaluate(() => Theme.set({ style: "aurora" }));
  await p.waitForTimeout(50);
  c = await chip();
  check("back in Aurora it is one chip again, saying the choice", () => assert.deepEqual([c.chips, c.words], [1, "claude-sonnet · High"]));

  // ---------- The mic ----------
  /** Wires the composer's mic as app.js does, the browser and server as given. */
  const mic = (/** @type {{ voice: boolean, recognition: boolean }} */ o) => p.evaluate(async ({ voice, recognition }) => {
    App.me.voice = voice;
    FX.recognised = 0;
    const w = /** @type {any} */ (window);
    w.SpeechRecognition = undefined;
    w.webkitSpeechRecognition = recognition ? class { start() { FX.recognised++; } stop() {} } : undefined;
    Object.defineProperty(navigator, "mediaDevices", { configurable: true, value: { getUserMedia: async () => ({ getTracks: () => [{ stop() {} }] }) } });
    w.MediaRecorder = class {
      constructor() { this.state = "inactive"; this.mimeType = "audio/webm"; }
      start() { this.state = "recording"; FX.recording = true; }
      stop() { this.state = "inactive"; this.ondataavailable({ data: new Blob(["sound"], { type: "audio/webm" }) }); this.onstop(); }
    };
    document.getElementById("composer").innerHTML = composer("Reply", "");
    const form = /** @type {HTMLFormElement} */ (document.querySelector("form.say"));
    const box = /** @type {HTMLTextAreaElement} */ (form.querySelector("textarea"));
    box.value = "Please";
    wireVoice(form, box);
    const b = /** @type {HTMLButtonElement} */ (form.querySelector("[data-voice-in]"));
    return { hidden: b.hidden };
  }, o);
  const said = () => p.evaluate(() => ({ text: /** @type {HTMLTextAreaElement} */ (document.querySelector("form.say textarea")).value, sent: FX.said.length, b64: FX.said[0] || "", recognised: FX.recognised, pressed: document.querySelector("[data-voice-in]")?.getAttribute("aria-pressed") }));

  let m = await mic({ voice: true, recognition: true });
  await p.click("[data-voice-in]");
  await p.waitForTimeout(50);
  const recording = await said();
  await p.click("[data-voice-in]");
  await p.waitForTimeout(200);
  const written = await said();
  check("with voice on the server the mic records, and shows it is listening", () => assert.deepEqual([m.hidden, recording.pressed, recording.recognised], [false, "true", 0]));
  check("the server writes down what was said, into the message", () => assert.deepEqual([written.text, written.sent, written.b64 === btoa("sound"), written.pressed], ["Please book the hotel", 1, true, "false"]));
  m = await mic({ voice: false, recognition: true });
  await p.click("[data-voice-in]");
  const own = await said();
  check("without it, the browser's own recognition is used", () => assert.deepEqual([m.hidden, own.recognised, own.sent], [false, 1, 1]));
  m = await mic({ voice: false, recognition: false });
  check("with neither, there is no mic", () => assert.equal(m.hidden, true));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
console.log("aurora compact: all checks passed");
