// Which of Aurora's boxes show, and for how long
// (decisions/2026-11-21-aurora-boxes-come-and-go.md), with aurora-boxes.js as
// it is:
// - a box that wants the person stays; any other leaves SHOWN_FOR_MS after
//   it appears or changes, so work that runs for hours keeps no box;
// - a new status line is not a change; another state, title or box is;
// - a box the pointer is on stays, and its time starts again when it goes;
// - Close puts a box away until it changes, remembered in this browser,
//   and a browser that keeps nothing still closes it for the page;
// - the app loads it before the stage.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const code = readFileSync(join(here, "../ui/aurora-boxes.js"), "utf8");
const index = readFileSync(join(here, "../ui/index.html"), "utf8");
assert.ok(index.indexOf('<script src="aurora-boxes.js"></script>') >= 0 && index.indexOf('<script src="aurora-boxes.js"></script>') < index.indexOf('<script src="aurora-stage.js"></script>'), "index.html loads aurora-boxes.js before aurora-stage.js");

/** A page with its own AuroraBoxes over `store`. @param {Map<string, string>|null} store null: storage throws */
function load(store = new Map()) {
  const localStorage = store ? { getItem: k => store.get(k) ?? null, setItem: (k, v) => store.set(k, String(v)) } : { getItem() { throw new Error("blocked"); }, setItem() { throw new Error("blocked"); } };
  const ctx = { localStorage, Date, JSON, Math, Array, Map, Set };
  vm.createContext(ctx);
  vm.runInContext(`${code}\nthis.AuroraBoxes = AuroraBoxes;`, ctx);
  return ctx.AuroraBoxes;
}

const run = (o = {}) => ({ key: "live", tone: "run", title: "Watch the inbox", desc: "Reading", ...o });
const ask = (o = {}) => ({ key: "approval:i1", tone: "needs", title: "Reply to Dana?", ...o });
const keys = list => Array.from(list, c => `${c.key}:${c.tone}`);
const T = 1_000_000;

// Work that runs for hours shows, then leaves; a decision stays.
{
  const B = load();
  const S = B.SHOWN_FOR_MS;
  assert.deepEqual(keys(B.shown([run(), ask()], T)), ["live:run", "approval:i1:needs"]);
  assert.equal(B.next([run(), ask()], T + 1000), S - 1000, "the running box leaves on time");
  assert.deepEqual(keys(B.shown([run(), ask()], T + S - 1)), ["live:run", "approval:i1:needs"]);
  assert.deepEqual(keys(B.shown([run(), ask()], T + S)), ["approval:i1:needs"], "work that keeps running keeps no box");
  assert.deepEqual(keys(B.shown([run(), ask()], T + 3600_000)), ["approval:i1:needs"], "a decision stays until it is dealt with");
  assert.equal(B.next([ask()], T), null, "nothing leaves on its own");
  // A new status line is not a change; finishing, or needing the person, is.
  assert.deepEqual(keys(B.shown([run({ desc: "Reading more" })], T + S + 10)), []);
  assert.deepEqual(keys(B.shown([run({ tone: "done" })], T + S + 20)), ["live:done"], "finished work shows again");
  assert.deepEqual(keys(B.shown([run({ title: "Another task" })], T + S + 30)), ["live:run"], "another task shows");
  // Gone and back is new again.
  B.shown([], T + S + 40);
  assert.deepEqual(keys(B.shown([run()], T + 2 * S)), ["live:run"]);
}

// A message, and a decision just answered, stay too.
{
  const B = load();
  const said = [{ key: "messages", tone: "talk", title: "Dana" }, { key: "approval:i2", tone: "done", title: "Pay?", said: "Allowed" }];
  B.shown(said, T);
  assert.deepEqual(keys(B.shown(said, T + 3600_000)), ["messages:talk", "approval:i2:done"]);
}

// The pointer on a box keeps it; its time starts again when the pointer goes.
{
  const B = load();
  const S = B.SHOWN_FOR_MS;
  B.shown([run()], T);
  const held = new Set([B.sign(run())]);
  assert.deepEqual(keys(B.shown([run()], T + 10 * S, held)), ["live:run"], "kept while the pointer is on it");
  B.left([B.sign(run())], T + 11 * S);
  assert.deepEqual(keys(B.shown([run()], T + 12 * S - 1)), ["live:run"], "its time starts again when the pointer goes");
  assert.deepEqual(keys(B.shown([run()], T + 12 * S)), []);
}

// Close puts a box away until it changes, and the browser remembers it.
{
  const store = new Map();
  const B = load(store);
  B.close(B.sign(ask()));
  assert.deepEqual(keys(B.shown([ask(), run()], T)), ["live:run"]);
  assert.deepEqual(keys(B.shown([ask({ key: "approval:i3", title: "Pay the invoice?" })], T)), ["approval:i3:needs"], "another decision comes in");
  B.close(B.sign(run()));
  assert.deepEqual(keys(B.shown([run({ desc: "Still reading" })], T)), [], "closed work stays away while it runs");
  assert.deepEqual(keys(B.shown([run({ tone: "needs" })], T)), ["live:needs"], "and comes back when it needs the person");
  assert.deepEqual(keys(load(store).shown([ask(), run()], T)), [], "a new page remembers what was closed");
  for (let i = 0; i < 250; i++) B.close(`x${i}|run|t`);
  assert.equal(JSON.parse(store.get("magnus-boxes-closed")).length, 200, "it remembers the latest closed, not every one");
}

// A browser that keeps nothing, or kept something unreadable.
{
  const B = load(null);
  B.close(B.sign(ask()));
  assert.deepEqual(keys(B.shown([ask()], T)), [], "closed for this page");
  assert.deepEqual(keys(load(new Map([["magnus-boxes-closed", "{not json"]])).shown([ask()], T)), ["approval:i1:needs"]);
}
console.log("aurora boxes check passed");
