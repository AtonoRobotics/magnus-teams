// The team workspace as teams.js, team-chat.js, team-conversation.js,
// team-panel.js and team-command.js draw it: the rail, messages with the
// agent's live work (drawn by the one chat view, chat-view.js), mentions, the
// board and the command menu.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const calls = [], listeners = {};
const plain = x => JSON.parse(JSON.stringify(x));
let reply = async () => ({});
const sandbox = {
  UI: {}, route: () => {}, esc, voice: { entry: null }, ICONS: new Proxy({}, { get: () => "" }), App: { canReadAloud: true }, enc: encodeURIComponent, $: () => null, $$: () => [], paint: () => true, when: t => (t ? "now" : ""), firstLine: s => String(s || "").split("\n")[0],
  chosenModel: form => form?.picked, modelPicker: async () => "",
  icon: k => `<svg data-icon="${k}"></svg>`, chip: st => `<span class="chip">${st}</span>`, statusLineSlot: (t, cls) => `<span class="${cls}" data-status-line="${t.task_id}">${t.status_line}</span>`, toast: () => {}, ask: async () => true, kb: n => `${n} B`, Magnus: {},
  Compose: { markdown: s => `<p>${esc(s)}</p>`, maskedText: s => esc(s) },
  api: (m, p, b) => { calls.push([m, p, b]); return reply(m, p, b); },
  document: { addEventListener: (type, f) => (listeners[type] ||= []).push(f) }, location: { hash: "#/team/w", origin: "https://server", pathname: "/" }, setTimeout, clearTimeout, console,
};
sandbox.globalThis = sandbox;
const files = ["server.js", "message-controls.js", "chat-view.js", "chat-composer.js", "side-panel.js", "teams.js", "team-chat.js", "team-conversation.js", "coedit.js", "team-panel.js", "team-command.js", "team-people.js"].map(f => readFileSync(join(here, "../ui", f), "utf8"));
vm.runInNewContext(`${files.join("\n")}\nglobalThis.Magnus = Object.assign(globalThis.Magnus || {}, { api: (m, p, b) => api(m, p, b) });\nglobalThis.__x = { Team, TeamChat, TeamPanel, TeamCommand, ChatView, Conversation, MessageDrafts };`, sandbox);

// An item as the server sends it (crates/magnus/src/chat_entry.rs): the personal
// chat's entry, its files as file entries, work no message carries as a `tasks` entry.
const served = m => { const files = m.files || String(m.text || "").split("\n").filter(l => l.startsWith("📎 ")).map(l => l.slice(2).trim()).map(name => ({ id: name, name })); const kind = m.kind || (m.deleted ? "deleted" : m.sender !== "agent" ? "message" : m.text ? "answer" : "tasks"); return kind === "tasks" && m.work ? { ...m, kind, files, work: undefined, tasks: [m.work], from_run: m.work.task_id } : { ...m, kind, files, text: String(m.text || "").split("\n").filter(l => !l.startsWith("📎 ")).join("\n"), task_id: m.task_id ?? m.task, for: m.for ?? (kind === "answer" && m.requester ? m.requester[0].toUpperCase() + m.requester.slice(1) : undefined) }; };
const { Team, TeamChat } = sandbox.__x;

// Places and links.
assert.deepEqual(plain({ ...sandbox.teamPlace("/c/ch1/t/m9") }), { kind: "channel", id: "ch1", thread: "m9" });
assert.deepEqual(plain({ ...sandbox.teamPlace("/p/p1/board") }), { kind: "project", id: "p1", tab: "board" });
assert.equal(sandbox.teamPlace("/channels/ch1").id, "ch1", "an old link no longer lands");
assert.equal(sandbox.teamHref("w", { kind: "channel", id: "ch1", thread: "m9" }), "#/team/w/c/ch1/t/m9");
assert.equal(sandbox.teamHref("w", { kind: "project", id: "p1" }), "#/team/w/p/p1");

// Any regular signed-in user can start a shared project; only guests cannot.
assert.match(sandbox.teamsPageHtml([], true), /<form id="new-team"><input id="team-name"/, "a user cannot type a name to start a shared project");
assert.equal(await sandbox.mayStartWorkspace(), true, "a regular user cannot start a shared project");
const guestPage = sandbox.teamsPageHtml([], false);
assert.ok(!/new-team|team-name/.test(guestPage), "a guest is offered a shared project form the server refuses");

// Shared conversations: clean titles (no Slack # prefix), minimal header tools
// (faces, Auto-join toggle, Members chip, no Join Call), and firstChannel selection.
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
assert.equal(sandbox.channelTitle(home, home.channels[0]), "magnus-alpha", "shared conversation title drops # prefix");
assert.equal(sandbox.channelTitle(home, home.channels[3]), "Dana", "direct conversation title shows the other person");
const tools = sandbox.channelTools(home, home.channels[0], home.projects[0]);
assert.match(tools, /data-agent-toggle[^>]*aria-pressed="true"[^>]*>Auto-join: On/, "Auto-join toggle reflects when_helpful mode");
assert.match(tools, /data-open="shared:settings"/, "Members chip opens shared settings in the unified Canvas");
assert.doesNotMatch(tools, /Join Call|data-room-call|data-pins|data-archive/, "Slack/call chrome is cut from shared conversation header");
assert.equal(sandbox.firstChannel(home), "c1", "a shared project opens where the mentions are");

// A channel's items, drawn by the one chat view as the personal chat draws
// its own: the state a channel holds, and one item drawn there.
const name = id => ({ dana: "Dana", leo: "Leo", sam: "Sam", agent: "Magnus" })[id] || id;
const channelState = { ws: "w", channel: "c1", view: { messages: [], names: {} }, files: {}, pinsOnly: false, whispers: [], hidden: new Set(), unreadAtOpen: 0, typedAt: 0 };
const draw = (m, o = {}) => {
  const st = { ...channelState, ...(o.s || {}) }, thread = o.thread || null;
  const conv = sandbox.teamConversation(st, thread), r = sandbox.teamReader(st, thread);
  return sandbox.__x.ChatView.item(sandbox.teamEntry(served(m), r), { conv, ui: { editing: null, sharing: null, ...(o.ui || {}) } });
};
Team.home = home;

// The agent's live work: the personal chat's task row, with the requester
// named and its own thread to open.
const work = { task_id: "t1", requester: "dana", status: "running", steps: 1, doing: "team_read_file", status_line: "Reading a team file", title: "Pull together the alpha feedback", long: true, thread: "m1", waiting_on: null };
const workItem = w => draw({ id: "w1", sender: "agent", requester: w.requester, text: "", thread: null, at: "", work: w });
const card = workItem(work);
assert.match(card, /class="reqgroup chatcard"[^]*class="tcard"/, "the work card is not the personal chat's task row");
assert.match(card, /for Dana/, "the work card does not name the requester");
assert.match(card, /Reading a team file/, "the work card does not say what the server says it is doing");
// A team reply is a chat turn: no step count, as no Magnus chat shows one (owner, 2026-11-13).
assert.doesNotMatch(card, /step \d/, "the work card counts steps");
assert.match(card, /href="#\/team\/w\/c\/c1\/t\/m1"/, "long work has no thread to open");
assert.match(workItem({ ...work, status: "suspended" }), /Waiting for Dana to answer an ask/);
assert.match(workItem({ ...work, status: "failed", waiting_on: "<no access>" }), /Could not finish: &#60;no access&#62;/);
assert.doesNotMatch(workItem({ ...work, thread: null }), /\/t\/m1/);
assert.match(workItem({ ...work, requester: "sam", status: "suspended" }), /Needs you/, "work waiting on me does not say it needs me");

// Messages: whose each is, people's words escaped, mentions marked,
// reactions, replies and edits; the agent's answer for whom.
const asked = draw({ id: "m1", sender: "dana", text: "@Magnus pull together <b>feedback</b>", thread: null, at: "2026-11-09T10:02:00Z", reactions: { "👍": ["sam", "leo"] }, replies: 3, last_reply: "2026-11-09T10:14:00Z", work });
assert.match(asked, /class="entry them"[^]*class="msg person"[^]*Dana/, "someone else's message is not drawn as theirs");
assert.match(asked, /<span class="mention">@Magnus<\/span>/, "a mention is not marked");
assert.doesNotMatch(asked, /<b>feedback/, "a person's words were not escaped");
assert.match(asked, /class="reaction on"[^>]*>👍 2/, "my reaction is not shown as mine");
assert.match(asked, /3 replies/);
assert.match(asked, /data-chat-work="t1"/, "the agent's work does not show on the asking message");
assert.doesNotMatch(asked, /data-msg-act="edit"/, "someone else's message can be edited");
const answer = draw({ id: "agent:t1", sender: "agent", requester: "dana", text: "Done. **Three** themes.", thread: "m1", at: "2026-11-09T10:31:00Z" });
assert.match(answer, /class="msg magnus"[^]*Magnus<\/span><span class="by-meta"> · for Dana/, "the agent's answer does not say who asked");
assert.match(answer, /<p>Done/, "the agent's answer is not rich content");
const mine = draw({ id: "m2", sender: "sam", text: "fixed", thread: null, at: "", edited: true, may: { edit: true, share: true, delete: true } });
assert.match(mine, /class="entry me"[^]*class="msg me"[^]*You/, "my own message is not drawn as the personal chat draws mine");
assert.match(mine, /data-msg-act="edit"/, "my own message cannot be edited");
assert.match(mine, / · edited/);
assert.match(draw({ id: "m2", sender: "sam", text: "fixed", thread: null, at: "", may: { edit: true } }, { ui: { editing: "m2" } }), /msg me editing[^]*data-msg-act="edit-send"/, "a team message is not edited as a personal one is");
assert.doesNotMatch(draw({ id: "m3", sender: "leo", text: "hi", thread: null, at: "", may: { share: true } }), /data-msg-act="(pin|task|save)"/, "a guest is offered pins and tasks the server refuses");
assert.match(draw({ id: "m4", sender: "leo", text: "see this\n📎 plan.pdf", thread: null, at: "" }), /class="previews"[^]*class="fprev"[^]*plan\.pdf[^]*data-file-download="plan.pdf"/, "a dropped file does not show as a personal chat's file does");
assert.match(draw({ id: "m5", sender: "leo", text: "\n📎 shot.png", thread: null, at: "" }, { s: { files: { [sandbox.teamPreviewKey("w", "p1", "shot.png")]: "data:image/png;base64,AA" } } }), /<img alt="" src="data:image\/png;base64,AA"/, "a dropped image shows no preview");
assert.doesNotMatch(draw({ id: "m5", sender: "leo", text: "\n📎 shot.png", thread: null, at: "" }, { s: { files: { [sandbox.teamPreviewKey("w2", "p1", "shot.png")]: "data:image/png;base64,AA" } } }), /<img /, "another shared project's image of the same name shows here");

// Sending: what the composer sends to a channel. The composer is the
// personal chat's: the words stay in the box until the server has them.
reply = async () => ({ id: "m8", agent: "mentioned" });
TeamChat.refresh = async () => {};
let then = await sandbox.teamComposer({ ...channelState }, null).send({ text: "@Magnus what is left?", files: [], form: null });
assert.deepEqual(plain(calls.at(-1)), ["POST", "/v1/workspaces/w/channels/c1/messages", { text: "@Magnus what is left?" }], "asking the agent needed more than a mention");
await then();
sandbox.Magnus.putBytes = async path => { calls.push(["PUT", path]); };
sandbox.__x.MessageDrafts.originals.set("u-plan", { name: "plan.pdf", type: "application/pdf", size: 3, arrayBuffer: async () => new ArrayBuffer(3) });
await sandbox.teamComposer({ ...channelState }, "m1").send({ text: "see", files: [{ uid: "u-plan", name: "plan.pdf" }], form: null });
assert.deepEqual(plain(calls.at(-2)), ["PUT", "/v1/workspaces/w/files/plan.pdf?project=p1"], "a file sent to a channel did not reach the shared Files");
assert.deepEqual(plain(calls.at(-1)), ["POST", "/v1/workspaces/w/channels/c1/messages", { text: "see\n📎 plan.pdf", thread: "m1" }], "a reply in a thread lost its thread or its file");
// The composer's model picker is the personal one: a request runs on its pick.
const picked = { adapter: "a", model: "fast" };
await sandbox.teamComposer({ ...channelState }, null).send({ text: "@Magnus plan it", files: [], form: { picked } });
assert.deepEqual(plain(calls.at(-1)[2].model), picked, "a team request does not carry the model the composer picked");
assert.match(TeamChat.mount.toString(), /Conversation\.open\(/, "a shared conversation does not open as the one conversation");
assert.match(sandbox.__x.Conversation.open.toString(), /modelPicker\(\)/, "the team composer has no model picker");
reply = async () => ({});

// Mentions: the agent first, then people.
assert.equal(sandbox.mentionAt("hey @Ma", 7), "Ma");
assert.equal(sandbox.mentionAt("mail me@x", 9), null, "an address read as a mention");
assert.deepEqual(plain(sandbox.mentionChoices(home, "").map(p => p.name)), ["Magnus", "Dana", "Leo"]);
assert.deepEqual(plain(sandbox.mentionChoices(home, "d").map(p => p.name)), ["Dana"]);

// A shared conversation's draft is the personal chat's (shared-draft-check.mjs).
assert.equal(typeof sandbox.TeamDrafts, "undefined", "a shared conversation keeps a second draft on the device");

// The board and progress.
const board = sandbox.boardHtml([{ id: "t1", title: "Fix takeover", status: "doing", assignee: "agent", from: { channel: "c1", message: "m1" }, by: "dana" }, { id: "t2", title: "Raise budget", status: "todo", by: "leo" }], "w", id => (id === "agent" ? "Magnus" : id), true);
assert.equal((board.match(/data-col=/g) || []).length, 3, "the board does not have three columns");
assert.match(board, /href="#\/team\/w\/c\/c1\/t\/m1">From the conversation/, "a task from a conversation does not link back");
assert.match(board, /data-new-task="todo"/, "a task cannot be added by typing");
assert.doesNotMatch(sandbox.boardHtml([], "w", name, false), /data-new-task/, "a guest can add tasks");
assert.match(sandbox.progressBar({ done: 1, total: 4 }), /aria-valuenow="25"/);
const overview = sandbox.overviewHtml({ project: null, channels: ["c1"], files: [{ name: "notes.md", size: 1, by: "leo", by_name: "Leo", project: null }], tasks: [], work: [{ task: "t1", channel: "c1", requester: "dana", requester_name: "Dana", waiting: false, steps: 1 }], members: [{ id: "dana", name: "Dana" }], knowledge: 0, progress: null, computer: "pc1" }, "w", "c1");
assert.match(overview, /Magnus is working for Dana/, "the panel does not say whom the agent works for");
assert.doesNotMatch(overview, /data-mission-new/, "retired mission form must not appear in overview");
assert.match(sandbox.teamFilesHtml({ project: null, channels: ["c1"], files: [{ name: "notes.md", size: 1, by: "leo", by_name: "Leo", project: null }], tasks: [], work: [{ task: "t1", channel: "c1", requester: "dana", requester_name: "Dana", waiting: false, steps: 1 }], members: [{ id: "dana", name: "Dana" }], knowledge: 0, progress: null, computer: "pc1" }, "w"), /notes\.md[^]*lib-size/, "the Files tab does not list the files with their size");
// The redesigned side panel has two intuitive tabs: Work (activity) and People (members).
const side = sandbox.teamSideHtml({ project: null, channels: ["c1"], files: [{ name: "notes.md", size: 1, by: "leo", by_name: "Leo", project: null }], tasks: [], work: [{ task: "t1", channel: "c1", requester: "dana", requester_name: "Dana", waiting: false, steps: 1 }], members: [{ id: "dana", name: "Dana" }], knowledge: 0, progress: null, computer: "pc1" }, "w", "c1", "activity");
assert.match(side, /class="companion-id"[^]*companion-status-lbl run">Working · 1 active</, "the side panel's head does not say how the agent stands");
assert.deepEqual([...side.matchAll(/data-team-side-tab="(\w+)"/g)].map(m => m[1]), ["activity", "members"], "the side panel has Work and People tabs");
assert.match(side, /data-team-side-tab="activity" aria-current="page"/, "the open tab is not marked");
assert.match(side, /Active now[^]*Magnus is working for Dana/, "the agent's current work is not the Active now card");
assert.doesNotMatch(side, /notes\.md|data-members-card/, "the Work tab carries the other tabs' content");
assert.match(sandbox.teamSideHtml({ project: null, channels: ["c1"], files: [{ name: "notes.md", size: 1, by: "leo", by_name: "Leo", project: null }], tasks: [], work: [{ task: "t1", channel: "c1", requester: "dana", requester_name: "Dana", waiting: false, steps: 1 }], members: [{ id: "dana", name: "Dana" }], knowledge: 0, progress: null, computer: "pc1" }, "w", "c1", "members"), /data-members-card/, "the People tab does not show the members");
assert.match(sandbox.teamSideHtml({ ...{ project: null, channels: ["c1"], files: [{ name: "notes.md", size: 1, by: "leo", by_name: "Leo", project: null }], tasks: [], work: [{ task: "t1", channel: "c1", requester: "dana", requester_name: "Dana", waiting: false, steps: 1 }], members: [{ id: "dana", name: "Dana" }], knowledge: 0, progress: null, computer: "pc1" }, work: [{ task: "t1", channel: "c1", requester: "dana", requester_name: "Dana", waiting: true, steps: 1 }] }, "w", "c1", "activity"), /Waiting on someone \(1\)[^]*Magnus is waiting for Dana/, "work waiting on a person is not at the top");
const noComputer = { project: null, channels: [], files: [], tasks: [], work: [], members: [], knowledge: 0, progress: null, computer: null };
for (const role of ["member", "admin", "owner"]) {
  Team.home = { ...home, role };
  const html = sandbox.overviewHtml(noComputer, "w");
  assert.doesNotMatch(html, /data-team-computer-new|a computer<\/button>/, `the ${role} is offered to set up a shared team computer`);
}
// Routines in the Team side panel: any collaborator can add routines and manage
// their own; guests see them read-only; project creator can manage any routine.
const withRoutines = {
  ...noComputer,
  channels: ["c1"],
  routines: [
    { id: "r1", channel: "c1", text: "Sum up the day", every_hours: 24, by: "dana", by_name: "Dana", last_at: null, stopped: null },
    { id: "r2", channel: "c1", text: "Check alerts", every_hours: 1, by: "sam", by_name: "Sam", last_at: null, stopped: "its last 3 runs failed" },
  ],
};
Team.home = { ...home, role: "member", me: "sam" };
const memberRoutines = sandbox.overviewHtml(withRoutines, "w", "c1");
assert.match(memberRoutines, /data-routines[^]*Sum up the day[^]*Daily/, "a peer does not see active routines");
assert.match(memberRoutines, /Stopped: its last 3 runs failed/, "a peer does not see why a routine stopped");
assert.match(memberRoutines, /data-routine-new/, "a peer cannot add a shared routine");
assert.match(memberRoutines, /data-routine-restart="r2"/, "a peer cannot restart their own stopped routine");
assert.doesNotMatch(memberRoutines, /data-routine-remove="r1"/, "a peer can remove someone else's routine");
Team.home = { ...home, role: "guest", me: "leo" };
const guestRoutines = sandbox.overviewHtml(withRoutines, "w", "c1");
assert.doesNotMatch(guestRoutines, /data-routine-new|data-routine-restart|data-routine-remove/, "a guest is offered routine management controls");
Team.home = { ...home, role: "owner", me: "dana" };
const ownerRoutines = sandbox.overviewHtml(withRoutines, "w", "c1");
assert.match(ownerRoutines, /data-routine-restart="r2"/, "the project creator cannot restart a stopped routine");
assert.match(ownerRoutines, /data-routine-remove="r1"/, "the project creator cannot remove a routine");
Team.home = home;
assert.equal(sandbox.workTitle("@Magnus pull together the feedback\nmore"), "Pull together the feedback", "a work card's title keeps the mention that asked");
assert.equal(sandbox.workTitle("Magnus, sum it up"), "Magnus, sum it up");
assert.match(card, /<span class="ttl">Pull together the alpha feedback<\/span>/);

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
assert.ok(sandbox.commandsFor(home, Team.place, "ann@example.com").some(c => c.run.kind === "invite"), "a peer collaborator cannot invite by email from the command menu");
assert.ok(!sandbox.commandsFor({ ...home, role: "guest" }, Team.place, "ann@example.com").some(c => c.run.kind === "invite"), "a guest is offered to invite");
// Inviting by email says what happened: emailed, never "send them this
// link"; not emailed, says so and gives the link to send.
const emailed = sandbox.inviteOutcome("ann@example.com", { invited: "ann", emailed: true, mail: true, link: "https://server/invite?token=t", path: "/invite?token=t" });
assert.match(emailed, /Invitation emailed to ann@example\.com/);
assert.doesNotMatch(emailed, /Send them this link/, "an emailed invitation still says to send the link");
const failed = sandbox.inviteOutcome("ann@example.com", { invited: "ann", emailed: false, mail: true, email_error: "connection refused", link: "https://server/invite?token=t" });
assert.match(failed, /could not be sent \(connection refused\)\. Send them this link yourself: https:\/\/server\/invite\?token=t/);
const noMail = sandbox.inviteOutcome("ann@example.com", { invited: "ann", emailed: false, mail: false, link: null, path: "/invite?token=t" });
assert.match(noMail, /does not send email[^]*Send them this link yourself: https:\/\/server\/invite\?token=t/, "with no mail the link to copy is missing");
assert.match(sandbox.inviteOutcome("leo@example.com", { added: "leo" }), /leo@example\.com is in/);
assert.equal(sandbox.serverLink({ code: "c", link: null, path: "/join?code=c" }), "https://server/join?code=c", "a copied link is not one a newcomer can open");
const found = sandbox.foundItems({ messages: [{ channel: "c1", channel_name: "magnus-alpha", id: "m1", thread: null, sender: "dana", text: "the deck" }], tasks: [{ id: "t", title: "Deck", status: "todo", project: "p1" }], files: [], knowledge: [] }, home);
assert.deepEqual(plain(found.map(f => f.run.place?.kind)), ["channel", "project"]);
assert.equal(found[1].run.place.tab, "board");
const groups = sandbox.commandsFor({ ...home, role: "admin" }, Team.place, "ann@example.com").map(c => c.group);
assert.equal(groups.filter((g, i) => g !== groups[i - 1]).length, new Set(groups).size, "a group of the menu is split in two");

// Shared workspace spend limits are retired — each collaborator runs on their own personal budget.
const owner = { ...home, role: "owner", spend: { spent_usd: 12.5, limit_usd: 50, channels: { c1: { spent_usd: 4, limit_usd: 10 } } } };
assert.equal(sandbox.limitCommands(owner, null, "$25").length, 0, "retired spend limit commands still appear");
assert.ok(!sandbox.commandsFor(owner, Team.place, "$25").some(c => c.run.kind === "limit"), "retired spend limit command appeared in command menu");
Team.home = owner;
const overviewNoSpend = sandbox.overviewHtml({ project: null, channels: [], files: [], tasks: [], work: [], members: [], knowledge: 0, progress: null, computer: null }, "w", "c1");
assert.doesNotMatch(overviewNoSpend, /data-spend|Spend this month/, "retired Shared Spend card appeared in panel");
Team.home = home;
assert.equal(sandbox.teamUsd(0.004), "under $0.01", "a few calls' spend reads as nothing");
assert.equal(sandbox.teamUsd(12), "$12");
assert.match(workItem({ ...work, doing: null, status_line: null, steps: 1 }), /Thinking it through/, "work with no action yet says it is starting");

assert.doesNotMatch(workItem({ ...work, doing: null, status_line: null, steps: 0 }), /step|Starting/, "a reply not begun yet counts steps");
assert.doesNotMatch(draw({ id: "w1", sender: "agent", requester: "dana", text: "", thread: "m1", at: "", work }, { thread: "m1" }), /\/t\/m1/, "a work card inside its own thread links to that thread");
// The words are the server's; the app keeps no copy of them.
assert.match(workItem({ ...work, doing: "task_complete", status_line: "Finishing up" }), /Finishing up/);
assert.match(workItem({ ...work, status_line: null }), /Working/, "work the server gave no words for says more than it was told");
assert.equal(typeof sandbox.doingWords, "undefined", "the app keeps its own copy of the server's words");

// The channel's header shows who is in it, with the agent always there.
const faces = sandbox.channelFaces(home, home.channels[0], home.projects[0]);
assert.match(faces, /In this channel: Dana, Sam, Magnus/, "a project channel's header does not show its people and the agent");
assert.match(faces, /team-face on/, "who is here now is not shown in the header");
assert.match(sandbox.channelFaces(home, home.channels[2], null), /Sam, Dana, Leo, Magnus/, "a workspace channel does not show the workspace's people");

// A knowledge entry shows what the spec gives each: type, statement,
// source, owner and last review, its relations and flags.
const older = { id: "k0", statement: "Ship on Fridays" };
const entry = { id: "k1", kind: "decision", statement: "Ship on <Tuesdays>", source: "web", weight: "outside", author: "agent", owner: "dana", thread: "m1", reviewed_at: "2026-11-09T10:00:00Z", relations: [{ kind: "supersedes", to: "k0" }], flags: [{ by: "leo", why: "we moved it" }] };
const kbHtml = sandbox.kbEntryHtml(entry, [older, entry], name, false);
assert.match(kbHtml, /<span class="chip">decision<\/span><span class="grow">Ship on &#60;Tuesdays&#62;/, "an entry's type and statement are not shown, or not escaped");
assert.match(kbHtml, /from outside the workspace/, "an entry from outside is not marked for its weight");
assert.match(kbHtml, /owner Dana · reviewed now · from a thread/, "an entry's owner, review or thread is missing");
assert.match(kbHtml, /supersedes “Ship on Fridays”/, "an entry's relation is not shown");
assert.match(kbHtml, /Flagged by Leo: we moved it/);
assert.doesNotMatch(kbHtml, /data-entry-correct|data-entry-retire/, "someone who is not a curator is offered to correct or retire");
assert.match(sandbox.kbEntryHtml(entry, [entry], name, true), /data-entry-correct[^]*data-entry-retire/, "a curator cannot correct or retire");

// TeamCommand no longer hijacks global Ctrl+K; unified app.js palette owns it.
assert.equal((listeners.keydown || []).length, 0, "TeamCommand does not hijack global Ctrl+K");

// Personal agent delegations appear in Work tab; retired workspace apps card is absent from Members tab.
const panelWithDelegations = {
  ...noComputer,
  computer: null,
  channels: ["c1"],
  members: [{ id: "sam", name: "Sam" }, { id: "dana", name: "Dana" }],
  delegations: [
    { task: "t99", channel: "c1", member: "dana", member_name: "Dana", mine: false, shared: false, status: "running", summary: "Check open PRs" },
  ],
  apps: [
    { name: "github", title: "GitHub", signs_in: true, roles: ["owner", "admin", "member"] },
  ],
};
const actHtml = sandbox.overviewHtml(panelWithDelegations, "w", "c1");
assert.match(actHtml, /data-delegations[^]*Personal agent delegations[^]*Dana's Magnus[^]*Working in personal agent/, "the Activity tab does not show personal agent delegations");
const memHtml = sandbox.teamSideHtml(panelWithDelegations, "w", "c1", "members");
assert.doesNotMatch(memHtml, /data-team-apps-card|Workspace app/, "retired workspace apps card still appears in Members tab");

console.log("team ok");
