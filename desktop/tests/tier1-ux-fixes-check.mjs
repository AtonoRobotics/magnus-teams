// Fast-tier regression checks for Tier 1 UX & trust fixes:
// 1. DND silences non-blocking notices but still surfaces blocking approvals/takeovers.
// 2. Admin kill switch syncs both #detail ([data-admin-kill]) and top-bar ([data-admin-kill-top]).
// 3. Settings "Request more" spend button passes a valid spend_limit payload to offerMoreSpend;
//    Admin Spend removes retired Team Admin copy; Admin Agents shows Idle when running is empty.
// 4. VNC takeover uses AbortController so repeat takeovers never duplicate canvas listeners,
//    and handback stops active step recording before leaving.
// 5. Composer formatting shortcuts stopPropagation, file-only sends submit, and cancelling
//    Edit-and-resend preserves the edit box and draft.
// 6. Nova A/E/D approval triage is guarded by isNova(), defaultPrevented, and open dialogs;
//    Shortcuts modal documents composer formatting and triage keys.
// 7. Schedule cadence fields hide irrelevant inputs by kind, tasks filter includes Waiting,
//    and Discovery Wizard weekday templates use weekdays_at.
// 8. User-facing Nova copy has no internal "Cedar" jargon, and Settings links to Trash.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);

// 1. DND exempts blocking notices while silencing non-blocking ones.
{
  const storage = new Map();
  const popped = [];
  const ctx = {
    localStorage: {
      getItem: k => storage.get(k) ?? null,
      setItem: (k, v) => storage.set(k, String(v)),
      removeItem: k => storage.delete(k),
    },
    document: { hidden: false, hasFocus: () => true, querySelectorAll: () => [], body: { append() {} } },
    location: { hash: "#/home" },
    Magnus: { kind: "web", notifyPermission: () => "default" },
    $: () => null,
    esc,
    setTimeout,
    clearTimeout,
  };
  ctx.globalThis = ctx;
  vm.runInNewContext(ui("notices.js") + "\nglobalThis.__N = Notices;", ctx);
  ctx.__N.popup = s => popped.push(s.key);
  ctx.__N.snooze(60);
  ctx.__N.show({ key: "nb:1", title: "FYI", body: "Low priority", system: "FYI", href: "#/tasks", blocking: false });
  ctx.__N.show({ key: "blk:1", title: "A task needs you", body: "Approve payment", system: "Approve", href: "#/inbox", blocking: true });
  assert.deepEqual(popped, ["blk:1"], "DND must silence non-blocking notices while still surfacing blocking approvals");
  assert.match(ui("app-polling.js"), /Notices\.show\(\{[\s\S]*?blocking:\s*!!i\.blocking/, "app-polling.js must forward i.blocking to Notices.show");
}

// 2. Admin kill switch syncs both #detail and persistent top-bar slots.
{
  const mkBox = () => ({ hidden: true, dataset: {}, html: "", clicks: 0, addEventListener() { this.wired = true; } });
  const detailBox = mkBox();
  const topBox = mkBox();
  const ctx = {
    $: sel => (sel === "[data-admin-kill]" ? detailBox : null),
    $$: sel => (sel.includes("data-admin-kill-top") ? [detailBox, topBox] : []),
    paint: (el, h) => { el.html = h; },
    esc,
    when: () => "now",
    route: () => {},
    toast: () => {},
    document: { body: { dataset: {} }, createElement: () => ({}) },
    location: { hash: "#/home" },
    Server: { admin: {}, stops: { list: async () => ({ stops: [] }) } },
    setInterval: () => 1,
    clearInterval: () => {},
  };
  ctx.globalThis = ctx;
  vm.runInNewContext(ui("admin.js") + "\nglobalThis.__A = { AdminState, showKillControl, drawKillControl, adminStop };", ctx);
  ctx.__A.AdminState.standing = { admin: true, owner: true };
  ctx.__A.showKillControl();
  await ctx.__A.drawKillControl();
  assert.equal(detailBox.hidden, false, "#detail kill switch must be shown");
  assert.equal(topBox.hidden, false, "top-bar kill switch must be shown");
  assert.match(topBox.html, /data-kill-press="server"/, "top-bar kill switch must render Stop everything");
  ctx.__A.adminStop();
  assert.equal(topBox.hidden, true, "adminStop must hide top-bar kill switch");
  assert.equal(topBox.html, "", "adminStop must clear top-bar kill switch");
  assert.match(ui("nova-frame.js"), /data-admin-kill-top/, "nova-frame.js must mount data-admin-kill-top");
  assert.match(ui("app.js"), /id="workspace-bar"[\s\S]*data-admin-kill-top/, "workspace-bar must mount data-admin-kill-top");
}

// 3. Spend request payload + Admin Agents Idle badge + Admin Spend clean copy.
{
  assert.match(ui("settings-sections.js"), /spend_limit:\s*\{\s*which:\s*"person",\s*can_ask:\s*true/, "Settings Request more must pass spend_limit to offerMoreSpend");
  assert.doesNotMatch(ui("admin-spend.js"), /Team Admin sets each channel/, "admin-spend.js must not mention retired Team Admin channel limits");

  const ctx = {
    esc,
    when: () => "now",
    registerAdminSection: () => {},
    registerKillLevel: () => {},
  };
  ctx.globalThis = ctx;
  vm.runInNewContext(ui("admin-agents.js") + "\nglobalThis.__card = agentsCard;", ctx);
  const idleHtml = ctx.__card({ id: "a1", name: "Ada", person: "ada", spent_usd: 0, paused: null, stopped: null, person_stopped: null, running: [], schedules: [] });
  const busyHtml = ctx.__card({ id: "a2", name: "Bob", person: "bob", spent_usd: 1, paused: null, stopped: null, person_stopped: null, running: [{ run: "r1", kind: "chat", status: "running", asked_by: "bob", spent_usd: 1, sub_agents: [] }], schedules: [] });
  assert.match(idleHtml, /<span class="chip">Idle<\/span>/, "Idle agent with no running tasks must show Idle chip");
  assert.match(busyHtml, /<span class="chip">Working<\/span>/, "Active agent with running tasks must show Working chip");
}

// 4. VNC takeover AbortController cleanup & step-recording handback guard.
{
  const compSrc = ui("computers.js");
  assert.match(compSrc, /const inputAbort = typeof AbortController === "function" \? new AbortController\(\) : null;/, "takeOver must create an AbortController for canvas listeners");
  assert.match(compSrc, /detachCanvas\(\)/, "takeOver must detach canvas listeners on leave/failure");
  assert.match(compSrc, /if \(view\.recording && typeof view\.stopRecording === "function"\) await view\.stopRecording\(\)/, "handback must finish active step recording before leaving");
}

// 5. Composer shortcuts stopPropagation, file-only sends allowed, and Edit-cancel preserves draft.
{
  const compSrc = ui("chat-composer.js");
  assert.match(compSrc, /if \(k === "b"\) \{ e\.preventDefault\(\); e\.stopPropagation\(\);/, "Cmd/Ctrl+B in composer must stopPropagation");
  assert.match(compSrc, /if \(\(!text && !record\.files\.length\) \|\| record\.sending\) return;/, "composer must allow file-only message submission");
  assert.doesNotMatch(compSrc, /<textarea[^>]*\brequired\b/, "composer textarea must not set required so file-only submits work");
  assert.match(ui("app.js"), /document\.addEventListener\("keydown",e=>\{\s*if\(e\.defaultPrevented\)return;/, "app.js global shortcuts must respect defaultPrevented");
  assert.match(ui("aurora-rail.js"), /document\.addEventListener\("keydown", e => \{\s*if \(e\.defaultPrevented\) return;/, "aurora-rail.js Ctrl+B must respect defaultPrevented");

  // Behavioral check: Edit-and-resend cancel keeps editing open and preserves draft.
  const drafts = { "edit-m1": "Edited text" };
  let drew = 0;
  const btn = { disabled: false };
  const ctx = {
    $: () => ({ value: "Edited text" }),
    esc,
    toast: () => {},
    drafts,
    CSS: { escape: s => s },
  };
  ctx.globalThis = ctx;
  vm.runInNewContext(ui("message-controls.js") + "\nglobalThis.__MC = MessageControls;", ctx);
  const view = { ui: { editing: "m1" }, section: {}, conv: {}, draw: () => { drew++; } };
  const acts = ctx.__MC.adapter({ id: "m1", text: "Original" }, view, btn, {}, {
    resend: async () => false,
    after: () => { throw new Error("after() must not run when resend is cancelled"); },
  });
  await acts.editSend();
  assert.equal(view.ui.editing, "m1", "cancelling editSend must keep ui.editing open");
  assert.equal(drafts["edit-m1"], "Edited text", "cancelling editSend must preserve edited draft");
  assert.equal(btn.disabled, false, "cancelling editSend must re-enable Send button");
  assert.match(ui("personal-chat.js"), /if \(typeof ask === "function" && !\(await ask\([^)]+\)\)\) return false;/, "personalTransport.resend must return false when cancelled");
}

// 6. Nova A/E/D triage guards & Shortcuts modal coverage.
{
  const novaUx = ui("nova-ux.js");
  assert.match(novaUx, /if \(!isNova\(\) \|\| e\.defaultPrevented \|\| e\.metaKey \|\| e\.ctrlKey \|\| e\.altKey\) return;/, "A/E/D triage must require Nova and ignore defaultPrevented");
  assert.match(novaUx, /if \(document\.querySelector\("dialog\[open\]"\)\) return;/, "A/E/D triage must not fire when a modal dialog is open");
  const appSrc = ui("app.js");
  assert.match(appSrc, /Format composer text/, "Shortcuts modal must document composer formatting shortcuts");
  assert.match(appSrc, /Triage waiting approval/, "Shortcuts modal must document approval triage shortcuts");
}

// 7. Schedule cadence toggling, Waiting filter chip, and weekday Discovery proposals.
{
  const ctx = { esc, App: { tasks: [], schedules: [], inbox: [] }, ENDED: ["completed", "failed", "cancelled"], ExploreIdeas: { open: () => [] } };
  ctx.globalThis = ctx;
  vm.runInNewContext(ui("tasks-schedules.js") + "\nglobalThis.__TS = { scheduleCadenceFields, tasksView };", ctx);
  const wdHtml = ctx.__TS.scheduleCadenceFields({ daily_at: "weekdays 08:30" });
  assert.match(wdHtml, /data-cadence-for="every" hidden/, "weekdays cadence must hide Every (min)");
  assert.match(wdHtml, /data-cadence-for="monthly" hidden/, "weekdays cadence must hide Day of month");
  assert.match(wdHtml, /data-cadence-for="once" hidden/, "weekdays cadence must hide One-time date");
  assert.match(wdHtml, /data-cadence-for="weekly" hidden/, "weekdays cadence must hide Weekly days checkboxes");
  assert.doesNotMatch(wdHtml, /data-cadence-for="weekdays daily weekly monthly" hidden/, "weekdays cadence must show Time");
  assert.match(ui("tasks-schedules.js"), /\["waiting",\s*"Waiting"\]/, "tasksView filter bar must include Waiting chip");

  const onb = ui("onboarding.js");
  assert.match(onb, /id: "d-inbox"[\s\S]*?schedule: \{ weekdays_at: "07:45" \}/, "d-inbox must schedule on weekdays_at");
  assert.match(onb, /id: "d-eng-pr"[\s\S]*?schedule: \{ weekdays_at: "09:00" \}/, "d-eng-pr must schedule on weekdays_at");
}

// 8. No internal "Cedar" jargon in user-facing Nova copy + Open Trash link in Settings.
{
  assert.doesNotMatch(ui("nova-pages.js"), /\bCedar\b/, "nova-pages.js must not expose internal Cedar jargon");
  assert.doesNotMatch(ui("nova-ux.js"), /\bCedar\b/, "nova-ux.js must not expose internal Cedar jargon");
  assert.match(ui("settings-sections.js"), /href="#\/trash"[^>]*>Open Trash<\/a>/, "Settings Data & privacy must include an Open Trash link");
}

console.log("tier 1 ux & trust fixes check ok");
