// The page's scripts load in index.html's order without a ReferenceError.
// Each <script src> runs in one shared context, as in the window: a name a
// script uses while it loads (a table of views, a call at its top level)
// must be defined by that script or an earlier one. A function declared in a
// later script does not exist yet, and the error stops the rest of that
// script: on 0.2.6-test.380 app-panel.js's PANELS named filesView from
// app-library.js, PANELS was never defined, and the installed app showed an
// empty window.
//
// The DOM is a stand-in that answers anything, so this checks names, not
// what the page draws (desktop/tests/nova-screens.mjs drives the real one).
// Timers, frames and events never fire: only load-time code runs.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const ui = join(dirname(fileURLToPath(import.meta.url)), "../ui");
const index = readFileSync(join(ui, "index.html"), "utf8");
const scripts = [...index.matchAll(/<script\b[^>]*\bsrc="([^"]+)"[^>]*><\/script>/g)].map(m => m[1]);
assert.ok(scripts.length > 20, `index.html loads its scripts (found ${scripts.length})`);
assert.ok(!/<script\b[^>]*type="module"/.test(index), "the page's scripts are classic scripts, which share one global scope");

/** A value that answers every use: any property, call, construction or iteration. */
function anything() {
  const target = function () {};
  /** @type {ProxyHandler<any>} */
  const handler = {
    get(_t, key) {
      if (key === Symbol.toPrimitive) return (/** @type {string} */ hint) => (hint === "number" ? 0 : "");
      if (key === Symbol.iterator) return function* () {};
      if (key === Symbol.asyncIterator) return async function* () {};
      if (key === "then") return undefined; // not a promise
      if (key === "length" || key === "size") return 0;
      return proxy;
    },
    set: () => true,
    has: () => true,
    deleteProperty: () => true,
    apply: () => proxy,
    construct: () => proxy,
  };
  const proxy = new Proxy(target, handler);
  return proxy;
}

const any = anything();
const storage = () => {
  /** @type {Map<string, string>} */
  const m = new Map();
  return { getItem: (/** @type {string} */ k) => (m.has(k) ? m.get(k) : null), setItem: (/** @type {string} */ k, /** @type {string} */ v) => m.set(k, String(v)), removeItem: (/** @type {string} */ k) => m.delete(k), clear: () => m.clear(), key: () => null, length: 0 };
};
const never = () => 0;
// The browser's globals the scripts may touch while loading. A name missing
// here and in the scripts is a ReferenceError, as it would be in the window.
const browser = {
  document: any, navigator: any, history: any, screen: any, performance: any, crypto: any, CSS: any,
  customElements: any, indexedDB: any, caches: any, speechSynthesis: any, visualViewport: any,
  location: { href: "http://127.0.0.1/", origin: "http://127.0.0.1", protocol: "http:", host: "127.0.0.1", hostname: "127.0.0.1", port: "", pathname: "/", search: "", hash: "", assign: never, replace: never, reload: never },
  localStorage: storage(), sessionStorage: storage(),
  setTimeout: never, clearTimeout: never, setInterval: never, clearInterval: never,
  requestAnimationFrame: never, cancelAnimationFrame: never, requestIdleCallback: never, cancelIdleCallback: never,
  queueMicrotask: never,
  addEventListener: never, removeEventListener: never, dispatchEvent: () => true,
  matchMedia: () => ({ matches: false, media: "", addEventListener: never, removeEventListener: never, addListener: never, removeListener: never }),
  getComputedStyle: () => any, getSelection: () => any, scrollTo: never, scrollBy: never,
  alert: never, confirm: () => false, prompt: () => null, open: () => null, close: never, print: never, focus: never, blur: never,
  fetch: () => new Promise(() => {}),
  innerWidth: 1344, innerHeight: 860, devicePixelRatio: 1, scrollX: 0, scrollY: 0, isSecureContext: true, name: "",
  HTMLElement: any, HTMLDialogElement: any, HTMLInputElement: any, HTMLTextAreaElement: any, HTMLButtonElement: any,
  HTMLAnchorElement: any, HTMLImageElement: any, HTMLCanvasElement: any, HTMLIFrameElement: any, HTMLSelectElement: any,
  HTMLFormElement: any, HTMLVideoElement: any, HTMLAudioElement: any, HTMLTemplateElement: any, SVGElement: any,
  Element: any, Node: any, NodeFilter: any, Text: any, DocumentFragment: any, Range: any, ShadowRoot: any,
  Event: any, CustomEvent: any, KeyboardEvent: any, MouseEvent: any, PointerEvent: any, FocusEvent: any, InputEvent: any,
  DOMParser: any, XMLSerializer: any, MutationObserver: any, ResizeObserver: any, IntersectionObserver: any,
  EventSource: any, WebSocket: any, BroadcastChannel: any, Worker: any, MessageChannel: any,
  Image: any, Audio: any, Notification: any, SpeechSynthesisUtterance: any, MediaRecorder: any,
  Blob: any, File: any, FileReader: any, FormData: any, Headers: any, Request: any, Response: any,
  ClipboardItem: any, DataTransfer: any, AudioContext: any, OffscreenCanvas: any, Path2D: any, ImageData: any,
  atob: (/** @type {string} */ s) => Buffer.from(s, "base64").toString("binary"),
  btoa: (/** @type {string} */ s) => Buffer.from(s, "binary").toString("base64"),
};
const context = vm.createContext({ ...browser, console: { log: never, info: never, debug: never, warn: never, error: never } });
const global = vm.runInContext("globalThis", context);
global.window = global;
global.self = global;
global.parent = global;
global.top = global;

/** @type {string[]} */
const failures = [];
for (const src of scripts) {
  const file = join(ui, src);
  const code = readFileSync(file, "utf8");
  try {
    vm.runInContext(code, context, { filename: src });
  } catch (e) {
    // Only names are this check's business: the stand-in DOM can make
    // other errors that the window would not.
    const err = /** @type {Error} */ (e);
    if (err && err.name === "ReferenceError") {
      const at = (err.stack || "").split("\n").find(l => l.includes(src)) || "";
      failures.push(`${src}: ${err.message}${at ? ` (${at.trim()})` : ""}`);
    }
  }
}
assert.deepEqual(failures, [], `scripts use a name before any script defines it:\n  ${failures.join("\n  ")}`);
console.log(`ok - the ${scripts.length} scripts of index.html load in order without a ReferenceError`);
