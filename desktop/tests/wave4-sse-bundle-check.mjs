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

// 2. webBridge clears ETag cache on signOut() and clearCache() (prevents cross-user 304 leaks).
ctx.window.Magnus._etags.set("/v1/tasks", { etag: 'W/"seq-1-alice"', data: { tasks: [{ id: "t1" }] } });
ctx.window.Magnus.clearCache();
assert.equal(ctx.window.Magnus._etags.size, 0, "clearCache() must empty _etags");
ctx.window.Magnus._etags.set("/v1/tasks", { etag: 'W/"seq-1-alice"', data: { tasks: [{ id: "t1" }] } });
await ctx.window.Magnus.signOut();
assert.equal(ctx.window.Magnus._etags.size, 0, "signOut() must empty _etags");

// 3. app.js defines refreshDomains() covering all SSE domains and app-boot.js reconciles safely while SSE is open.
const appSrc = fs.readFileSync(path.join(uiDir, "app.js"), "utf8");
const bootSrc = fs.readFileSync(path.join(uiDir, "app-boot.js"), "utf8");
assert.match(appSrc, /async function refreshDomains\s*\(/, "app.js must define refreshDomains()");
assert.match(appSrc, /want\.has\("projects"\)/, "refreshDomains() must handle projects");
assert.match(appSrc, /want\.has\("computers"\)/, "refreshDomains() must handle computers");
assert.match(appSrc, /want\.has\("workspaces"\)/, "refreshDomains() must handle workspaces");
assert.match(appSrc, /want\.has\("staged"\)/, "refreshDomains() must handle staged");
assert.match(bootSrc, /Magnus\.events\(/, "app-boot.js must subscribe to Magnus.events()");
assert.match(bootSrc, /if\s*\(\s*sseConnected\s*&&\s*Date\.now\(\)\s*-\s*lastRefreshMs\s*<\s*REFRESH_SSE_RECONCILE_MS\s*\)\s*return;/, "app-boot.js must throttle to 60s reconcile while SSE is connected");

// 4. Zero-build concatenated JS bundle parses with zero syntax errors.
const indexHtml = fs.readFileSync(path.join(uiDir, "index.html"), "utf8");
const scripts = [...indexHtml.matchAll(/<script src="([^"]+\.js)"><\/script>/g)].map(m => m[1]);
assert.ok(scripts.length >= 80, `expected >= 80 scripts in index.html, got ${scripts.length}`);
const jsBundle = scripts.map(f => fs.readFileSync(path.join(uiDir, f), "utf8")).join("\n;\n");
new vm.Script(jsBundle, { filename: "app.bundle.js" });

console.log("wave4-sse-bundle-check: ok");
