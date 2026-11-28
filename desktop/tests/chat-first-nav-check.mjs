// The app is navigated as a chat agent is (decisions/2026-11-28-chat-first-navigation.md):
// - the sidebar is New chat and Search, then Projects (personal and shared
//   in one list, a shared one marked with the people icon, each opening its
//   own chats by its chevron), then Recents, the chats in no project and the
//   direct ones, newest first; the rest is one row of icons at its foot;
// - a message asked from a project's page is sent, by the chat's composer;
// - Add people shares a project in one step: the people, then Share, which
//   calls POST /v1/projects/{id}/share and invites an email after it;
// - the conversation's Share dialog keeps its summary, without the boundary
//   note or the explanation of what crosses;
// - a private answer that stopped says why, not "Working on it…" forever;
// - no sentence of the UI reassures instead of informing.
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import { readAppBundle } from "./app-bundle.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const uiDir = join(here, "../ui");
const read = f => readFileSync(join(uiDir, f), "utf8");
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const app = readAppBundle();

function extractFn(src, signature) {
  const start = src.indexOf(signature);
  assert.ok(start >= 0, `missing ${signature}`);
  let i = src.indexOf("{", start), depth = 0, q = null;
  for (; i < src.length; i++) {
    const ch = src[i], prev = src[i - 1];
    if (q) { if (ch === q && prev !== "\\") q = null; continue; }
    if (ch === "'" || ch === '"' || ch === "`") { q = ch; continue; }
    if (ch === "{") depth++;
    else if (ch === "}") { depth--; if (depth === 0) return src.slice(start, i + 1); }
  }
  throw new Error(`unclosed ${signature}`);
}

// 1. The sidebar.
const old = new Date(Date.now() - 3 * 86_400_000).toISOString();
const newer = new Date(Date.now() - 3_600_000).toISOString();
const navCtx = {
  esc, enc: encodeURIComponent,
  icon: (k, cls = "") => `<svg class="ic ${cls}" data-ic="${k}"></svg>`,
  displayTitle: t => String(t || ""),
  needsMe: () => [{ id: "n1", thread: "t1" }],
  ENDED: ["completed", "failed", "cancelled"],
  isDev: () => false,
  Theme: { has: k => k === "topBar" },
  location: { hash: "#/chat/t2" },
  App: {
    me: { id: "sam", name: "Sam" }, appliance: "https://server",
    thread: "t2", tasks: [],
    threads: [
      { thread: "t1", title: "Old personal chat", updated_at: old },
      { thread: "t2", title: "Quarter plan", updated_at: newer, project: { id: "p1", name: "Q4" } },
    ],
    projects: [{ id: "p1", name: "Q4" }],
    workspaces: [{ id: "w1", name: "Launch", role: "owner" }],
  },
  Team: { homes: { w1: { id: "w1", me: "sam", me_name: "Sam", members: [{ id: "sam", name: "Sam" }, { id: "mo", name: "Mo", online: true }],
    channels: [
      { id: "c1", name: "general", last_at: new Date().toISOString(), unread: 2 },
      { id: "c2", name: "site-copy", last_at: old },
      { id: "c3", name: "old", archived: true },
      { id: "d1", name: "dm", direct: { people: ["sam", "mo"], agent: false }, last_at: newer },
    ] } } },
  Projects: { inlineNewHtml: () => "" },
  localStorage: { store: {}, getItem(k) { return this.store[k] ?? null; }, setItem(k, v) { this.store[k] = v; } },
  sidebar: () => {},
};
navCtx.globalThis = navCtx;
vm.createContext(navCtx);
const navConsts = ["NAV_PROJECT_CHATS", "NAV_OPEN_KEY", "navShut"].map(n => app.slice(app.indexOf(`const ${n} =`), app.indexOf(";", app.indexOf(`const ${n} =`)) + 1).replace(/^const /, "var ")).join("\n");
vm.runInContext(`${navConsts}\n${extractFn(app, "function navOpen()")}\n${extractFn(app, "function navProjectShown(key, show)")}\n${extractFn(app, "function navigationTree()")}\n${extractFn(app, "function nav()")}`, navCtx, { filename: "app-shell-nav.js" });
const side = vm.runInContext("nav()", navCtx);
const order = ["New chat", 'data-quick-jump', 'aria-label="Projects"', 'aria-label="Recents"', 'class="nav-more"', 'class="foot"'].map(s => side.indexOf(s));
assert.ok(order.every(i => i >= 0) && order.every((v, i) => !i || order[i - 1] < v), `the sidebar is New chat, Search, Projects, Recents, the rest, the person: ${order}`);
assert.match(side, /<span class="nav-label">Search<\/span>/, "Search is named Search");
assert.doesNotMatch(side, /aria-label="People"|Start or join a shared project|Quick Jump|companion-theme-nav|<span>Today<\/span>|<span>Previous<\/span>/, "no People list, Quick Jump, theme button or date groups");
const section = (html, from, to) => html.slice(html.indexOf(from), html.indexOf(to));
const projects = section(side, 'aria-label="Projects"', 'aria-label="Recents"');
assert.match(projects, /href="#\/project\/p1"[^>]*>.*Q4/s, "a personal project is listed");
assert.match(projects, /href="#\/team\/w1"[^>]*>.*Launch.*class="nav-shared"/s, "a shared project is in the same list, with the people mark");
assert.doesNotMatch(projects, /<details/, "a project's chats open by its chevron, not a details element");
// The project of the open chat shows its chats; the other is closed.
assert.match(projects, /data-nav-project="p:p1" aria-expanded="true"/, "the open chat's project shows its chats");
assert.match(projects.slice(projects.indexOf('data-nav-project="p:p1"')), /href="#\/chat\/t2"[^>]*>.*Quarter plan/s, "a project's chat is under it");
assert.match(projects, /data-nav-project="w:w1" aria-expanded="false"/, "another project is closed");
assert.doesNotMatch(projects, /c\/c1"/, "a closed project's chats are not shown");
// Opening it shows its chats, newest first, and not an archived or direct one.
vm.runInContext('navProjectShown("w:w1", true)', navCtx);
const opened = section(vm.runInContext("nav()", navCtx), 'data-nav-project="w:w1"', 'aria-label="Recents"');
const inShared = ['href="#/team/w1/c/c1"', 'href="#/team/w1/c/c2"'].map(h => opened.indexOf(h));
assert.ok(inShared.every((v, i) => v >= 0 && (!i || inShared[i - 1] < v)), `a shared project's chats, newest first: ${inShared}`);
assert.doesNotMatch(opened, /c\/c3"|c\/d1"/, "an archived chat or a direct one is not a project's chat");
assert.match(opened, /<span class="ttl">general<\/span>/, "a project's chat is named by its own name under it");
assert.ok(JSON.parse(navCtx.localStorage.store["magnus.nav.projects.open"]).includes("w:w1"), "an opened project stays open");
// Closing the open chat's project keeps it closed.
vm.runInContext('navProjectShown("p:p1", false)', navCtx);
assert.match(vm.runInContext("nav()", navCtx), /data-nav-project="p:p1" aria-expanded="false"/, "a project closed by hand stays closed");
// Recents: the chats in no project and the direct ones, newest first.
const recents = section(side, 'aria-label="Recents"', 'class="nav-more"');
const at = s => recents.indexOf(s);
assert.ok(at("Old personal chat") >= 0 && at("Mo") >= 0, "Recents holds chats in no project and direct ones");
assert.doesNotMatch(recents, /Quarter plan|c\/c1"|c\/c2"|c\/c3"/, "a project's chat is not listed again in Recents");
const seq = ['href="#/team/w1/c/d1"', 'href="#/chat/t1"'].map(at);
assert.ok(seq.every((v, i) => v >= 0 && (!i || seq[i - 1] < v)), `Recents is newest first across personal and direct: ${seq}`);
const more = side.slice(side.indexOf('class="nav-more"'), side.indexOf('class="foot"'));
for (const href of ["#/inbox", "#/briefing", "#/explore", "#/library", "#/trash"]) assert.match(more, new RegExp(`href="${href}"`), `${href} is in the row at the foot`);
assert.match(more, /class="nav-inbox-count"[^>]*>1</, "the waiting decisions are counted on their icon");

// 2. Asking Magnus in a project sends the message as the chat's first.
const showNewChat = extractFn(app, "async function showNewChat(main, project)");
let submitted = 0;
const form = { requestSubmit: () => { submitted++; } };
const chatCtx = {
  App: { appSearch: "Draft the plan", sendOnOpen: true, draftRecord: { text: "" } },
  chatLayout: async () => ({}),
  paint: () => {}, sidebar: () => {},
  ChatView: { listHtml: () => "" },
  $: q => q === "form.say" ? form : q === "#msg" ? { value: "", setSelectionRange() {} } : { },
};
chatCtx.globalThis = chatCtx;
vm.createContext(chatCtx);
vm.runInContext(showNewChat, chatCtx);
await vm.runInContext("showNewChat({}, 'p1')", chatCtx);
assert.equal(submitted, 1, "the asked message is sent by the composer");
assert.equal(chatCtx.App.draftRecord.text, "Draft the plan");
assert.equal(chatCtx.App.sendOnOpen, false, "and only once");
chatCtx.App.appSearch = "Only start the box";
await vm.runInContext("showNewChat({}, null)", chatCtx);
assert.equal(submitted, 1, "a message started elsewhere waits in the box");

// 3. Add people: one step, then the shared project.
const calls = [];
const shareCtx = {
  esc, enc: encodeURIComponent, icon: () => "",
  App: { me: { id: "sam" } },
  toast: m => calls.push(["toast", m]),
  inviteOutcome: (email, r) => `send ${email} ${r.link}`,
  Server: {
    projects: { share: async (id, people) => { calls.push(["share", id, people]); return { workspace: "w9" }; } },
    workspaces: { invite: async (ws, email) => { calls.push(["invite", ws, email]); return email.startsWith("new") ? { invited: email, emailed: false, link: "https://s/j/x" } : { emailed: true }; } },
  },
};
shareCtx.globalThis = shareCtx;
vm.createContext(shareCtx);
vm.runInContext(`${read("project-share.js")}\nglobalThis.PS = ProjectShare;`, shareCtx);
const PS = shareCtx.PS;
assert.deepEqual([...PS.emails("a@b.co, new@c.io  not-an-email")], ["a@b.co", "new@c.io"]);
const html = PS.dialogHtml([{ id: "mo", name: "Mo" }], "Q4");
assert.equal((html.match(/<button[^>]*type="submit"/g) || []).length, 1, "one button");
assert.match(html, />Share<\/button>/);
assert.match(html, /name="person" value="mo"/, "the directory's people");
assert.match(html, /name="emails"/, "an email box for someone new");
assert.doesNotMatch(html, /<select|proj_file|copy_instructions|copy_invite|new_ws_name|stays private/, "no picker, name, documents, instructions or invite-link steps");
assert.equal(await PS.share("p1", ["mo"], ["a@b.co", "new@c.io"]), "#/team/w9");
assert.deepEqual(JSON.parse(JSON.stringify(calls.slice(0, 3))), [["share", "p1", ["mo"]], ["invite", "w9", "a@b.co"], ["invite", "w9", "new@c.io"]]);
assert.match(calls[3]?.[1] || "", /send new@c.io https:\/\/s\/j\/x/, "a link no email carried is given to send");
const server = read("server.js");
assert.match(server, /share: \(id, people\) => post\(`\/v1\/projects\/\$\{enc\(id\)\}\/share`, \{ people \}\)/, "Server.projects.share posts the people");
assert.match(server, /rename: \([^)]*\) => patch\(ws\(w\), \{ name \}\)/, "Server.workspaces.rename patches the name");
assert.match(app, /data-project-share[\s\S]{0,200}ProjectShare\.open\(/, "Add people opens the one-step share");

// 4. The conversation's Share dialog: the summary stays; the boundary note and explanation go.
const canvas = read("shared-canvas.js");
assert.match(canvas, /name="summary"/, "the summary box stays");
assert.doesNotMatch(canvas, /share-boundary-note|What crosses|stays private|stayed private|projectId|isProjectPromote/, "no boundary note, explanation or project copy");

// 5. A private answer that stopped says why.
const tcCtx = { esc };
tcCtx.globalThis = tcCtx;
vm.createContext(tcCtx);
vm.runInContext(extractFn(read("team-conversation.js"), "function whisperHtml(w)"), tcCtx);
const stopped = vm.runInContext(`whisperHtml({ task: "k1", done: true, text: null, held_because: "it read content from outside" })`, tcCtx);
assert.match(stopped, /it read content from outside/);
assert.doesNotMatch(stopped, /Working on it/, "a stopped run is not working");
assert.match(stopped, /data-not-now="k1"/, "and it can be put away");
assert.match(vm.runInContext(`whisperHtml({ task: "k2", done: false })`, tcCtx), /Working on it…/);
assert.match(vm.runInContext(`whisperHtml({ task: "k3", done: true, text: "Done: 3 rows" })`, tcCtx), /data-whisper-share="k3"/);

// 6. Nothing reassures instead of informing.
const banned = [/nothing runs until/i, /Magnus waits until you ask/i, /Start one when you.re ready/i, /Your private (project|conversation) stays private/i,
  /spaces? (are|is) untouched/i, /stayed private/i, /stay untouched/i, /never touched/i, /Nothing starts until/i, /No one else sees these/i,
  /works in private and shares scrubbed/i, /it holds data only you may see/i, /New shared chat name/i];
for (const f of readdirSync(uiDir).filter(f => f.endsWith(".js"))) {
  const src = read(f);
  for (const b of banned) assert.doesNotMatch(src, b, `${f} still says ${b}`);
}
console.log("chat-first navigation check passed");
