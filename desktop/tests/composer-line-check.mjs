// The line above the composer speaks for its own conversation
// (decisions/2026-11-28-ui-without-ceremony.md). It read the whole app's
// state, so with a task running anywhere it said "Working across your
// apps…" under a conversation whose answer was in (installed app, test.427:
// "pong", answered in 3s). Work elsewhere has its own card and tile.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const src = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../ui/aurora-reasoning.js"), "utf8");

/** A small element: its classes and attributes, matched by simple selectors. */
const el = (/** @type {string} */ cls, /** @type {Record<string, string>} */ attrs = {}) => ({ cls: new Set(cls.split(" ").filter(Boolean)), attrs, dataset: Object.fromEntries(Object.entries(attrs).filter(([k]) => k.startsWith("data-")).map(([k, v]) => [k.slice(5).replace(/-(.)/g, (_, c) => c.toUpperCase()), v])), hasAttribute: function (/** @type {string} */ a) { return a in this.attrs; } });
/** @param {string} s */
const split = s => { const out = []; let d = 0, cur = ""; for (const c of s) { if (c === "(") d++; if (c === ")") d--; if (c === "," && !d) { out.push(cur.trim()); cur = ""; } else cur += c; } if (cur.trim()) out.push(cur.trim()); return out; };
/** @param {any} e @param {string} sel @returns {boolean} */
const matches = (e, sel) => split(sel).some(one => {
  let rest = one;
  while (rest) {
    let m;
    if ((m = /^\.([\w-]+)/.exec(rest))) { if (!e.cls.has(m[1])) return false; }
    else if ((m = /^\[([\w-]+)="([^"]*)"\]/.exec(rest))) { if (e.attrs[m[1]] !== m[2]) return false; }
    else if ((m = /^\[([\w-]+)\]/.exec(rest))) { if (!(m[1] in e.attrs)) return false; }
    else if ((m = /^:is\(((?:[^()]|\([^()]*\))*)\)/.exec(rest))) { if (!matches(e, m[1])) return false; }
    else throw new Error(`selector not understood: ${rest}`);
    rest = rest.slice(m[0].length);
  }
  return true;
});
const node = (/** @type {any[]} */ kids) => ({ kids, querySelector: (/** @type {string} */ s) => kids.find(k => matches(k, s)) || null, querySelectorAll: (/** @type {string} */ s) => kids.filter(k => matches(k, s)) });

let thread = node([]);
const root = { dataset: /** @type {Record<string, string>} */ ({}) };
const document = { readyState: "loading", documentElement: root, addEventListener() {}, getElementById: (/** @type {string} */ id) => (id === "thread" ? thread : null), querySelectorAll: () => [] };
const sandbox = { document, window: { addEventListener() {} }, setTimeout, clearTimeout, setInterval, clearInterval, requestAnimationFrame: () => 0 };
vm.createContext(sandbox);
vm.runInContext(`${src}; this.AuroraReasoning = AuroraReasoning;`, sandbox);
const state = () => sandbox.AuroraReasoning.threadState();

// A running task elsewhere makes the app "working"; this conversation, its
// answer in, stays at rest.
root.dataset.auroraAvatarState = "working";
thread = node([el("aurora-reason", { "data-phase": "done" })]);
assert.equal(state(), "ready", "work elsewhere says this answered conversation is working");
// Its own answer at work, its own task card running, its own ask waiting.
thread = node([el("aurora-reason", { "data-phase": "working" })]);
assert.equal(state(), "working");
thread = node([el("aurora-reason", { "data-phase": "working", "data-planned": "" })]);
assert.equal(state(), "planning");
thread = node([el("aurora-reason", { "data-phase": "done" }), el("tcard", { "data-status": "running" })]);
assert.equal(state(), "working", "this conversation's running task is not said");
thread = node([el("aurora-reason", { "data-phase": "thinking" })]);
assert.equal(state(), "thinking");
thread = node([el("st need")]);
assert.equal(state(), "waiting");
// The person's own states hold wherever they are.
root.dataset.auroraAvatarState = "control";
thread = node([]);
assert.equal(state(), "control");
root.dataset.auroraAvatarState = "listening";
assert.equal(state(), "listening");
// The words for work say no more than is known.
assert.doesNotMatch(src, /Working across your apps/, "the line claims work across apps it does not know of");
// Its look follows its own state, not the app's.
const css = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../ui/nova-widgets.css"), "utf8") + readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../ui/nova.css"), "utf8");
assert.doesNotMatch(css, /data-aurora-(avatar-)?state[^{]*\.aurora-say-state/, "the line's look follows the app's state");
console.log("composer-line-check: ok");
