// Fast-tier tests for the unified multiplayer architecture (shared-canvas.js):
// - SharedCanvas.boxes() surfaces private Release Airlock drafts ONLY to the lane owner;
// - SharedCanvas.dockItems() exposes context-aware shared Views popover entries;
// - SharedCanvas.cleanRoomPayload() enforces clean-room boundary (never leaks unselected turns);
// - SharedCanvas.view() renders Missions, Tasks, Files, Routines, Knowledge, and Project settings
//   natively in the single Canvas (#detail).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const home = {
  id: "ws1",
  name: "Launch Q4",
  role: "owner",
  me: "bob",
  me_name: "Bob",
  agent: { id: "agent", name: "Magnus" },
  computer: "comp-ws1",
  members: [
    { id: "bob", name: "Bob", role: "owner", online: true },
    { id: "alice", name: "Alice", role: "member", online: true },
  ],
  channels: [
    { id: "c1", name: "general", project: "p1", mode: "mentions", notify: "all", unread: 0, mentions: 0, direct: null },
  ],
  projects: [{ id: "p1", name: "Launch Q4", owner: "bob", members: ["bob", "alice"], progress: { done: 1, total: 2 }, channels: ["c1"] }],
  spend: { spent_usd: 12, limit_usd: 50, channels: {} },
  apps: [],
};
const missions = [{
  id: "m1",
  goal: "Friday Board Update",
  complete: false,
  released_count: 1,
  total_lanes: 2,
  lanes: [
    { id: "l1", kind: "member_agent", owner: "alice", objective: "Pipeline", status: "released", contribution: "$4.2M" },
    { id: "l2", kind: "member_agent", owner: "bob", objective: "Blockers", scope: "weekly_blockers", status: "waiting_release", private_draft: "TOP_SECRET_BOB" },
  ],
}];

const panelData = {
  project: home.projects[0],
  channels: ["c1"],
  files: [{ name: "brief.pdf", size: 2048, by: "bob", by_name: "Bob", project: "p1" }],
  tasks: [{ id: "t1", project: "p1", title: "Ship release", status: "doing", assignee: "bob", from: null, by: "bob" }],
  work: [],
  delegations: [],
  routines: [{ id: "r1", channel: "c1", text: "Weekly digest", every_hours: 168, stopped: null }],
  members: home.members,
  knowledge: 1,
  progress: { done: 1, total: 2 },
  computer: "comp-ws1",
};

const ctx = {
  UI: {},
  PANEL_TITLES: {},
  PANELS: {},
  Panel: { what: null },
  App: { me: { id: "bob", name: "Bob" }, shared: { ws: "ws1", channel: "c1", project: "p1", home, missions } },
  route: () => {},
  esc,
  enc: encodeURIComponent,
  firstLine: s => String(s || "").split("\n")[0],
  when: () => "now",
  kb: n => `${Math.round(n / 1024)} KB`,
  icon: k => `<svg data-ic="${k}"></svg>`,
  $: () => null,
  $$: () => [],
  paint: () => true,
  toast: () => {},
  ask: async () => true,
  askText: async () => "fix",
  Compose: { markdown: s => esc(s), maskedText: s => esc(s) },
  Magnus: { kind: "web" },
  Server: {
    workspaces: {
      panel: async () => panelData,
      missions: async () => missions,
      capabilities: async () => [{ id: "alice", name: "Alice", apps: [{ name: "linear", title: "Linear" }] }],
      people: async () => [{ id: "carol", name: "Carol" }],
      knowledge: async () => [{ id: "k1", kind: "decision", statement: "Launch Tuesday", source: "chat", weight: "team", author: "bob", owner: "bob", thread: null, reviewed_at: "2026-11-22T00:00:00Z" }],
      activity: async () => ({ mentions: [], replies: [], asks: [] }),
    },
  },
  document: { addEventListener: () => {} },
  location: { hash: "#/team/ws1/c/c1", origin: "https://server" },
  setTimeout,
  clearTimeout,
  console,
};
ctx.globalThis = ctx;
vm.createContext(ctx);
for (const f of ["side-panel.js", "user-avatar.js", "coedit.js", "teams.js", "team-chat.js", "team-panel.js", "team-people.js", "team-command.js", "shared-canvas.js"]) {
  vm.runInContext(readFileSync(join(here, "../ui", f), "utf8"), ctx, { filename: f });
}
vm.runInContext("globalThis.__x = { SharedCanvas, Team, PANELS };", ctx);
const { SharedCanvas, Team, PANELS } = /** @type {any} */ (ctx.__x);
Team.home = home;
Team.ws = "ws1";

// 1. Native Bento tiles: Missions are retired, so shared conversations stay quiet like private chat.
assert.equal(SharedCanvas.boxes().length, 0, "retired Missions must not emit Bento tiles");

// 2. Context-aware Views popover focuses on Files and Project settings (no Missions/Kanban/KB clutter).
const dock = SharedCanvas.dockItems("ws1");
assert.deepEqual([...dock.map(d => d[0])], ["shared:files", "shared:settings"]);
assert.deepEqual([...dock.map(d => d[3])], ["Files", "Project settings"]);

// 3. Clean-room Share payload never leaks unselected private messages.
const msgs = [
  { id: "e1", kind: "message", text: "PRIVATE_DRAFT_SECRET" },
  { id: "e2", kind: "answer", text: "Public summary ready for teammates" },
];
assert.equal(SharedCanvas.cleanRoomPayload({ mode: "summary", summary: "  Clean deliverable  ", messages: msgs, selectedIds: ["e1"] }), "Clean deliverable");
const selOnly = SharedCanvas.cleanRoomPayload({ mode: "selected", summary: "ignored", messages: msgs, selectedIds: ["e2"] });
assert.match(selOnly, /Public summary ready for teammates/);
assert.doesNotMatch(selOnly, /PRIVATE_DRAFT_SECRET/, "unchecked private message leaked into clean-room payload");
assert.doesNotMatch(selOnly, /\bmagnus\b/i, "a shared answer names the agent, so posting it would start the sharer's agent");
assert.equal(SharedCanvas.cleanRoomPayload({ mode: "selected", messages: msgs, selectedIds: [] }), "", "empty selection shares nothing");

// 4. Unified Canvas renders shared views via PANELS.shared; legacy "missions" falls back cleanly to Files.
assert.equal(typeof PANELS.shared, "function", "PANELS.shared is registered");
const vLegacy = await PANELS.shared("missions");
assert.match(vLegacy.head, /Files/);
assert.doesNotMatch(vLegacy.body, /data-mission-new/, "retired Start shared work mission form must not render");

const vTasks = await PANELS.shared("tasks");
assert.match(vTasks.head, /Tasks/);
assert.match(vTasks.body, /Ship release/);

const vFiles = await PANELS.shared("files");
assert.match(vFiles.head, /Files/);
assert.match(vFiles.body, /brief\.pdf/);

const vRoutines = await PANELS.shared("routines");
assert.match(vRoutines.head, /Routines/);
assert.match(vRoutines.body, /Weekly digest/);

const vKnowledge = await PANELS.shared("knowledge");
assert.match(vKnowledge.head, /Knowledge/);
assert.match(vKnowledge.body, /Launch Tuesday/);

const vSettings = await PANELS.shared("settings");
assert.match(vSettings.head, /Project settings/);
assert.match(vSettings.body, /data-people-input/);
assert.match(vSettings.body, /data-quick-add="carol"/);
assert.match(vSettings.body, /data-invite-link/);
assert.match(vSettings.body, /Linear/);
assert.match(vSettings.body, /Danger zone/);
assert.doesNotMatch(vSettings.body, /data-shared-limit|data-role|data-team-apps/, "retired Shared Spend, role dropdowns, or Shared Apps leaked into Project settings");

console.log("shared canvas check passed");
