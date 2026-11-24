// An answer shows as given only once the server took it
// (decisions/2026-11-23-answer-shown-once-taken.md):
// - app.js's respond() tells "magnus:answered" after the server accepted the
//   answer, and never when it refused it (answered by someone else first,
//   expired, the network);
// - the thread's line for a decision reads "✓ Allowed" only for an answer
//   told so, not for a tap (aurora-alerts.js): Mo's "Don't allow" taken just
//   before Sam's "Allow" leaves no "✓ Allowed" on Sam's thread;
// - an approval box says the answer only once it is told (aurora-stage.js).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const read = f => readFileSync(join(here, "../ui", f), "utf8");

/** A fake element, as much of one as these scripts touch. */
class El {
  constructor(className = "", attrs = {}) {
    this.className = className;
    this.attrs = { ...attrs };
    this.dataset = {};
    this.children = [];
    /** @type {?El} */
    this.parentElement = null;
    this.textContent = "";
    /** Set on a page's root: it and what is in it are on the page. */
    this.root = false;
    /** What closest(selector) answers, by selector. */
    this.near = {};
    /** @type {?string} what outerHTML was set to */
    this.replacedWith = null;
  }
  get isConnected() { return this.root || !!this.parentElement?.isConnected; }
  get previousElementSibling() { const p = this.parentElement; return p ? p.children[p.children.indexOf(this) - 1] || null : null; }
  get nextElementSibling() { const p = this.parentElement; return p ? p.children[p.children.indexOf(this) + 1] || null : null; }
  /** Children from markup: each tag with a class, and the text in it. */
  set innerHTML(markup) {
    this.children = [...markup.matchAll(/<(\w+) class="([^"]+)"[^>]*>([^<]*)/g)].map(([, , cls, text]) => {
      const e = new El(cls);
      e.textContent = text;
      e.parentElement = this;
      return e;
    });
  }
  set outerHTML(markup) { this.replacedWith = markup; }
  get outerHTML() { return this.replacedWith || ""; }
  querySelector(sel) {
    for (const s of sel.split(",").map(x => x.trim())) {
      if (!/^\.[\w-]+$/.test(s)) continue;
      const found = this.children.find(c => c.className.split(" ").includes(s.slice(1)));
      if (found) return found;
    }
    return null;
  }
  querySelectorAll() { return []; }
  closest(sel) { return this.near[sel] ?? null; }
  getAttribute(a) { return this.attrs[a] ?? null; }
  setAttribute(a, v) { this.attrs[a] = String(v); }
  removeAttribute(a) { delete this.attrs[a]; }
  remove() {
    const p = this.parentElement;
    if (p) p.children = p.children.filter(c => c !== this);
    this.parentElement = null;
  }
  append(n) { n.remove(); n.parentElement = this; this.children.push(n); }
  before(n) {
    const p = this.parentElement;
    if (!p) return;
    n.remove();
    n.parentElement = p;
    p.children.splice(p.children.indexOf(this), 0, n);
  }
}

const IN_THREAD = "section.conv .card.item[data-id]";
const DECISION = ".card.item:has(.ask-answers)";

/** A page with aurora-alerts.js and aurora-stage.js on it, in the Aurora style. */
function page() {
  /** @type {Array<{ type: string, fn: Function, capture: boolean }>} */
  const listeners = [];
  /** @type {?Function} */
  let onDom = null;
  const state = { /** @type {El[]} */ cards: [], /** @type {?El} */ entry: null };
  const document = {
    readyState: "complete",
    documentElement: { dataset: { style: "aurora", motion: "off" } },
    body: new El(),
    addEventListener(type, fn, capture) { listeners.push({ type, fn, capture: !!capture }); },
    dispatchEvent(ev) {
      const mine = listeners.filter(l => l.type === ev.type);
      for (const l of [...mine.filter(l => l.capture), ...mine.filter(l => !l.capture)]) {
        l.fn(ev);
        if (ev.stopped) break;
      }
      return true;
    },
    createElement() { return new El(); },
    getElementById() { return null; },
    querySelectorAll(sel) { return sel === IN_THREAD ? state.cards.filter(c => c.isConnected) : []; },
    querySelector(sel) {
      const card = sel.startsWith(`${IN_THREAD}[data-id="`) && sel.slice(IN_THREAD.length + 10, -2);
      if (card) return state.cards.find(c => c.dataset.id === card && c.isConnected) || null;
      const entry = sel.match(/^\[data-entry="([^"]+)"\]$/);
      if (entry) return state.entry?.attrs["data-entry"] === entry[1] ? state.entry : null;
      return null;
    },
  };
  class Observer { observe() {} disconnect() {} }
  const ctx = {
    document,
    window: { addEventListener() {} },
    location: { hash: "#/" },
    Theme: { stage: s => s === "aurora" },
    AuroraCore: { onDom: fn => { onDom = fn; } },
    AuroraBoxes: { isClosed: () => false, sign: () => "" },
    MutationObserver: Observer,
    ResizeObserver: Observer,
    HTMLElement: El,
    CSS: { escape: s => String(s) },
    requestAnimationFrame: () => 1,
    setTimeout: () => 0,
    Date, Map, Set, WeakMap, WeakSet, JSON, Math,
  };
  ctx.globalThis = ctx;
  vm.createContext(ctx);
  vm.runInContext(`${read("aurora-alerts.js")}\nthis.AuroraAlerts = AuroraAlerts;`, ctx);
  vm.runInContext(read("aurora-stage.js"), ctx);
  const event = (type, o) => ({ type, preventDefault() {}, stopImmediatePropagation() { this.stopped = true; }, ...o });
  return {
    alerts: ctx.AuroraAlerts,
    state,
    /** The page changed: the scripts look again. */
    changed: () => onDom?.([{ addedNodes: [1], removedNodes: [], target: {} }]),
    /** The person taps a button. @param {El} target */
    tap: target => document.dispatchEvent(event("click", { target })),
    /** The app tells that the server took an answer. */
    answered: detail => document.dispatchEvent(event("magnus:answered", { detail })),
  };
}

/** A decision's card in a conversation, its Allow button and its entry. @param {string} id */
function decision(id) {
  const entry = new El("entry", { "data-entry": "a:t1" });
  entry.root = true;
  const card = new El("card item approval");
  card.dataset.id = id;
  card.near = { "[data-entry]": entry };
  card.innerHTML = '<p class="ask-sentence">Book the flight?</p><div class="ask-answers"></div>';
  entry.append(card);
  const allow = new El("primary");
  allow.dataset.item = id;
  allow.dataset.decision = "approve:once";
  allow.near = { ".ask-answers button[data-decision]": allow, "button[data-item]": allow, [DECISION]: card };
  card.querySelector(".ask-answers")?.append(allow);
  return { entry, card, allow };
}
const said = (/** @type {El} */ line) => line.querySelector(".aurora-decision-said")?.textContent;

// 1. respond() tells the answer only once the server took it.
{
  const app = read("app.js");
  const start = app.indexOf("async function respond(btn, root) {");
  assert.ok(start >= 0, "app.js has respond(btn, root)");
  const src = app.slice(start, app.indexOf("\n}\n", start) + 2);
  /** @param {() => Promise<unknown>} answer */
  const run = async answer => {
    const told = [], toasts = [];
    class CustomEvent { constructor(type, o) { this.type = type; this.detail = o?.detail; } }
    const ctx = {
      Server: { inbox: { answer } }, CustomEvent, CSS: { escape: s => String(s) },
      document: { dispatchEvent: ev => { told.push(ev); return true; } },
      $: () => null, drafts: {}, toast: m => toasts.push(m), refresh: async () => {},
    };
    vm.createContext(ctx);
    vm.runInContext(`${src}\nthis.respond = respond;`, ctx);
    const btn = { disabled: false, dataset: { item: "i1", decision: "approve:once" } };
    await ctx.respond(btn, null);
    return { told, toasts, btn };
  };
  const refused = await run(async () => { throw new Error("Someone else already answered this."); });
  assert.equal(refused.told.length, 0, "a refused answer was told as taken");
  assert.deepEqual(refused.toasts, ["Someone else already answered this."], "the refusal was not said");
  assert.equal(refused.btn.disabled, false, "a refused answer left its button off");
  const taken = await run(async () => ({}));
  assert.equal(taken.told.length, 1, "a taken answer was not told");
  assert.equal(taken.told[0].type, "magnus:answered");
  assert.equal(taken.told[0].detail.item, "i1");
  assert.equal(taken.told[0].detail.decision, "approve", "the answer told is the decision, not its scope");
  assert.equal(taken.told[0].detail.button, taken.btn, "the answer told names the button it came from");
}

// 2. In a shared project, Mo's "Don't allow" is taken just before Sam taps
// "Allow": Sam's answer is refused, the card goes, and Sam's thread line
// does not read "✓ Allowed". An answer the server took does.
{
  const p = page();
  const mo = decision("i1");
  p.state.cards = [mo.card];
  p.state.entry = mo.entry;
  p.changed();
  const line = p.alerts.lines.get("i1")?.el;
  assert.ok(line, "the thread keeps a line for the decision");
  assert.equal(said(line), "Needs your decision");
  p.tap(mo.allow);
  // The server refused Sam's answer; the redraw takes the card away.
  mo.card.remove();
  p.changed();
  assert.notEqual(said(line), "✓ Allowed", "a refused answer reads as allowed in the thread");
  assert.equal(p.alerts.given.has("i1"), false, "a tap was taken as an answer before the server took it");
  assert.equal(p.alerts.lines.has("i1"), false, "the line for a decision answered elsewhere stayed");
  assert.equal(line.isConnected, false, "the line for a decision answered elsewhere stayed on the page");

  const sam = decision("i2");
  p.state.cards = [sam.card];
  p.state.entry = sam.entry;
  p.changed();
  p.tap(sam.allow);
  p.answered({ item: "i2", decision: "approve", button: sam.allow });
  sam.card.remove();
  p.changed();
  const taken = p.alerts.lines.get("i2")?.el;
  assert.ok(taken?.isConnected, "the line for an answer taken left the thread");
  assert.equal(said(taken), "✓ Allowed", "an answer taken does not read as given");
}

// 3. An approval box says the answer only once the server took it.
{
  const p = page();
  const box = () => {
    const tile = new El("stage-tile");
    tile.root = true;
    tile.dataset.tile = "approval:i3";
    tile.dataset.tone = "needs";
    tile.innerHTML = '<a class="stage-tile-title">Send the report?</a><div class="ask-answers"></div>';
    const allow = new El("primary");
    allow.dataset.item = "i3";
    allow.dataset.decision = "approve:once";
    allow.near = { ".stage-tile .ask-answers button[data-decision]": allow, ".ask-answers button[data-decision]": allow, ".stage-tile": tile, "button[data-item]": allow };
    tile.querySelector(".ask-answers")?.append(allow);
    return { tile, allow, row: /** @type {El} */ (tile.querySelector(".ask-answers")) };
  };
  const refused = box();
  p.tap(refused.allow);
  assert.equal(refused.tile.dataset.tone, "needs", "a tapped box was marked answered before the server took it");
  assert.equal(refused.row.replacedWith, null, "a tapped box said its answer before the server took it");

  const taken = box();
  p.tap(taken.allow);
  p.answered({ item: "i3", decision: "approve", button: taken.allow });
  assert.equal(taken.tile.dataset.tone, "done", "a box whose answer was taken was not marked answered");
  assert.match(taken.row.replacedWith || "", /✓ Allowed/, "a box whose answer was taken does not say it");
}

console.log("answer shown check passed");
