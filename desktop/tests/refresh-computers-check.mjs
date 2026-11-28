// A refresh of the computers keeps App.computers the list GET /v1/computers
// holds, as the first load does. Production, 2026-11-27: it kept the whole
// response, and the conversation head threw "(computers || []).find is not
// a function".
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const src = readFileSync(new URL("../ui/app.js", import.meta.url), "utf8");
const start = src.indexOf("async function refreshDomains(");
assert.ok(start >= 0, "app.js defines refreshDomains()");
const end = src.indexOf("\n}\n", start);
const fn = src.slice(start, end + 2);

const mine = { id: "c1", owner: { person: "mo" } };
const ctx = {
  App: { me: { id: "mo" }, computers: null },
  Server: { computers: { list: async () => ({ computers: [mine] }) } },
  Resources: { read: async () => null },
  pollInbox: async () => null,
  syncUiAfterRefresh: async () => {},
};
vm.createContext(ctx);
vm.runInContext(`${fn}\nthis.refreshDomains = refreshDomains;`, ctx);
await ctx.refreshDomains(["computers"]);
assert.ok(Array.isArray(ctx.App.computers), `App.computers is a list: ${JSON.stringify(ctx.App.computers)}`);
assert.equal(ctx.App.computers[0].id, "c1");
console.log("refresh-computers-check: ok");
