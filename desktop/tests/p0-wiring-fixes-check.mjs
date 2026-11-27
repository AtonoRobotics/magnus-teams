// End-to-end behavioral regression tests for P0 runtime wiring fixes:
// 1. UIStore accepts real Magnus /v1/tasks payloads keyed by `task_id` (not just `id`),
//    and app.js refresh() publishes real `needsMe()` approvals into UIStore.
// 2. MobileApprove (#/approve) calls real Server.inbox.list(), Server.inbox.batch(),
//    delegates individual button[data-decision] clicks to respond(btn, main),
//    renders Admin Emergency Stop when AdminState.standing.admin is true,
//    and filters real `status === "open"` items.
// 3. PlanSurface.renderLiveCard(e.task_id) is mounted in app.js `case "turn"`.
// 4. Desktop Tauri bridge in bridge.js exposes pickFolder().
// 5. Blocking approval notices in app-polling.js deep-link to #/approve?ask=<id> on mobile widths.
import assert from "node:assert/strict";
import { readAppBundle } from "./app-bundle.mjs";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";

const ROOT = path.resolve(import.meta.dirname, "..", "..");
const read = rel => fs.readFileSync(path.join(ROOT, rel), "utf8");

// 1. Behavioral test: UIStore with real Magnus `task_id` payloads
{
  const ctx = vm.createContext({ Date });
  vm.runInContext(`${read("desktop/ui/ui-store.js")}\nthis.__UIStore = UIStore;`, ctx);
  const S = ctx.__UIStore;
  S.setTasks([{ task_id: "t_real_1", status: "running", plan: { units: [{ goal: "Step 1", status: "active" }] } }]);
  assert.ok(S.getTask("t_real_1"), "UIStore.setTasks must index Magnus tasks by task_id");
  assert.equal(S.derivePhase(), "planning", "UIStore.derivePhase must derive planning from task_id task");
  S.setTask({ task_id: "t_real_1", status: "completed" });
  assert.equal(S.derivePhase(Date.now()), "done", "UIStore.setTask must track completion by task_id");
  assert.match(readAppBundle(), /asks:\s*typeof needsMe === "function" \? needsMe\(\) : \[\]/, "app.js must publish needsMe() to UIStore.asks");
}

// 2. Behavioral test: MobileApprove end-to-end with real Server.inbox, respond(btn, main), and Admin Emergency Stop
{
  const inboxItems = [
    { id: "ask_rev_1", status: "open", can_respond: true, tier: "reversible", summary: "Draft follow-up" },
    { id: "ask_rev_2", status: "open", can_respond: true, tier: "reversible", summary: "Update calendar" },
    { id: "ask_fin_1", status: "open", can_respond: true, tier: "financial", summary: "Pay $500 invoice" },
    { id: "ask_closed", status: "resolved", can_respond: true, tier: "reversible", summary: "Already done" },
  ];
  const calls = { list: 0, batch: [], respond: [], adminStop: 0 };
  const main = { innerHTML: "", onclick: null };
  const ctx = vm.createContext({
    URLSearchParams,
    location: { origin: "https://magnus.example", pathname: "/", hash: "#/approve?ask=ask_fin_1" },
    App: { inbox: [] },
    AdminState: { standing: { admin: true } },
    Server: {
      inbox: {
        list: async () => { calls.list += 1; return { items: inboxItems }; },
        batch: async (ids, body) => { calls.batch.push({ ids, body }); },
      },
    },
    askCard: (a, can) => `<div class="ask" data-ask="${a.id}"><button data-item="${a.id}" data-decision="approve:once" data-can="${can}">Allow</button></div>`,
    respond: async (btn, root) => { calls.respond.push({ item: btn.dataset.item, decision: btn.dataset.decision, root }); },
    adminStop: async () => { calls.adminStop += 1; },
    toast: () => {},
  });
  vm.runInContext(`${read("desktop/ui/mobile-approve.js")}\nthis.__MobileApprove = MobileApprove;`, ctx);
  await ctx.__MobileApprove.show(main, "ask=ask_fin_1");

  assert.equal(calls.list, 1, "MobileApprove.show must fetch Server.inbox.list()");
  assert.match(main.innerHTML, /Approvals \(3\)/, "must count only open approvals (excluding resolved)");
  assert.match(main.innerHTML, /data-mobile-admin-kill/, "must render Emergency Stop button for admins");
  assert.match(main.innerHTML, /Allow all 2 reversible/, "batch count must exclude financial asks");
  assert.ok(main.innerHTML.indexOf("ask_fin_1") < main.innerHTML.indexOf("ask_rev_1"), "focused ?ask= must appear first");

  // Click individual Allow button -> delegates to respond(btn, main)
  const fakeAllowBtn = { dataset: { item: "ask_rev_1", decision: "approve:once" }, closest: s => (s === "button[data-decision]" ? fakeAllowBtn : null) };
  await main.onclick({ target: fakeAllowBtn });
  assert.equal(calls.respond.length, 1, "individual decision button must delegate to respond(btn, main)");
  assert.equal(calls.respond[0].item, "ask_rev_1");
  assert.equal(calls.respond[0].root, main);

  // Click Batch Allow -> calls Server.inbox.batch with reversible IDs only
  const fakeBatchBtn = { disabled: false, closest: s => (s === "[data-mobile-allow-reversible]" ? fakeBatchBtn : null) };
  await main.onclick({ target: fakeBatchBtn });
  assert.equal(calls.batch.length, 1, "batch button must call Server.inbox.batch()");
  assert.deepEqual([...calls.batch[0].ids].sort(), ["ask_rev_1", "ask_rev_2"], "batch must only include reversible asks");
  assert.deepEqual({ ...calls.batch[0].body }, { decision: "approve", scope: "once" });

  // Click Emergency Stop -> calls adminStop()
  const fakeKillBtn = { closest: s => (s === "[data-mobile-admin-kill]" ? fakeKillBtn : null) };
  await main.onclick({ target: fakeKillBtn });
  assert.equal(calls.adminStop, 1, "Emergency Stop button must invoke adminStop()");
}

// 3. PlanSurface.renderLiveCard mounted in app.js case "turn"
assert.match(readAppBundle(), /PlanSurface\.renderLiveCard\(e\.task_id\)/, "app.js must mount PlanSurface.renderLiveCard(e.task_id) in turn entries");

// 4. Desktop Tauri bridge exposes pickFolder()
const bridge = read("desktop/ui/bridge.js");
const desktopSlice = bridge.slice(0, bridge.indexOf("const webBridge ="));
assert.match(desktopSlice, /async pickFolder\(\)\s*\{\s*return this\.invoke\("pick_folder"\)/, "desktop Tauri bridge must expose pickFolder()");

// 5. Blocking notices deep-link to #/approve?ask=<id> on mobile viewports
assert.match(read("desktop/ui/app-polling.js"), /#\/approve\?ask=\$\{enc\(i\.id\)\}/, "app-polling.js must deep-link blocking notices to #/approve?ask=<id> on mobile");

// 6. Desktop Tauri live() feeds UIStore.setLiveWriting and UIStore.setTask
assert.match(desktopSlice, /UIStore\.setLiveWriting\(task,\s*true\)/, "desktop live() must set liveWriting true");
assert.match(desktopSlice, /UIStore\.setLiveWriting\(task,\s*false\)/, "desktop live() must set liveWriting false on close");
assert.match(desktopSlice, /UIStore\.setTask\(/, "desktop live() must forward task updates into UIStore");

// 7. PlanSurface livePlans survives reload via sessionStorage
{
  const bag = new Map();
  const storage = { getItem: k => bag.get(k) ?? null, setItem: (k, v) => bag.set(k, String(v)) };
  const c1 = vm.createContext({ sessionStorage: storage, Date });
  vm.runInContext(`${read("desktop/ui/plan-surface.js")}\nthis.__P = PlanSurface;`, c1);
  c1.__P.trackTask("t_reload_1", ["Step A", "Step B"]);
  const c2 = vm.createContext({ sessionStorage: storage, Date });
  vm.runInContext(`${read("desktop/ui/plan-surface.js")}\nthis.__P = PlanSurface;`, c2);
  const reloadedHtml = c2.__P.renderLiveCard("t_reload_1");
  assert.match(reloadedHtml, /Step A[\s\S]*Step B/, "PlanSurface livePlans must hydrate from sessionStorage across reload");
}

// 8. MobileApprove subscribes to UIStore asks updates and unsubscribes onLeave
{
  let leaveFn = null;
  const main = { innerHTML: "", onclick: null };
  const ctx = vm.createContext({
    URLSearchParams,
    Date,
    location: { origin: "https://magnus.example", pathname: "/", hash: "#/approve" },
    App: { inbox: [] },
    onLeave: fn => { leaveFn = fn; },
    Server: { inbox: { list: async () => ({ items: [] }) } },
    askCard: a => `<div data-ask="${a.id}">${a.summary}</div>`,
  });
  vm.runInContext(`${read("desktop/ui/ui-store.js")}\n${read("desktop/ui/mobile-approve.js")}\nthis.__S = UIStore; this.__M = MobileApprove;`, ctx);
  await ctx.__M.show(main, "");
  assert.match(main.innerHTML, /All caught up/);
  ctx.__S.patch({ asks: [{ id: "live_ask_1", status: "open", can_respond: true, summary: "Send live email" }] });
  await new Promise(r => setTimeout(r, 0));
  assert.match(main.innerHTML, /Send live email/, "MobileApprove must auto-redraw when UIStore asks updates");
  assert.equal(typeof leaveFn, "function", "MobileApprove must register onLeave cleanup for UIStore subscription");
  leaveFn();
}

console.log("p0-wiring-fixes-check: ok");
