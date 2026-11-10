// A small fake page that computers.js and screen.js run in, with the
// server and the stream stubbed. Used by takeover-check.mjs and
// computer-panel-check.mjs.
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));

// Elements the markup names (by id or data attribute), kept per page, so a
// lookup finds what is on the page now and a replaced element is gone.
class El {
  constructor(page, tag, attrs) {
    this.page = page; this.tag = tag; this.attrs = attrs; this.hidden = / hidden(\s|$|>)/.test(` ${attrs}`); this.src = ""; this.onclick = null; this.textContent = "";
    this.width = 0; this.height = 0; this.className = (attrs.match(/\bclass="([^"]*)"/) || [])[1] || ""; this.isConnected = true; this.fullScreens = 0;
  }
  // Redrawing an element throws away everything that was in it.
  set innerHTML(html) {
    this.html = html;
    for (const [key, el] of Object.entries(this.page.els)) if (el.parent === this) { el.isConnected = false; delete this.page.els[key]; }
    this.page.parse(html, this);
  }
  get innerHTML() { return this.html; }
  getContext() { return { putImageData() {}, drawImage() {} }; }
  toDataURL() { return "data:frame"; }
  focus() {}
  addEventListener() {}
  getBoundingClientRect() { return { left: 0, top: 0, width: 1, height: 1 }; }
  requestFullscreen() { this.fullScreens++; return Promise.resolve(); }
}

export class Page {
  constructor() { this.els = {}; this.body = new El(this, "div", ""); }
  parse(html, parent) {
    for (const [, tag, attrs] of html.matchAll(/<(\w+)([^>]*)>/g)) {
      const el = new El(this, tag, attrs);
      el.parent = parent;
      const id = attrs.match(/\bid="([^"]+)"/);
      if (id) this.els[`#${id[1]}`] = el;
      for (const [, name, value] of attrs.matchAll(/\b(data-[a-z-]+)(?:="([^"]*)")?/g)) {
        this.els[`[${name}]`] = el;
        if (value !== undefined) this.els[`[${name}="${value}"]`] = el;
      }
    }
  }
  // Each element of a fresh page is new: what was there before is gone.
  fresh() { for (const el of Object.values(this.els)) el.isConnected = false; this.els = {}; }
}

/**
 * computers.js and screen.js in a sandbox. `reply(method, path)` answers the
 * server; the sandbox records each call in `calls`.
 */
export function load({ reply, hash = "#/computer/c/screen", me = "alice" }) {
  const page = new Page();
  const calls = [];
  const stream = { rfb: null, closed: null, timers: [], listeners: [] };
  const sandbox = {
    UI: {}, route: () => {}, location: { hash },
    window: { addEventListener() {}, removeEventListener() {} },
    document: { addEventListener: (type, f) => stream.listeners.push([type, f]), fullscreenElement: null },
    esc: s => String(s ?? "").replace(/[&<>"]/g, c => `&#${c.charCodeAt(0)};`), enc: encodeURIComponent, toast: () => {}, paint: () => {}, when: at => `at ${at}`,
    firstLine: s => s.split("\n")[0], fieldValue: v => String(v),
    App: { me: { id: me }, tasks: [], inbox: [] }, PANELS: {}, PANEL_TITLES: {}, Panel: { what: null }, openPanel: () => {}, CSS: { escape: s => s },
    $: sel => page.els[sel],
    setTimeout: f => { stream.timers.push(f); return 0; },
    api: async (method, path, body) => { calls.push(`${method} ${path}`); return reply(method, path, body); },
    Magnus: { displayOpen: (id, ticket, prefix, feed, onClose) => { stream.closed = onClose; return { send() {}, close() {} }; } },
    Rfb: class { constructor(o) { stream.rfb = o; } feed() {} },
    ImageData: class { constructor() {} },
  };
  sandbox.globalThis = sandbox;
  vm.createContext(sandbox);
  for (const file of ["computers.js", "screen.js"]) vm.runInContext(readFileSync(join(here, "../ui", file), "utf8"), sandbox, { filename: file });
  return { page, calls, stream, sandbox };
}

export const settle = () => new Promise(r => setImmediate(r));

/** Lets every waiting look go round once more. */
export async function tick(stream) {
  const due = stream.timers.splice(0);
  for (const f of due) f();
  for (let i = 0; i < 5; i++) await settle();
}
