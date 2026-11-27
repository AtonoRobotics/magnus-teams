// Wave 4 regression check: SSE /v1/events invalidation client + zero-build concatenated JS/CSS bundle.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";

const uiDir = path.resolve(import.meta.dirname, "../ui");

// 1. webBridge.events() connects to /v1/events, forwards domains, tracks connection state, and closes cleanly.
const bridgeSrc = fs.readFileSync(path.join(uiDir, "bridge.js"), "utf8");
let lastEs = null;
class FakeEventSource {
  constructor(url, opts) {
    this.url = url;
    this.opts = opts;
    this.closed = false;
    lastEs = this;
  }
  close() { this.closed = true; }
}
const ctx = {
  window: { addEventListener() {}, location: { origin: "https://magnus.test", protocol: "https:", host: "magnus.test" } },
  document: { addEventListener() {} },
  location: { origin: "https://magnus.test", protocol: "https:", host: "magnus.test" },
  EventSource: FakeEventSource,
  fetch: async () => ({ ok: true, status: 200, headers: new Headers(), text: async () => "{}" }),
  Headers,
};
ctx.globalThis = ctx;
vm.runInNewContext(bridgeSrc, ctx);

const seen = [];
const states = [];
const handle = ctx.window.Magnus.events(msg => seen.push(msg), ok => states.push(ok));
assert.equal(lastEs.url, "/v1/events");
lastEs.onopen();
assert.deepEqual(states, [true]);
lastEs.onmessage({ data: JSON.stringify({ seq: 5, domains: [] }) });
assert.equal(seen.length, 0, "empty initial domains should not trigger invalidation");
lastEs.onmessage({ data: JSON.stringify({ seq: 6, domains: ["tasks", "inbox"] }) });
assert.equal(seen.length, 1);
assert.deepEqual(Array.from(seen[0].domains), ["tasks", "inbox"]);
lastEs.onerror();
assert.deepEqual(states, [true, false]);
handle.close();
assert.equal(lastEs.closed, true);

// 2. app.js defines refreshDomains() and app-boot.js wires Magnus.events() + pauses pollRefreshTick when SSE is connected.
const appSrc = fs.readFileSync(path.join(uiDir, "app.js"), "utf8");
const bootSrc = fs.readFileSync(path.join(uiDir, "app-boot.js"), "utf8");
assert.match(appSrc, /async function refreshDomains\s*\(/, "app.js must define refreshDomains()");
assert.match(bootSrc, /Magnus\.events\(/, "app-boot.js must subscribe to Magnus.events()");
assert.match(bootSrc, /if\s*\(\s*sseConnected\s*\)\s*return;/, "app-boot.js must skip 10s poll while SSE is connected");

// 3. Zero-build concatenated JS bundle parses with zero syntax errors.
const indexHtml = fs.readFileSync(path.join(uiDir, "index.html"), "utf8");
const scripts = [...indexHtml.matchAll(/<script src="([^"]+\.js)"><\/script>/g)].map(m => m[1]);
assert.ok(scripts.length >= 80, `expected >= 80 scripts in index.html, got ${scripts.length}`);
const jsBundle = scripts.map(f => fs.readFileSync(path.join(uiDir, f), "utf8")).join("\n;\n");
new vm.Script(jsBundle, { filename: "app.bundle.js" });

console.log("wave4-sse-bundle-check: ok");
