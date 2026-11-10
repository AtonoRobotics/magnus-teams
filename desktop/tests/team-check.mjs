// The team workspace as teams.js, team-chat.js, team-panel.js and
// team-command.js draw it: the rail, messages with the agent's live work,
// optimistic changes, mentions, the board and the command menu.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const calls = [];
const plain = x => JSON.parse(JSON.stringify(x));
let reply = async () => ({});
const sandbox = {
  UI: {}, route: () => {}, esc, enc: encodeURIComponent, $: () => null, $$: () => [], paint: () => true, when: t => (t ? "now" : ""), firstLine: s => String(s || "").split("\n")[0],
  icon: k => `<svg data-icon="${k}"></svg>`, toast: () => {}, ask: async () => true, kb: n => `${n} B`, Screen: { last: {} }, Magnus: {},
  Compose: { markdown: s => `<p>${esc(s)}</p>`, maskedText: s => esc(s) },
  api: (m, p, b) => { calls.push([m, p, b]); return reply(m, p, b); },
  document: { addEventListener: () => {} }, location: { hash: "#/team/w", origin: "https://server", pathname: "/" }, setTimeout, clearTimeout, console,
};
sandbox.globalThis = sandbox;
const files = ["teams.js", "team-chat.js", "team-panel.js", "team-command.js"].map(f => readFileSync(join(here, "../ui", f), "utf8"));
vm.runInNewContext(`${files.join("\n")}\nglobalThis.__x = { Team, TeamChat, TeamPanel, TeamCommand, TeamDrafts };`, sandbox);
const { Team, TeamChat } = sandbox.__x;

// Places and links.
assert.deepEqual(plain({ ...sandbox.teamPlace("/c/ch1/t/m9") }), { kind: "channel", id: "ch1", thread: "m9" });
assert.deepEqual(plain({ ...sandbox.teamPlace("/p/p1/board") }), { kind: "project", id: "p1", tab: "board" });
assert.equal(sandbox.teamPlace("/channels/ch1").id, "ch1", "an old link no longer lands");
assert.equal(sandbox.teamHref("w", { kind: "channel", id: "ch1", thread: "m9" }), "#/team/w/c/ch1/t/m9");
assert.equal(sandbox.teamHref("w", { kind: "project", id: "p1" }), "#/team/w/p/p1");

// The rail: projects with progress, channels with counts, agents, people.
const home = {
  id: "w", name: "Alpha <Vector>", role: "member", me: "sam", me_name: "Sam", agent: { id: "agent", name: "Magnus" }, computer: null,
  members: [{ id: "sam", name: "Sam", role: "member", online: true }, { id: "dana", name: "Dana", role: "owner", online: true }, { id: "leo", name: "Leo", role: "member", online: false }],
  channels: [
    { id: "c1", name: "magnus-alpha", project: "p1", mode: "when_helpful", notify: "all", unread: 4, mentions: 2, direct: null },
    { id: "c2", name: "design", project: null, mode: "mentions", notify: "off", unread: 7, mentions: 0, direct: null },
    { id: "c3", name: "general", project: null, mode: "mentions", notify: "all", unread: 0, mentions: 0, direct: null },
    { id: "d1", name: "", project: null, mode: "mentions", notify: "all", unread: 1, mentions: 0, direct: { people: ["dana", "sam"], agent: false } },
  ],
  projects: [{ id: "p1", name: "Magnus alpha", owner: "dana", members: ["dana", "sam"], progress: { done: 1, total: 3 }, channels: ["c1"] }],
};
Team.home = home; Team.ws = "w";
const rail = sandbox.railHtml(home, { kind: "channel", id: "c1" });
assert.match(rail, /Alpha &#60;Vector&#62;/, "the workspace's name was not escaped");
assert.match(rail, /Magnus alpha<span class="small faint">1\/3<\/span>/, "a project's progress is not at a glance");
assert.match(rail, /#magnus-alpha<span class="team-count loud" aria-label="2 mentions">2<\/span>/, "mentions are not counted in the rail");
assert.match(rail, /data-channel="c1"[^>]*aria-current="page"|aria-current="page" data-channel="c1"/, "the open channel is not marked");
assert.doesNotMatch(rail.match(/data-channel="c2"[^]*?<\/a>/)[0], /team-count/, "a channel with notifications off still shows a count");
const later = { ...home, channels: home.channels.map(c => ({ ...c, unread: c.unread + 1, mentions: c.id === "c3" ? 1 : c.mentions })) };
const notices = sandbox.teamNotices(home, later).map(n => n.title);
assert.ok(notices.includes("You were mentioned in #general"), "a mention is not notified");
assert.ok(notices.includes("New in #magnus-alpha"), "a channel set to all is not notified");
assert.ok(!notices.some(t => t.includes("#design")), "a channel set to off notified");
assert.equal(sandbox.teamNotices(null, later).length, 0, "the first look notified of everything");
assert.match(rail, /data-direct-agent[^]*Magnus <span class="small faint">team<\/span>/, "the team agent is not listed among the agents");
assert.match(rail, /Your Magnus <span class="small faint">private<\/span>/);
assert.match(rail, /team-presence on[^]*Dana/, "presence is not shown for a direct message");
assert.match(rail, /data-dm="leo"/, "someone without a conversation yet cannot be messaged");
assert.doesNotMatch(rail, /data-dm="dana"/, "someone already in a direct message is listed twice");
assert.equal(sandbox.firstChannel(home), "c1", "a workspace does not open where the mentions are");

// The agent's live work, with the requester named and its own thread.
const name = id => ({ dana: "Dana", leo: "Leo", sam: "Sam" })[id] || id;
const work = { task: "t1", requester: "dana", status: "working", steps: 1, doing: "team_read_file", title: "Pull together the alpha feedback", long: true, thread: "m1", reason: null };
const card = sandbox.workCard(work, name, "w", "c1");
assert.match(card, /for Dana/, "the work card does not name the requester");
assert.match(card, /Reading a team file · step 2/);
assert.match(card, /data-open-thread="m1"/, "long work has no thread to open");
assert.match(sandbox.workCard({ ...work, status: "waiting" }, name, "w", "c1"), /Waiting for Dana to answer an ask/);
assert.match(sandbox.workCard({ ...work, status: "failed", reason: "<no access>" }, name, "w", "c1"), /Could not finish: &#60;no access&#62;/);
assert.doesNotMatch(sandbox.workCard({ ...work, thread: null }, name, "w", "c1"), /data-open-thread/);

// Messages: the agent's marked as such and for whom, people's escaped,
// mentions marked, reactions, replies, edits and pending states.
const ctx = { me: "sam", ws: "w", channel: "c1", name, guest: false, files: {}, inThread: false, editing: null };
const asked = sandbox.messageHtml({ id: "m1", sender: "dana", text: "@Magnus pull together <b>feedback</b>", thread: null, at: "2026-11-09T10:02:00Z", reactions: { "👍": ["sam", "leo"] }, replies: 3, last_reply: "2026-11-09T10:14:00Z", work }, ctx);
assert.match(asked, /<span class="mention">@Magnus<\/span>/, "a mention is not marked");
assert.doesNotMatch(asked, /<b>feedback/, "a person's words were not escaped");
assert.match(asked, /class="tm-reaction on"[^>]*>👍 2/, "my reaction is not shown as mine");
assert.match(asked, /3 replies/);
assert.match(asked, /tm-work/, "the agent's work does not show on the asking message");
assert.doesNotMatch(asked, /data-edit>/, "someone else's message can be edited");
const answer = sandbox.messageHtml({ id: "agent:t1", sender: "agent", requester: "dana", text: "Done. **Three** themes.", thread: "m1", at: "2026-11-09T10:31:00Z" }, ctx);
assert.match(answer, /team agent<\/span> <span class="small faint">for Dana/, "the agent's answer does not say who asked");
assert.match(answer, /<p>Done/, "the agent's answer is not rich content");
const mine = sandbox.messageHtml({ id: "m2", sender: "sam", text: "fixed", thread: null, at: "", edited: true }, ctx);
assert.match(mine, /data-edit>/, "my own message cannot be edited");
assert.match(mine, /\(edited\)/);
assert.match(sandbox.messageHtml({ id: "tmp", sender: "sam", text: "hi", thread: null, at: "", state: "sending" }, ctx), /Sending…/);
assert.match(sandbox.messageHtml({ id: "tmp", sender: "sam", text: "hi", thread: null, at: "", state: "failed" }, ctx), /data-retry="tmp"/, "a failed send cannot be retried");
assert.doesNotMatch(sandbox.messageHtml({ id: "m3", sender: "leo", text: "hi", thread: null, at: "" }, { ...ctx, guest: true }), /data-pin|data-make-task/, "a guest is offered pins and tasks");
assert.match(sandbox.messageHtml({ id: "m4", sender: "leo", text: "see this\n📎 plan.pdf", thread: null, at: "" }, ctx), /tm-file[^]*plan\.pdf[^]*data-team-download="plan.pdf"/, "a dropped file does not show");
assert.match(sandbox.messageHtml({ id: "m5", sender: "leo", text: "\n📎 shot.png", thread: null, at: "" }, { ...ctx, files: { "shot.png": "data:image/png;base64,AA" } }), /<img src="data:image\/png;base64,AA"/, "a dropped image shows no preview");

// Optimistic changes: shown at once, merged when the server has them.
const pending = [{ id: "tmp-1", sender: "sam", text: "a", thread: null, at: "", real: "m7" }, { id: "tmp-2", sender: "sam", text: "b", thread: null, at: "", state: "sending" }];
assert.deepEqual(plain(sandbox.mergePending([{ id: "m7" }], pending).map(m => m.id)), ["m7", "tmp-2"], "a confirmed message shows twice");
const view = { messages: [{ id: "m1", text: "x", reactions: {} }] };
sandbox.applyLocal(view, { op: "react", id: "m1", emoji: "🎉", me: "sam", on: true });
assert.deepEqual(plain([...view.messages[0].reactions["🎉"]]), ["sam"]);
sandbox.applyLocal(view, { op: "react", id: "m1", emoji: "🎉", me: "sam", on: false });
assert.equal(view.messages[0].reactions["🎉"], undefined, "an empty reaction stays");
sandbox.applyLocal(view, { op: "edit", id: "m1", text: "y" });
sandbox.applyLocal(view, { op: "pin", id: "m1", pinned: true });
assert.equal(view.messages[0].text + view.messages[0].edited + view.messages[0].pinned, "ytruetrue");

let release;
reply = (m, p) => p.endsWith("/messages") ? new Promise(r => { release = r; }) : Promise.resolve({});
TeamChat.s = { ws: "w", channel: "c1", pending: [], files: {}, view: { messages: [] } };
TeamChat.draw = () => {}; TeamChat.refresh = async () => {};
const sent = TeamChat.send("@Magnus what is left?", null, false, []);
assert.equal(TeamChat.s.pending.length, 1, "a message does not show before the server answers");
assert.equal(TeamChat.s.pending[0].state, "sending");
release({ id: "m8", agent: "mentioned" });
await sent;
assert.equal(TeamChat.s.pending[0].real, "m8");
assert.deepEqual(plain(calls.at(-1)), ["POST", "/v1/workspaces/w/channels/c1/messages", { text: "@Magnus what is left?" }], "asking the agent needed more than a mention");
reply = async () => { throw new Error("offline"); };
await TeamChat.send("later", null, false, []);
assert.equal(TeamChat.s.pending.at(-1).state, "failed", "a failed send is not marked");

// Mentions: the agent first, then people.
assert.equal(sandbox.mentionAt("hey @Ma", 7), "Ma");
assert.equal(sandbox.mentionAt("mail me@x", 9), null, "an address read as a mention");
assert.deepEqual(plain(sandbox.mentionChoices(home, "").map(p => p.name)), ["Magnus", "Dana", "Leo"]);
assert.deepEqual(plain(sandbox.mentionChoices(home, "d").map(p => p.name)), ["Dana"]);

// Drafts survive, and storage that throws breaks nothing.
sandbox.localStorage = { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); }, removeItem: () => {} };
assert.equal(sandbox.__x.TeamDrafts.get("w", "c1"), "");
sandbox.__x.TeamDrafts.set("w", "c1", null, "half");

// The board and progress.
const board = sandbox.boardHtml([{ id: "t1", title: "Fix takeover", status: "doing", assignee: "agent", from: { channel: "c1", message: "m1" }, by: "dana" }, { id: "t2", title: "Raise budget", status: "todo", by: "leo" }], "w", id => (id === "agent" ? "Magnus" : id), true);
assert.equal((board.match(/data-col=/g) || []).length, 3, "the board does not have three columns");
assert.match(board, /href="#\/team\/w\/c\/c1\/t\/m1">From the conversation/, "a task from a conversation does not link back");
assert.match(board, /data-new-task="todo"/, "a task cannot be added by typing");
assert.doesNotMatch(sandbox.boardHtml([], "w", name, false), /data-new-task/, "a guest can add tasks");
assert.match(sandbox.progressBar({ done: 1, total: 4 }), /aria-valuenow="25"/);

// The command menu: making things by typing their name, asking the agent,
// settings for the channel, and search.
Team.place = { kind: "channel", id: "c1" };
const cmds = sandbox.commandsFor(home, Team.place, "Launch site");
const labels = cmds.map(c => c.label);
assert.ok(labels.includes("New project Launch site"), "a project cannot be made by typing its name");
assert.ok(labels.includes("New channel #launch-site"));
assert.ok(labels.includes("Ask Magnus: Launch site"));
assert.ok(labels.includes("Search everything for “Launch site”"));
assert.equal(cmds.find(c => c.label === "New task: Launch site").run.project, "p1", "a task from the menu is not on this channel's project");
assert.equal(sandbox.channelSlug("#Alpha  testers!"), "alpha-testers");
const bare = sandbox.commandsFor(home, Team.place, "").map(c => c.label);
assert.ok(bare.includes("Magnus here: join when it can help") && bare.includes("Magnus here: reply to mentions only"), "the channel's agent setting is not in the menu");
assert.ok(bare.includes("Notifications here: mentions only"));
assert.ok(bare.includes("Activity: mentions, replies and asks"));
const guest = sandbox.commandsFor({ ...home, role: "guest" }, Team.place, "Launch").map(c => c.group);
assert.ok(!guest.includes("Create") && !guest.includes("Ask Magnus"), "a guest is offered to create or to have the agent act");
assert.ok(!sandbox.commandsFor(home, Team.place, "ann@example.com").some(c => c.run.kind === "invite"), "a member who is not an admin is offered to invite");
assert.ok(sandbox.commandsFor({ ...home, role: "admin" }, Team.place, "ann@example.com").some(c => c.run.kind === "invite"));
const found = sandbox.foundItems({ messages: [{ channel: "c1", channel_name: "magnus-alpha", id: "m1", thread: null, sender: "dana", text: "the deck" }], tasks: [{ id: "t", title: "Deck", status: "todo", project: "p1" }], files: [], knowledge: [] }, home);
assert.deepEqual(plain(found.map(f => f.run.place?.kind)), ["channel", "project"]);
assert.equal(found[1].run.place.tab, "board");

console.log("team ok");
