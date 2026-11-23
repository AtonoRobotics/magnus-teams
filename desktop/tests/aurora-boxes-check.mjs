// Which of Aurora's boxes show, and for how long
// (decisions/2026-11-21-aurora-boxes-come-and-go.md), with aurora-boxes.js as
// it is:
// - a box that wants the person stays; any other leaves SHOWN_FOR_MS after
//   it appears or changes, so work that runs for hours keeps no box;
// - a new status line is not a change; another state, title or box is;
// - a box the pointer is on stays, and its time starts again when it goes;
// - Close puts a box away until it changes, remembered in this browser for
//   the person signed in and no one else, and a browser that keeps nothing
//   still closes it for the page;
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

/** A page with its own AuroraBoxes over `store`, `person` signed in. @param {Map<string, string>|null} store null: storage throws @param {?string} person */
function load(store = new Map(), person = "ana") {
  const localStorage = store ? { getItem: k => store.get(k) ?? null, setItem: (k, v) => store.set(k, String(v)), removeItem: k => store.delete(k) } : { getItem() { throw new Error("blocked"); }, setItem() { throw new Error("blocked"); }, removeItem() { throw new Error("blocked"); } };
  const ctx = { localStorage, Date, JSON, Math, Array, Map, Set };
  vm.createContext(ctx);
  vm.runInContext(`${code}\nthis.AuroraBoxes = AuroraBoxes;`, ctx);
  ctx.AuroraBoxes.scope("https://s", person);
  return ctx.AuroraBoxes;
}
const ANA = "magnus-boxes-closed.https://s.ana";

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
  assert.equal(JSON.parse(store.get(ANA)).length, 200, "it remembers the latest closed, not every one");
}

// What one person closed is theirs: another person signing in on this
// browser sees their boxes and none of it, signing out forgets it for the
// page, and the list an older app kept for everyone goes unread.
{
  const store = new Map([["magnus-boxes-closed", JSON.stringify(["approval:i1|needs|Reply to Dana?"])]]);
  const B = load(store);
  assert.ok(!store.has("magnus-boxes-closed"), "the list kept for everyone on this browser stayed");
  assert.deepEqual(keys(B.shown([ask()], T)), ["approval:i1:needs"], "the list kept for everyone was used");
  B.close(B.sign(ask()));
  assert.deepEqual([...store.keys()], [ANA], "closed boxes were kept for no one in particular");
  B.scope(null, null);
  assert.deepEqual(keys(B.shown([ask()], T)), ["approval:i1:needs"], "signed out, the page still hides what the last person closed");
  B.close(B.sign(run()));
  assert.deepEqual([...store.keys()], [ANA], "a box closed while signed out was kept");
  assert.deepEqual(keys(load(store, "ben").shown([ask()], T)), ["approval:i1:needs"], "the next person sees what the last one closed");
  assert.deepEqual(keys(load(store, "ana").shown([ask()], T)), [], "the person's own closed boxes were lost");
}

// A browser that keeps nothing, or kept something unreadable.
{
  const B = load(null);
  B.close(B.sign(ask()));
  assert.deepEqual(keys(B.shown([ask()], T)), [], "closed for this page");
  assert.deepEqual(keys(load(new Map([[ANA, "{not json"]])).shown([ask()], T)), ["approval:i1:needs"]);
}
console.log("aurora boxes check passed");
