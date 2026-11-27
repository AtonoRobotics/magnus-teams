// Regression tests for Tier 3 Phase 1: Reactive UIStore
// Verifies 9-state phase derivation, completion decay timers, task freshness
// cache deduplication, subscriber notifications, and integration hooks.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";

const ROOT = path.resolve(import.meta.dirname, "..", "..");
const read = rel => fs.readFileSync(path.join(ROOT, rel), "utf8");

const src = read("desktop/ui/ui-store.js");
const ctx = vm.createContext({ Date });
vm.runInContext(`${src}\nthis.__UIStore = UIStore;`, ctx);
const UIStore = ctx.__UIStore;
assert.ok(UIStore, "UIStore should be defined");

// 1. Initial unhydrated state
assert.equal(UIStore.hasState(), false);
assert.equal(UIStore.derivePhase(), "ready");

// 2. All 9 canonical phases derived purely from state
UIStore.patch({ voiceListening: true });
assert.equal(UIStore.hasState(), true);
assert.equal(UIStore.derivePhase(), "listening");

UIStore.patch({ voiceListening: false, takeoverHeld: true });
assert.equal(UIStore.derivePhase(), "control");

UIStore.patch({ takeoverHeld: false, asks: [{ id: "ask-1" }] });
assert.equal(UIStore.derivePhase(), "waiting");

UIStore.patch({ asks: [] });
UIStore.setTask({ id: "t1", status: "running", plan: { units: [{ goal: "Draft", status: "active" }] } });
assert.equal(UIStore.derivePhase(), "planning");

UIStore.setTask({ id: "t1", status: "running", plan: { units: [] } });
assert.equal(UIStore.derivePhase(), "working");

// Completion transition -> done hold, then settles to ready after decay
UIStore.setTask({ id: "t1", status: "completed" });
assert.equal(UIStore.derivePhase(Date.now()), "done");
assert.equal(UIStore.derivePhase(Date.now() + 10000), "ready");

// Failure transition -> failed hold, then settles to ready
UIStore.setTask({ id: "t2", status: "running" });
UIStore.setTask({ id: "t2", status: "failed" });
assert.equal(UIStore.derivePhase(Date.now()), "failed");
assert.equal(UIStore.derivePhase(Date.now() + 10000), "ready");

// Live SSE writing -> thinking
UIStore.reset();
UIStore.setLiveWriting("t3", true);
assert.equal(UIStore.derivePhase(), "thinking");
UIStore.setLiveWriting("t3", false);
assert.equal(UIStore.derivePhase(), "ready");

// 3. Task freshness cache + pub/sub notifications
let hits = 0;
const unsub = UIStore.subscribe("tasksById", () => { hits += 1; });
UIStore.setTask({ id: "t4", status: "running", status_line: "Scanning inbox" });
assert.equal(hits, 1, "tasksById subscriber should fire");
assert.equal(UIStore.isTaskFresh("t4"), true, "just-stored task should be fresh");
assert.equal(UIStore.isTaskFresh("t4", 0), false, "zero maxAge should treat task as stale");
unsub();
UIStore.setTask({ id: "t4", status: "completed" });
assert.equal(hits, 1, "unsubscribed listener should not fire");

// 4. Verify wiring in index.html, assets.rs, aurora-core.js, aurora-reasoning.js, bridge.js, computers.js, app.js
assert.match(read("desktop/ui/index.html"), /<script src="ui-store\.js"><\/script>/);
assert.match(read("crates/magnus/src/ui/assets.rs"), /ui-store\.js/);
assert.match(read("desktop/ui/aurora-core.js"), /UIStore\.derivePhase\(\)/);
assert.match(read("desktop/ui/aurora-core.js"), /UIStore\.subscribe\("\*"/);
assert.match(read("desktop/ui/aurora-reasoning.js"), /UIStore\.isTaskFresh\(r\.task\)/);
assert.match(read("desktop/ui/aurora-reasoning.js"), /UIStore\.subscribe\("tasksById"/);
assert.match(read("desktop/ui/bridge.js"), /UIStore\.setLiveWriting\(task,\s*true\)/);
assert.match(read("desktop/ui/computers.js"), /UIStore\.patch\(\{\s*takeoverHeld:\s*true\s*\}\)/);
assert.match(read("desktop/ui/app.js"), /UIStore\.setTasks\(App\.tasks\)/);

console.log("ui-store-check: ok");
