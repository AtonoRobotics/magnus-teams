// One chat view (chat-view.js) for the personal chat and a team channel
// (owner, 2026-11-13: "Teams is a multi user version of the single chat
// interface"; "Not the same system"). Both conversations meet the view's
// contract and the composer's; the same kinds of item come out as the same
// markup; what several people add is absent with one; and no second
// renderer or composer is left in the team's files.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const sandbox = {
  UI: {}, route: () => {}, esc, voice: { entry: null }, ICONS: new Proxy({}, { get: () => "" }), App: { canReadAloud: true }, enc: encodeURIComponent, $: () => null, $$: () => [], paint: () => true,
  when: t => (t ? "at" : ""), firstLine: s => String(s || "").split("\n")[0], icon: k => `<svg data-icon="${k}"></svg>`, chip: st => `<span class="chip">${st}</span>`,
  statusLineSlot: (t, cls) => `<span class="${cls}">${t.status_line}</span>`, filePreview: f => `<div class="fprev">${esc(f.name)}</div>`, shownFrame: () => "", entry: e => `<div data-personal-only="${e.kind}"></div>`,
  toast: () => {}, ask: async () => true, Magnus: {}, Compose: { markdown: s => `<p>${esc(s)}</p>`, maskedText: s => esc(s) }, api: async () => ({}),
  document: { addEventListener: () => {} }, location: { hash: "" }, setTimeout, clearTimeout, console,
};
sandbox.globalThis = sandbox;
const files = ["message-controls.js", "chat-view.js", "chat-composer.js", "personal-chat.js", "teams.js", "team-chat.js", "team-conversation.js"].map(ui);
vm.runInNewContext(`${files.join("\n")}\nglobalThis.__x = { ChatView, Team };`, sandbox);

// An item as the server sends it (crates/magnus/src/team/items.rs): its kind and files.
const served = m => ({ ...m, kind: m.kind || (m.deleted ? "deleted" : m.sender !== "agent" ? "message" : m.text ? "answer" : "work"), files: m.files || String(m.text || "").split("\n").filter(l => l.startsWith("📎 ")).map(l => l.slice(2).trim()) });
const { ChatView, Team } = sandbox.__x;
Team.home = { id: "w", role: "member", me: "sam", me_name: "Sam", agent: { id: "agent", name: "Magnus" }, members: [{ id: "sam", name: "Sam" }, { id: "dana", name: "Dana" }], channels: [{ id: "c1", name: "general", direct: null }], projects: [] };
const state = { ws: "w", channel: "c1", view: { messages: [], names: {} }, files: {}, pinsOnly: false, whispers: [], hidden: new Set(), unreadAtOpen: 0, typedAt: 0 };

// The contract: each conversation says where its items come from, what a
// message is to its controls, what the viewer may do and what each control
// does; each composer, where files and messages go.
const personal = sandbox.personalChat("th1"), team = sandbox.teamConversation(state, null), thread = sandbox.teamConversation(state, "m1");
for (const [label, conv] of [["the personal chat", personal], ["a team channel", team], ["a team thread", thread]]) {
  for (const k of ["load", "describe", "caps", "actions"]) assert.equal(typeof conv[k], "function", `${label} has no ${k}`);
}
for (const [label, c] of [["the personal chat", sandbox.personalComposer()], ["a team channel", sandbox.teamComposer(state, null)]]) {
  for (const k of ["upload", "send", "refresh"]) assert.equal(typeof c[k], "function", `${label}'s composer has no ${k}`);
}
// A conversation of several people names them; the personal chat does not.
assert.ok(!ChatView.several(personal) && ChatView.several(team), "who is in a conversation is read wrongly");

const draw = (conv, e) => ChatView.item(e, { conv, ui: { editing: null, sharing: null, people: [] } });
const skeleton = html => html.replace(/<svg[^]*?<\/svg>/g, "").replace(/>[^<]*</g, "><").replace(/ (title|aria-label|data-tooltip|data-voice-entry|id|aria-controls|data-menu|data-entry|data-task)="[^"]*"/g, "");
const reader = sandbox.teamReader(state, null);

// One's own message: the same entry, bubble and controls in both.
const mine = draw(personal, { id: "e1", kind: "message", text: "Hello", at: "t" });
const mineTeam = draw(team, sandbox.teamEntry(served({ id: "m1", sender: "sam", text: "Hello", thread: null, at: "t" }), reader));
assert.match(mine, /^<div class="entry me"[^>]*><div class="msg me"><div class="small faint byline">You · at<\/div><div>Hello<\/div><\/div>/, "the personal chat's own message changed");
assert.match(mineTeam, /^<div class="entry me"[^>]*><div class="msg me"><div class="small faint byline"><span class="team-avatar"[^]*<\/span>You · at<\/div><div>Hello<\/div><\/div>/, "a team message of one's own is not the personal chat's");
assert.equal(skeleton(mineTeam.replace(/<span class="team-avatar"[^]*?<\/span>/, "")).replace(/<div class="msg-actions[^]*$/, ""), skeleton(mine).replace(/<div class="msg-actions[^]*$/, ""), "one's own message is drawn two ways");

// Magnus's answer: the same, with the requester named in a team.
const answer = draw(personal, { id: "e2", kind: "answer", text: "Done", at: "t", task_id: "t1" });
const answerTeam = draw(team, sandbox.teamEntry(served({ id: "a1", sender: "agent", requester: "dana", task: "t1", text: "Done", thread: null, at: "t" }), reader));
assert.match(answer, /^<div class="entry them"[^>]*><div class="msg magnus" data-answer data-task="t1"><div class="small faint byline">Magnus · at<\/div><div class="answer-body"><p>Done<\/p><\/div><\/div><div class="msg-actions small"/, "the personal chat's answer changed");
assert.match(answerTeam, /^<div class="entry them"[^>]*><div class="msg magnus" data-answer data-task="t1"><div class="small faint byline">[^]*Magnus · for Dana · at<\/div><div class="answer-body"><p>Done<\/p><\/div><\/div><div class="msg-actions small"/, "a team answer is not the personal chat's");
// Both answers' controls are the one module's row and menu.
for (const html of [answer, answerTeam]) assert.match(html, /class="icon menu-trigger" data-menu="message-menu-/, "an answer's controls are not the one row and menu");

// Work: the personal chat's task row, in a team the requester named.
const taskRow = ChatView.workRow({ id: "t1", status: "running", icon: "working", pill: "", title: "Plan", sub: "", href: "#/task/t1" });
const teamRow = draw(team, sandbox.teamEntry(served({ id: "w1", sender: "agent", requester: "dana", text: "", thread: null, at: "", work: { task: "t1", requester: "dana", status: "running", steps: 0, doing: null, title: "Plan", long: false, thread: null, reason: null } }), reader));
assert.match(taskRow, /^<li class="tcard" data-chat-task="t1" data-status="running"><a class="rg-row"/);
assert.match(teamRow, /<div class="reqgroup chatcard" data-chat-work="t1"><ul class="rg-rows"><li class="tcard" data-chat-task="t1" data-status="running"><div class="rg-row"/, "team work is not the personal chat's task row");

// What only the personal chat has (its cards) goes to its own drawing.
assert.match(draw(personal, { id: "e3", kind: "suggestion_card" }), /data-personal-only="suggestion_card"/);
// Mentions are marked only where there are people to mention.
assert.doesNotMatch(draw(personal, { id: "e4", kind: "message", text: "mail @dana", at: "" }), /class="mention"/, "the personal chat marks mentions");
assert.match(draw(team, sandbox.teamEntry(served({ id: "m4", sender: "dana", text: "hi @Sam", thread: null, at: "" }), reader)), /class="mention">@Sam/);
// The word after a name is not part of it unless someone has both.
assert.match(draw(team, sandbox.teamEntry(served({ id: "m5", sender: "sam", text: "@Magnus Plan the launch", thread: null, at: "" }), reader)), /<span class="mention">@Magnus<\/span> Plan the launch/, "the word after a mention was taken into it");
Team.home.members.push({ id: "ds", name: "Dana Smith" });
assert.match(draw(team, sandbox.teamEntry(served({ id: "m6", sender: "sam", text: "ask @Dana Smith", thread: null, at: "" }), reader)), /<span class="mention">@Dana Smith<\/span>/, "a two-word name is not one mention");
Team.home.members.pop();

// The server says what an item is and what it carries; the app does not guess.
const told = sandbox.teamEntry({ id: "a2", kind: "answer", sender: "agent", text: "", files: ["plan.pdf"], thread: null, at: "" }, reader);
assert.equal(told.kind, "answer", "the app guessed an item's kind");
assert.deepEqual(JSON.parse(JSON.stringify(told.files)), [{ name: "plan.pdf" }], "the app did not take the server's files");

// The one view and the one composer: no team copy of either is left.
for (const f of ["team-chat.js", "team-conversation.js"]) {
  const src = ui(f);
  assert.doesNotMatch(src, /<article|class="tm[ "-]|<textarea|<form/, `${f} still draws its own messages or composer`);
}
assert.doesNotMatch(ui("teams.css"), /\.tm\b|\.tm-|team-composer|team-mentions/, "the team's own chat styles are still there");
for (const f of ["chat-view.js", "chat-composer.js", "personal-chat.js", "team-conversation.js"]) assert.match(ui("index.html"), new RegExp(`<script src="${f}"></script>`), `the page does not load ${f}`);
assert.doesNotMatch(ui("app.js"), /function wireComposer|function msgActions|case "message": return|case "answer": return/, "app.js keeps a second chat view");
console.log("chat view ok");
