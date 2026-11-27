// Wave C regression checks: Morning Briefing card, Notification Center & DND,
// explicit Steering pill, safe Edit confirmation, Tasks search/filter/bulk
// clear, block-level Edit with Magnus, deterministic receipt Undo, Ask Magnus
// about file, inline project rename, and sidebar conversation quick delete.
import assert from "node:assert/strict";
import { readAppBundle } from "./app-bundle.mjs";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const uiDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../ui");
const read = f => fs.readFileSync(path.join(uiDir, f), "utf8");

const appSrc = readAppBundle();
const noticesSrc = read("notices.js");
const composerSrc = read("chat-composer.js");
const framesSrc = read("composed-frames.js");
const personalSrc = read("personal-chat.js");
const tasksSrc = read("tasks-schedules.js");
const evidenceSrc = read("evidence.js");
const serverSrc = read("server.js");

const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

// 1. Server.briefing & Server.tasks.undo exist in server.js.
assert.match(serverSrc, /briefing\s*:\s*\(\)\s*=>\s*get\("\/v1\/briefing"\)/, "Server.briefing endpoint wrapper missing");
assert.match(serverSrc, /undo\s*:\s*\([^)]*id[^)]*callId\)\s*=>\s*post\(`\/v1\/tasks\/\$\{enc\(id\)\}\/undo\/\$\{enc\(callId\)\}`/, "Server.tasks.undo endpoint wrapper missing");

// 2. Notices: history cap, DND snooze/resume, and Notification Center HTML.
{
  const store = new Map();
  let popups = 0;
  const ctx = vm.createContext({
    esc,
    $: () => null,
    document: { hidden: false, hasFocus: () => true, createElement: () => ({}), body: { append() {} } },
    location: { hash: "#/home" },
    localStorage: {
      getItem: k => (store.has(k) ? store.get(k) : null),
      setItem: (k, v) => store.set(k, String(v)),
      removeItem: k => store.delete(k),
    },
    Magnus: { kind: "desktop", notifyPermission: () => "denied" },
    Server: { notices: async () => ({ now: "2026-11-24T12:00:00Z", notices: [], names: {} }) },
    teamHref: () => "#/team/w1",
    setTimeout,
    clearTimeout,
  });
  vm.runInContext(`${noticesSrc}\nthis.Notices = Notices;`, ctx);
  const { Notices } = ctx;
  Notices.popup = () => { popups++; };

  assert.equal(Notices.isDnd(), false, "DND starts off");
  Notices.show({ key: "k1", title: "Task finished", body: "Weekly report ready", system: "Task finished", href: "#/task/t1" });
  assert.equal(popups, 1, "popup fires when DND is off");
  assert.equal(Notices.history.length, 1, "notification recorded in history");

  Notices.snooze(60);
  assert.equal(Notices.isDnd(), true, "DND turns on after snooze(60)");
  Notices.show({ key: "k2", title: "Background note", body: "Snoozed", system: "Background", href: "#/task/t2" });
  assert.equal(popups, 1, "popup suppressed during DND");
  assert.equal(Notices.history.length, 2, "history still captures notification during DND");
  const htmlDnd = Notices.centerHtml();
  assert.match(htmlDnd, /Resume alerts/, "DND button shows Resume alerts when active");
  assert.match(htmlDnd, /Task finished/, "Notification Center lists history items");

  Notices.snooze(0);
  assert.equal(Notices.isDnd(), false, "snooze(0) clears DND");
  assert.match(Notices.centerHtml(), /Snooze 1h/, "DND button shows Snooze 1h when off");

  Notices.clearHistory();
  assert.equal(Notices.history.length, 0, "clearHistory empties list");
  assert.match(Notices.centerHtml(), /No recent notifications/, "empty state shown after clear");
}

// 3. Tasks search, status filters, and bulk clear in tasks-schedules.js.
{
  const ctx = vm.createContext({
    esc,
    enc: encodeURIComponent,
    when: () => "2m ago",
    until: () => "in 1h",
    firstLine: s => String(s || "").split("\n")[0],
    chip: s => `<span class="chip">${s}</span>`,
    icon: () => "<svg/>",
    WAITING: { approval: "Waiting on approval" },
    ENDED: ["completed", "failed", "cancelled"],
    ExploreIdeas: { open: () => [] },
    App: {
      tasks: [
        { task_id: "t1", title: "Quarterly budget audit", text: "Audit Q4", status: "running", created_at: "2026-11-24T10:00:00Z" },
        { task_id: "t2", title: "Send vendor invoice", text: "Invoice ACME", status: "completed", created_at: "2026-11-24T09:00:00Z", ended_at: "2026-11-24T09:05:00Z" },
        { task_id: "t3", title: "Nightly sync", text: "Sync CRM", status: "failed", created_at: "2026-11-24T08:00:00Z", ended_at: "2026-11-24T08:02:00Z" },
      ],
      schedules: [],
      inbox: [],
    },
  });
  vm.runInContext(`${tasksSrc}\nthis.tasksView = tasksView; this.TaskFilter = TaskFilter;`, ctx);
  const allHtml = ctx.tasksView();
  assert.match(allHtml, /id="tasks-q"/, "tasks search input rendered");
  assert.match(allHtml, /data-task-filter="running"/, "status filter chips rendered");
  assert.match(allHtml, /data-clear-ended-tasks/, "bulk clear ended button rendered");
  assert.match(allHtml, /Clear ended \(2\)/, "bulk clear shows ended count");

  ctx.TaskFilter.state = "running";
  const runningHtml = ctx.tasksView();
  assert.match(runningHtml, /Quarterly budget audit/, "running filter keeps running task");
  assert.doesNotMatch(runningHtml, /Send vendor invoice/, "running filter hides completed task");

  ctx.TaskFilter.state = "all";
  ctx.TaskFilter.q = "invoice";
  const searchHtml = ctx.tasksView();
  assert.match(searchHtml, /Send vendor invoice/, "search matches title/text");
  assert.doesNotMatch(searchHtml, /Quarterly budget audit/, "search filters non-matching tasks");
}

// 4. Morning Briefing card & app.js Wave C hooks.
{
  const briefingFn = appSrc.match(/function briefingCard\(\) \{[\s\S]*?\n\}/)?.[0];
  assert.ok(briefingFn, "briefingCard() exists in app.js");
  const ctx = vm.createContext({ esc, when: () => "yesterday", App: { briefing: null } });
  vm.runInContext(`${briefingFn}\nthis.briefingCard = briefingCard;`, ctx);
  assert.equal(ctx.briefingCard(), "", "no briefing card when App.briefing is null");

  ctx.App.briefing = {
    needs_count: 2,
    staged_count: 1,
    active_count: 1,
    completed_24h: 4,
    overdue_count: 1,
    overdue: [{ what: "Send Q4 deck", to: "Board", by: "2026-11-23T17:00:00Z" }],
  };
  const bHtml = ctx.briefingCard();
  assert.match(bHtml, /data-briefing/, "briefing section rendered");
  assert.match(bHtml, /<strong>2<\/strong><span>waiting<\/span>/, "waiting count rendered");
  assert.match(bHtml, /<strong>1<\/strong><span>drafts<\/span>/, "drafts count rendered");
  assert.match(bHtml, /<strong>4<\/strong><span>done 24h<\/span>/, "completed_24h count rendered");
  assert.match(bHtml, /Send Q4 deck/, "overdue commitment listed in briefing");
}

// 5. Structural assertions for composer steer pill, safe edit confirm, block edit, receipt undo, file ask, project rename, sidebar conv delete.
assert.match(composerSrc, /data-steer-pill[^>]*>Steering active task</, "composer has explicit Steering active task pill");
assert.match(personalSrc, /await ask\("Edit and resend this message\?/, "personalChat edit-and-resend confirms before removing later replies");
assert.match(framesSrc, /data-ui-edit-block=/, "composed-frames adds block-level Edit with Magnus button");
assert.match(evidenceSrc, /Server\.tasks\.undo\(taskId,\s*callId\)/, "evidence receipt Undo calls Server.tasks.undo");
assert.match(appSrc, /Server\.briefing\(\)/, "refresh() fetches Server.briefing()");
assert.match(appSrc, /data-toggle-notices/, "convHead includes Notification Center bell button");
assert.match(appSrc, /data-act="ask-file"/, "fileView includes Ask Magnus button");
assert.match(appSrc, /data-rename-project=/, "projectView includes inline rename form");
assert.match(appSrc, /class="nav-conv-del"\s+data-trash=/, "sidebar navigation includes conversation quick delete button");

console.log("wave-c-check: ok");
