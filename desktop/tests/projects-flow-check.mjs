// Fast-tier tests for the simplified, quiet Projects flow (projects.js,
// teams.js, team-people.js, team-chat.js, shared-canvas.js):
// - Inline sidebar `+` creates a project without starting any task;
// - Full-stage Project page renders in #main with rename, Add people, Delete,
//   collapsible Instructions, Files, and hides empty Tasks/Routines boilerplate;
// - Direct deletion works from list, sidebar, and header; solo shared projects
//   use a single confirmation (no typed-name ceremony), while multi-member
//   shared projects still confirm by typing the name;
// - Shared project deep links do not auto-open Missions ("Start shared work").
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);

const calls = [];
let askCalls = 0, askTypedCalls = 0, confirmResult = true;
const ctx = {
  UI: {},
  PANEL_TITLES: {},
  PANELS: {},
  Panel: { what: null },
  App: {
    me: { id: "sam", name: "Sam", role: "member" },
    projects: [{ id: "proj_1", name: "Q4 Plan", counts: { files: 1, tasks: 0, schedules: 0 } }],
    workspaces: [{ id: "ws_1", name: "Launch", role: "owner" }],
    threads: [],
  },
  drafts: {},
  route: () => {},
  esc,
  enc: encodeURIComponent,
  firstLine: s => String(s || "").split("\n")[0],
  when: () => "now",
  until: () => "tomorrow",
  kb: n => `${Math.round(n / 1024)} KB`,
  icon: k => `<svg data-ic="${k}"></svg>`,
  attachButton: label => `<button data-attach-btn>${label}</button>`,
  fileChip: f => `<span class="file-chip">${esc(f.name)}</span>`,
  taskRow: t => `<li>${esc(t.title)}</li>`,
  sidebar: () => {},
  closePanel: () => {},
  refresh: async () => {},
  $: () => null,
  $$: () => [],
  paint: () => true,
  toast: () => {},
  ask: async () => { askCalls++; return confirmResult; },
  askTyped: async () => { askTypedCalls++; return confirmResult; },
  Server: {
    projects: {
      create: async b => { calls.push(["POST", "/v1/projects", b]); return { id: "proj_new" }; },
      get: async id => ({ id, name: "Q4 Plan", instructions: "", files: [], threads: [], tasks: [], schedules: [] }),
      update: async (id, b) => { calls.push(["PUT", `/v1/projects/${id}`, b]); return { id }; },
    },
    trash: {
      add: async id => { calls.push(["POST", "/v1/trash", { item: id }]); },
      restore: async id => { calls.push(["POST", `/v1/trash/${id}/restore`]); },
    },
    workspaces: {
      home: async id => id === "ws_solo"
        ? { id: "ws_solo", name: "Solo", role: "owner", me: "sam", members: [{ id: "sam", name: "Sam", role: "owner" }], channels: [] }
        : { id: "ws_team", name: "Team", role: "owner", me: "sam", members: [{ id: "sam", name: "Sam", role: "owner" }, { id: "mo", name: "Mo", role: "member" }], channels: [] },
      remove: async id => { calls.push(["DELETE", `/v1/workspaces/${id}`]); },
      list: async () => [],
    },
  },
  document: { addEventListener: () => {}, querySelector: () => null },
  requestAnimationFrame: fn => fn(),
  location: { hash: "#/projects", origin: "https://server" },
  setTimeout,
  clearTimeout,
  console,
};
ctx.globalThis = ctx;
vm.createContext(ctx);
for (const f of ["projects.js", "teams.js", "team-chat.js"]) {
  vm.runInContext(readFileSync(join(here, "../ui", f), "utf8"), ctx, { filename: f });
}
vm.runInContext("globalThis.__x = { Projects, teamsPageHtml, sharedDeepLinkPanel, channelTools };", ctx);
const { Projects, teamsPageHtml, sharedDeepLinkPanel, channelTools } = /** @type {any} */ (ctx.__x);

// 1. Unified Projects list includes both personal and shared projects with direct delete buttons.
const listHtml = Projects.listHtml(ctx.App.projects, ctx.App.workspaces);
assert.match(listHtml, /data-project-create/, "unified list missing inline create bar");
assert.match(listHtml, /data-del-personal="proj_1"/, "personal project missing delete button in list");
assert.match(listHtml, /data-del-shared="ws_1"/, "shared project missing delete button in list");

// 2. Full-stage Project page has rename, Add people, Delete, collapsed Instructions, and hides empty Tasks/Routines.
const emptyProjHtml = Projects.pageHtml({ id: "proj_1", name: "Q4 Plan", instructions: "", files: [], threads: [], tasks: [], schedules: [] });
assert.match(emptyProjHtml, /data-project-rename="proj_1"/, "project page missing inline rename");
assert.match(emptyProjHtml, /data-project-share="proj_1"/, "project page missing Add people button");
assert.match(emptyProjHtml, /data-project-delete="proj_1"/, "project page missing direct Delete button");
assert.doesNotMatch(emptyProjHtml, /project-instructions" open/, "empty Instructions should be collapsed by default");
assert.doesNotMatch(emptyProjHtml, /<h2>Tasks<\/h2>|<h2>Routines<\/h2>/, "empty Tasks/Routines boilerplate should not clutter a fresh project");

// 3. Project with instructions/tasks renders them.
const richProjHtml = Projects.pageHtml({ id: "proj_1", name: "Q4 Plan", instructions: "Keep it brief", files: [], threads: [], tasks: [{ title: "Review" }], schedules: [] });
assert.match(richProjHtml, /project-instructions" open/, "populated Instructions should open automatically");
assert.match(richProjHtml, /<h2>Tasks<\/h2>/, "populated Tasks should render");

// 4. Deleting a personal project uses a single confirm and calls /v1/trash.
askCalls = 0; askTypedCalls = 0; calls.length = 0;
assert.equal(await Projects.deletePersonal("proj_1", "Q4 Plan"), true);
assert.equal(askCalls, 1, "personal project delete should use one simple confirm");
assert.equal(askTypedCalls, 0, "personal project delete should never require typing the name");
assert.deepEqual(calls[0], ["POST", "/v1/trash", { item: "proj_1" }]);

// 5. Deleting a solo shared project uses a single confirm (no askTyped ceremony);
//    deleting a multi-member shared project uses askTyped.
askCalls = 0; askTypedCalls = 0; calls.length = 0;
assert.equal(await Projects.deleteShared("ws_solo", "Solo"), true);
assert.equal(askCalls, 1, "solo shared project delete should use a simple confirm");
assert.equal(askTypedCalls, 0, "solo shared project delete should not require typing the name");
assert.deepEqual(calls[0], ["DELETE", "/v1/workspaces/ws_solo"]);

askCalls = 0; askTypedCalls = 0; calls.length = 0;
assert.equal(await Projects.deleteShared("ws_team", "Team"), true);
assert.equal(askTypedCalls, 1, "multi-member shared project delete should confirm by name");
assert.deepEqual(calls[0], ["DELETE", "/v1/workspaces/ws_team"]);

// 6. Legacy #/teams list includes direct delete for creators, and project deep link does not auto-open Missions.
const teamsHtml = teamsPageHtml([{ id: "ws_1", name: "Launch", role: "owner" }, { id: "ws_2", name: "GuestProj", role: "member" }], true);
assert.match(teamsHtml, /data-del-shared-ws="ws_1"/, "creator missing delete button on #/teams");
assert.doesNotMatch(teamsHtml, /data-del-shared-ws="ws_2"/, "non-creator should not see delete button on #/teams");
assert.equal(sharedDeepLinkPanel({ kind: "project", id: "p1", tab: "overview" }), null, "project deep link must not auto-open Missions panel");

// 7. Shared conversation header gives creator a direct delete button.
const ownerHome = { id: "ws_1", name: "Launch", role: "owner", me: "sam", agent: { id: "agent", name: "Magnus" }, members: [{ id: "sam", name: "Sam", online: true }] };
const tools = channelTools(ownerHome, { id: "c1", name: "general", mode: "mentions", direct: null }, null);
assert.match(tools, /data-nav-del-shared="ws_1"/, "creator missing direct delete button in shared header");

console.log("projects flow check passed");
