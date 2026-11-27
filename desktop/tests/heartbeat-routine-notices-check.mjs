// Regression check for:
// 1. Heartbeat is a system task (never clutters chat, stage, rail badge, or task list unless user is needed).
// 2. Routine/ideas notifications disappear once resolved or set up (Notices.syncInbox prunes history & live popups).
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const uiDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../ui");
const read = f => fs.readFileSync(path.join(uiDir, f), "utf8");

const noticesSrc = read("notices.js");
const exploreIdeasSrc = read("explore-ideas.js");
const appSrc = read("app.js");
const tasksSchedSrc = read("tasks-schedules.js");
const railSrc = read("aurora-rail.js");
const stageSrc = read("aurora-stage.js");

// 1. Verify Notices.syncInbox prunes resolved items and stale routine/ideas notices.
const removed = [];
const fakePopups = [
  { dataset: { notice: "inbox:i_done" }, remove() { removed.push("inbox:i_done"); } },
  { dataset: { notice: "inbox:i_ideas" }, remove() { removed.push("inbox:i_ideas"); } },
  { dataset: { notice: "inbox:i_open" }, remove() { removed.push("inbox:i_open"); } },
];
class FakeEl {}
for (const p of fakePopups) Object.setPrototypeOf(p, FakeEl.prototype);

const ctx = vm.createContext({
  HTMLElement: FakeEl,
  document: {
    hidden: false,
    hasFocus: () => true,
    querySelectorAll: () => fakePopups,
  },
  location: { hash: "#/home" },
  localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
  $: () => null,
  esc: s => String(s ?? ""),
  Magnus: { kind: "desktop", notifyPermission: () => "denied" },
  AuroraStage: { notes: new Map([["inbox:i_ideas", {}]]), takes: () => false, refresh() {} },
});
vm.runInContext(`${exploreIdeasSrc}\n${noticesSrc}\nthis.Notices = Notices; this.ExploreIdeas = ExploreIdeas;`, ctx);
const { Notices } = ctx;

Notices.history = [
  { key: "inbox:i_done", title: "Routine setup", body: "", system: "", href: "#/inbox" },
  { key: "inbox:i_ideas", title: "Ideas in Explore", body: "Magnus has an idea", system: "", href: "#/explore" },
  { key: "inbox:i_open", title: "A task needs you", body: "Approve send", system: "", href: "#/inbox" },
];

const changed = Notices.syncInbox([
  { id: "i_done", kind: "suggestion", status: "resolved", details: { every_minutes: 60 } },
  { id: "i_ideas", kind: "notice", status: "open", details: { reason: "ideas", run: "r1" } },
  { id: "i_open", kind: "approval", status: "open", details: {} },
]);
assert.equal(changed, true, "syncInbox reports pruning");
assert.deepEqual(Notices.history.map(n => n.key), ["inbox:i_open"], "resolved routine & stale ideas notices disappear from history while real pending approval stays");
assert.ok(removed.includes("inbox:i_done") && removed.includes("inbox:i_ideas") && !removed.includes("inbox:i_open"), "live popups for resolved/stale items removed");
assert.equal(ctx.AuroraStage.notes.has("inbox:i_ideas"), false, "stale ideas note cleared from AuroraStage.notes");

// 2. The heartbeat is retired (decisions/2026-11-26-the-heartbeat-is-retired.md):
// nothing offers to start one, and a system schedule still listed stays out of quiet UI surfaces.
assert.doesNotMatch(appSrc + tasksSchedSrc, /data-enable-heartbeat|Enable Heartbeat|title:\s*"Heartbeat"/, "nothing offers to start a heartbeat");
assert.match(appSrc, /const isQuietSystemTask =/, "app.js defines isQuietSystemTask helper");
assert.doesNotMatch(tasksSchedSrc, /data-system-heartbeat/, "schedulesView has no Heartbeat card");
assert.match(tasksSchedSrc, /const list = sorted\.filter\(sc => !isSys\(sc\)\)/, "schedulesView does not duplicate System Heartbeat into user routines");
assert.match(railSrc, /!t\.routine && !t\.system/, "aurora-rail excludes quiet system tasks from rail badge");
assert.match(stageSrc, /!\(t\.system && !waits\(t\) && t\.status !== "failed"\)/, "aurora-stage excludes quiet system tasks unless user is needed");

console.log("heartbeat-routine-notices-check: ok");
