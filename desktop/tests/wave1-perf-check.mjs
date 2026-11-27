// Regression tests for Wave 1A frontend performance quick wins:
//   - aurora-avatar.js retired setInterval and documentElement MutationObserver
//   - app-shell.js wireDocument() retired duplicate body MutationObserver
//   - bridge.js webBridge.api() supports ETag / If-None-Match (304 Not Modified) caching
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";

const ROOT = path.resolve(import.meta.dirname, "..", "..");
const read = rel => fs.readFileSync(path.join(ROOT, rel), "utf8");

// 1. aurora-avatar.js has no setInterval or MutationObserver
const avatar = read("desktop/ui/aurora-avatar.js");
assert.doesNotMatch(avatar, /setInterval\(/, "aurora-avatar.js must not poll via setInterval");
assert.doesNotMatch(avatar, /new MutationObserver/, "aurora-avatar.js must not instantiate a MutationObserver");
assert.match(avatar, /UIStore\.subscribe/, "aurora-avatar.js must subscribe to UIStore");
assert.match(avatar, /magnus-render/, "aurora-avatar.js must listen for magnus-render");

// 2. app-shell.js wireDocument() uses magnus-render instead of duplicate body MutationObserver
const shell = read("desktop/ui/app-shell.js");
assert.match(shell, /window\.addEventListener\("magnus-render",\s*syncDocumentWidgets\)/, "wireDocument must use magnus-render for widget sync");

// 3. Behavioral ETag / 304 test in bridge.js webBridge.api()
{
  const seenHeaders = [];
  let callCount = 0;
  const ctx = vm.createContext({
    window: { __TAURI__: null },
    location: { protocol: "https:", host: "magnus.example" },
    fetch: async (_url, opts) => {
      callCount += 1;
      seenHeaders.push(opts.headers);
      if (callCount === 1) {
        return {
          ok: true,
          status: 200,
          headers: { get: h => (h.toLowerCase() === "etag" ? 'W/"seq-42"' : null) },
          text: async () => JSON.stringify({ tasks: [{ task_id: "t1" }] }),
        };
      }
      return { ok: false, status: 304, headers: { get: () => null }, text: async () => "" };
    },
    URLSearchParams,
    Map,
    JSON,
    Error,
  });
  ctx.window = ctx;
  vm.runInContext(`${read("desktop/ui/bridge.js")}\nthis.__Magnus = Magnus;`, ctx);
  const first = await ctx.__Magnus.api("GET", "/v1/tasks");
  assert.equal(first.tasks[0].task_id, "t1");
  const second = await ctx.__Magnus.api("GET", "/v1/tasks");
  assert.equal(second.tasks[0].task_id, "t1", "304 Not Modified must return cached response");
  assert.equal(seenHeaders[1]["if-none-match"], 'W/"seq-42"', "second GET must send If-None-Match ETag");
}

console.log("wave1-perf-check: ok");
