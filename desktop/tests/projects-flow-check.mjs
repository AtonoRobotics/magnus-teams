// Fast-tier tests for the simplified, quiet Projects flow (projects.js,
// teams.js, team-people.js, team-chat.js, shared-canvas.js):
// - Inline sidebar `+` creates a project without starting any task;
// - Full-stage Project page renders in #main with rename, Add people, Delete,
//   collapsible Instructions, Files, and hides empty Tasks/Routines boilerplate;
// - Direct deletion works from list, sidebar, and header; solo shared projects
//   use a single confirmation (no typed-name ceremony), while multi-member
//   shared projects still confirm by typing the name;
// - Shared project deep links do not auto-open Missions ("Start shared work");
// - Personal and shared project homes are one page: the name (editable), a
//   composer that sends, the chats, files and instructions, and People only
//   on the shared one; a shared project's composer is there with no chat,
//   and a chat is never named before the first message
//   (decisions/2026-11-28-chat-first-navigation.md).
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
    channels: { post: async (ws, ch, b) => { calls.push(["POST", `/v1/workspaces/${ws}/channels/${ch}`, b]); return {}; } },
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
      start: async (ws, b) => { calls.push(["START", ws, b]); return { channel: `ch_${b.name.replace(/\W+/g, "-")}` }; },
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
const ownerHome = { id: "ws_1", name: "Launch", role: "owner", me: "sam", agent: { id: "agent", name: "Magnus" }, members: [{ id: "sam", name: "Sam", role: "owner", online: true }], channels: [{ id: "c1", name: "general", direct: null }] };
const tools = channelTools(ownerHome, { id: "c1", name: "general", mode: "mentions", direct: null }, null);
assert.match(tools, /data-nav-del-shared="ws_1"/, "creator missing direct delete button in shared header");

// 8. Unified Shared Project Home renders shared chats, shared documents, instructions, people, and hero ask composer (no Missions).
const sharedHtml = Projects.sharedPageHtml({ ...ownerHome, instructions: "Keep updates concise." }, { files: [{ name: "spec.pdf", size: 2048, by_name: "Sam" }] });
assert.match(sharedHtml, /data-shared-project-page="ws_1"/, "shared project home missing root");
assert.match(sharedHtml, /data-shared-project-ask="ws_1"/, "shared project home missing hero composer");
assert.doesNotMatch(sharedHtml, /data-shared-new-chat|New shared chat name/, "a chat is not named before anyone talks");
assert.match(sharedHtml, /data-shared-rename="ws_1"[\s\S]*class="project-title-input"/, "the shared project's name is edited in place, as a personal one's");
assert.match(sharedHtml, /spec\.pdf/, "shared project home missing shared documents");
assert.match(sharedHtml, /project-instructions" open/, "populated shared instructions should open automatically");
assert.match(sharedHtml, /data-shared-save-ins="ws_1"/, "shared project home missing instructions save button");
assert.match(sharedHtml, /data-shared-project-delete="ws_1"/, "shared project home missing creator delete");
assert.doesNotMatch(sharedHtml, /data-mission-new|Missions/, "shared project home must not show Missions");
assert.deepEqual(JSON.parse(JSON.stringify(ctx.teamPlace(""))), { kind: "home" }, "bare #/team/:ws opens Shared Project Home");

// 9. One project home: the same parts, and People only on the shared one.
const parts = h => ["project-rename-form", "project-hero-composer", "<h2>Chats</h2>", "<h2>Files</h2>", "project-instructions", "<h2>People</h2>"].filter(p => h.includes(p));
assert.deepEqual(parts(emptyProjHtml), ["project-rename-form", "project-hero-composer", "<h2>Chats</h2>", "<h2>Files</h2>", "project-instructions"]);
assert.deepEqual(parts(sharedHtml), [...parts(emptyProjHtml), "<h2>People</h2>"]);
const memberHtml = Projects.sharedPageHtml({ ...ownerHome, role: "member", channels: [] }, { files: [] });
assert.match(memberHtml, /data-shared-project-ask="ws_1"/, "a shared project with no chat still has its composer");
assert.match(memberHtml, /data-shared-rename/, "a member renames the shared project (decisions/2026-11-28-projects-without-hoops.md)");
assert.doesNotMatch(memberHtml, /data-shared-project-delete/, "only its admins delete it");
assert.match(memberHtml, /class="project-title-input" name="name" value="Launch"/);
for (const h of [emptyProjHtml, sharedHtml, listHtml, Projects.listHtml([], [])]) {
  assert.doesNotMatch(h, /nothing runs|Magnus waits|when you're ready|untouched|Optional|Active/, "nothing reassures or explains");
}

// 10. Asking Magnus in a personal project sends: the new chat's composer sends it.
ctx.App.appSearch = null; ctx.App.sendOnOpen = false;
Projects.askPersonal("proj_1", "Draft the Q4 plan");
assert.equal(ctx.location.hash, "#/chat/new/proj_1");
assert.equal(ctx.App.appSearch, "Draft the Q4 plan");
assert.equal(ctx.App.sendOnOpen, true, "asked, not only typed");
ctx.App.appSearch = null; ctx.App.sendOnOpen = false;
Projects.askPersonal("proj_1", "");
assert.equal(ctx.App.sendOnOpen, false, "an empty ask only opens the new chat");

// 11. Asking in a shared project: a chat named by its first message, or general when it has none.
calls.length = 0;
const withGeneral = { ...ownerHome, channels: [{ id: "c1", name: "general", direct: null }] };
assert.equal(await Projects.askShared("ws_1", withGeneral, "Draft the launch plan for Friday please now"), "#/team/ws_1/c/ch_Draft-the-launch-plan-for-Friday");
assert.deepEqual(JSON.parse(JSON.stringify(calls.slice(0, 2))), [
  ["START", "ws_1", { kind: "channel", name: "Draft the launch plan for Friday" }],
  ["POST", "/v1/workspaces/ws_1/channels/ch_Draft-the-launch-plan-for-Friday", { text: "@Magnus Draft the launch plan for Friday please now" }],
]);
calls.length = 0;
assert.equal(await Projects.askShared("ws_1", { ...ownerHome, channels: [] }, "@Magnus hello"), "#/team/ws_1/c/ch_general");
assert.deepEqual(JSON.parse(JSON.stringify(calls.slice(0, 2))), [
  ["START", "ws_1", { kind: "channel", name: "general" }],
  ["POST", "/v1/workspaces/ws_1/channels/ch_general", { text: "@Magnus hello" }],
], "with no chat, the message goes to the project's general chat");
calls.length = 0;
const archived = { ...ownerHome, channels: [{ id: "c9", name: "general", archived: true, direct: null }] };
assert.equal(await Projects.askShared("ws_1", archived, "status?"), "#/team/ws_1/c/c9", "the default channel the server made");
assert.deepEqual(calls.map(c => c[0]), ["POST"]);

console.log("projects flow check passed");
