// A harness for the Admin sections' checks: admin.js and one section's
// script run in a VM with the page's helpers stubbed, the API answering from
// a table, and a small stand-in for the elements the section touches.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
export const ui = join(here, "../ui");
export const read = f => readFileSync(join(ui, f), "utf8");
const root = join(here, "../..");

/** The script is loaded by the page and served by the server, and the fast
 * tier runs its check. @param {string} file @param {string} check */
export function wired(file, check) {
  const page = read("index.html");
  const scripts = [...page.matchAll(/<script src="([^"]+)"><\/script>/g)].map(m => m[1]);
  assert.ok(scripts.includes(file) && scripts.indexOf(file) > scripts.indexOf("admin.js"), `index.html loads ${file} after admin.js`);
  const assets = readFileSync(join(root, "crates/magnus/src/ui/assets.rs"), "utf8");
  assert.ok(assets.includes(`("${file}", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/${file}"))`), `the server serves ${file}`);
  const fast = readFileSync(join(root, ".github/workflows/fast.yml"), "utf8");
  assert.ok(fast.includes(`node desktop/tests/${check}`), `the fast tier runs ${check}`);
}

/** An element: attributes, a parent, and what a section reads of it.
 * @param {Record<string, string>} [attrs] @param {any} [parent] */
export function node(attrs = {}, parent = null) {
  const dataset = {};
  for (const [k, v] of Object.entries(attrs)) if (k.startsWith("data-")) dataset[k.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())] = v;
  const n = {
    attrs, parent, dataset, html: "", value: attrs.value ?? "", checked: attrs.checked === "true", hidden: false, textContent: "", fields: {},
    listeners: {},
    getAttribute: name => attrs[name] ?? null,
    setAttribute: (name, v) => { attrs[name] = v; },
    addEventListener(t, f) { this.listeners[t] = f; },
    remove() {},
    /** @param {string} sel */
    matches(sel) {
      return sel.split(",").some(one => {
        const m = one.trim().match(/^\[([\w-]+)(?:="([^"]*)")?\]$/);
        return !!m && m[1] in attrs && (m[2] === undefined || attrs[m[1]] === m[2]);
      });
    },
    /** @param {string} sel */
    closest(sel) { let x = this; while (x) { if (x.matches(sel)) return x; x = x.parent; } return null; },
  };
  return n;
}

/** Load admin.js and `file` with the API answering from `answers`, keyed
 * "METHOD path", each a value or a function of the body.
 * @param {string} file @param {Record<string, any>} answers */
export function load(file, answers = {}) {
  const calls = [], toasts = [], els = new Map(), sections = new Map();
  const ctx = {
    calls, toasts, els, sections, console, URLSearchParams, encodeURIComponent, Promise, JSON, Math, Date, Number, String, Object, Array, Map,
    location: { hash: "#/admin" },
    enc: encodeURIComponent,
    esc: s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`),
    paint: (box, html) => { box.html = html; },
    toast: m => toasts.push(m),
    when: () => "2 min ago",
    icon: k => `<svg data-icon="${k}"></svg>`,
    route: () => {},
    sidebar: () => {},
    confirm: () => true,
    navigator: {},
    setInterval: () => 1, clearInterval: () => {}, setTimeout: () => 1,
    document: { createElement: () => node(), body: { append() {} } },
    FormData: class { constructor(f) { this.m = new Map(Object.entries(f?.fields || {})); } get(k) { return this.m.get(k) ?? null; } [Symbol.iterator]() { return this.m.entries(); } },
    $: sel => { if (!els.has(sel)) els.set(sel, node({ [sel.replace(/^\[|\]$/g, "").split("=")[0]]: "" })); return els.get(sel); },
  };
  ctx.api = async (m, p, b) => {
    calls.push([m, p, b]);
    const a = answers[`${m} ${p}`];
    if (a instanceof Error) throw a;
    return typeof a === "function" ? a(b) : (a ?? {});
  };
  ctx.Magnus = { api: ctx.api };
  vm.createContext(ctx);
  vm.runInContext(read("admin.js"), ctx);
  vm.runInContext(`AdminState.standing = { admin: true, owner: ${answers.owner === true}, step_up_seconds: 300, stepped_up_until: null };`, ctx);
  vm.runInContext(read(file), ctx);
  vm.runInContext("for (const [n, r] of AdminState.sections) globalThis.sections.set(n, r);", ctx);
  return ctx;
}

/** Draw `name` into a fresh element and give it back. */
export async function draw(ctx, name) {
  const el = node({ "data-admin-section": name });
  await ctx.sections.get(name)(el);
  return el;
}

/** Equal as JSON: values made inside the VM are of another realm. */
export const same = (a, b, msg) => assert.equal(JSON.stringify(a), JSON.stringify(b), msg);

/** An error as the API throws it. */
export const refused = (status, body) => Object.assign(new Error(body.message || body.error), { status, body });
