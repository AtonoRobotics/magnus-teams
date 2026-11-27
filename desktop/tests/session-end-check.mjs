// A session's end, with app.js and shared-canvas.js as they are
// (decisions/2026-11-23-sign-out-ends-everything.md):
// - sign-out, declining the agreements and a session the server ended all
//   end the session one way: the app is told (it stops the local agent and
//   everything else that ran for the person), then the page loads afresh,
//   so nothing one person had in it (stage notes and files, shared previews,
//   workspace state, a live stream) reaches whoever signs in next;
// - closed boxes are kept for the person signed in (aurora-boxes-check.mjs),
//   scoped at boot and dropped at the end;
// - the share dialog reads its conversation under its own name, so the
//   chat's redraw cannot leave it empty.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const read = f => readFileSync(join(here, "../ui", f), "utf8");
const app = read("app.js"), canvas = read("shared-canvas.js");
/** A top-level piece of app.js, from its first line to the line that closes it. */
const piece = (src, start, end = "\n}\n") => {
  const at = src.indexOf(start);
  assert.ok(at >= 0, `app.js has no ${start}`);
  return src.slice(at, src.indexOf(end, at) + end.length);
};

// Every way a session ends goes through endSession.
assert.match(app, /data-act=sign-out\]"\)\) \{\s*MessageDrafts\.clear\(\);\s*await endSession\(\);\s*\}/, "sign-out does not end the session");
assert.match(app, /if \(!\(await agreementsGate\(\)\)\) return endSession\(\);/, "declining the agreements does not end the session");
assert.match(app, /error\?\.status===401 && App\.me\)endSession\("Your session ended\. Sign in again\."\)/, "a session the server ended keeps the page");
assert.match(read("app-polling.js"), /e\.status === 401 && App\.me\) endSession\("Your session ended\. Sign in again\."\)/, "a session the server ended keeps the page");
const stopFn = piece(app, "function stop() {");
assert.ok(stopFn.includes("AuroraBoxes.scope(null, null);"), "stop() keeps the last person's closed boxes");
assert.ok(stopFn.includes("stopPanelLive();"), "stop() leaves the panel's live stream running");
assert.ok(piece(app, "async function boot() {").includes("AuroraBoxes.scope(App.appliance, App.me.id);"), "boot does not keep closed boxes for the person");

// endSession: the app first, then a fresh page, the note carried across.
{
  const done = [];
  const notes = new Map();
  const ctx = {
    stop: () => done.push("stop"),
    Magnus: { signOut: async () => { done.push("signOut"); } },
    sessionStorage: { setItem: (k, v) => notes.set(k, v) },
    location: { hash: "#/chat/t1", reload: () => done.push(`reload ${ctx.location.hash}`) },
  };
  vm.createContext(ctx);
  vm.runInContext(`${piece(app, "const SESSION_NOTE", ";\n")}\n${piece(app, "async function endSession(")}\nthis.endSession = endSession; this.SESSION_NOTE = SESSION_NOTE;`, ctx);
  await ctx.endSession("Your session ended. Sign in again.");
  assert.deepEqual(done, ["stop", "signOut", "reload "], "the session did not end with a fresh page");
  assert.equal(notes.get(ctx.SESSION_NOTE), "Your session ended. Sign in again.");
  done.length = 0;
  ctx.Magnus.signOut = async () => { throw new Error("offline"); };
  await ctx.endSession();
  assert.deepEqual(done, ["stop", "reload "], "a failed sign-out kept the page");
  assert.match(piece(app, "async function start() {"), /sessionStorage\.getItem\(SESSION_NOTE\)[\s\S]*signInScreen\(\(r && r\.note\) \|\| ended\)/, "the sign-in page does not say why the session ended");
}

// The share dialog's read and the chat's are apart: only the newest read
// under one name answers (Resources), so one name left the dialog empty.
{
  const pending = [];
  const ctx = { $: () => null, App: {}, Server: { call: () => new Promise(ok => pending.push(ok)) }, Date, Map, Set, document: {} };
  vm.createContext(ctx);
  vm.runInContext(`${piece(app, "const Resources = (()=>{", "\n})();\n")}\nthis.Resources = Resources;`, ctx);
  const name = canvas.match(/const SHARE_SOURCE = "([^"]+)";/)?.[1];
  assert.ok(name && name !== "Conversation", "the share dialog reads under the chat's name");
  assert.match(canvas, /Resources\.read\(SHARE_SOURCE, `\/v1\/chat\//, "the share dialog does not read under its own name");
  const both = async (first, second) => {
    const a = ctx.Resources.read(first, "/v1/chat/t1"), b = ctx.Resources.read(second, "/v1/chat/t1");
    pending.splice(0).forEach(ok => ok({ entries: [{ kind: "message", text: "hi" }] }));
    return [await a, await b];
  };
  const [dialog] = await both(name, "Conversation");
  assert.equal(dialog?.entries?.length, 1, "the chat's redraw left the share dialog empty");
  const [lost] = await both("Conversation", "Conversation");
  assert.equal(lost, undefined, "Resources answers more than the newest read of a name; this check no longer shows why the names differ");
}

console.log("session end: one way, app first then a fresh page; boxes per person; the share dialog reads apart passed");
