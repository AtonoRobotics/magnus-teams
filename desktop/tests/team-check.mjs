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
const files = ["message-controls.js", "chat-view.js", "chat-composer.js", "teams.js", "team-chat.js", "team-conversation.js", "team-panel.js", "team-command.js", "team-people.js"].map(f => readFileSync(join(here, "../ui", f), "utf8"));
vm.runInNewContext(`${files.join("\n")}\nglobalThis.__x = { Team, TeamChat, TeamPanel, TeamCommand, TeamDrafts, ChatView };`, sandbox);

// An item as the server sends it (crates/magnus/src/team/items.rs): its kind and files.
const served = m => ({ ...m, kind: m.kind || (m.deleted ? "deleted" : m.sender !== "agent" ? "message" : m.text ? "answer" : "work"), files: m.files || String(m.text || "").split("\n").filter(l => l.startsWith("📎 ")).map(l => l.slice(2).trim()) });
const { Team, TeamChat } = sandbox.__x;

// Places and links.
assert.deepEqual(plain({ ...sandbox.teamPlace("/c/ch1/t/m9") }), { kind: "channel", id: "ch1", thread: "m9" });
assert.deepEqual(plain({ ...sandbox.teamPlace("/p/p1/board") }), { kind: "project", id: "p1", tab: "board" });
assert.equal(sandbox.teamPlace("/channels/ch1").id, "ch1", "an old link no longer lands");
assert.equal(sandbox.teamHref("w", { kind: "channel", id: "ch1", thread: "m9" }), "#/team/w/c/ch1/t/m9");
assert.equal(sandbox.teamHref("w", { kind: "project", id: "p1" }), "#/team/w/p/p1");

// A workspace is started by typing its name, by those the server lets: a
// Team Admin. Nobody else is offered a form the server would refuse.
assert.match(sandbox.teamsPageHtml([], true), /<form id="new-team"><input id="team-name"/, "a Team Admin cannot type a name to start a workspace");
assert.match(sandbox.mayStartWorkspace.toString(), /standing\?\.team_admin/, "the form follows the server admin role, not Team Admin");
const memberPage = sandbox.teamsPageHtml([], false);
assert.ok(!/new-team|team-name/.test(memberPage), "a member is offered a workspace form the server refuses");
assert.match(memberPage, /A Team Admin starts one and invites you\./);

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
assert.match(rail, /data-direct-agent[^]*Magnus <span class="small faint">team<\/span>/, "the team agent is not listed among the agents");
assert.match(rail, /Your Magnus <span class="small faint">private<\/span>/);
assert.match(rail, /team-presence on[^]*Dana/, "presence is not shown for a direct message");
assert.match(rail, /data-dm="leo"/, "someone without a conversation yet cannot be messaged");
assert.doesNotMatch(rail, /data-dm="dana"/, "someone already in a direct message is listed twice");
assert.equal(sandbox.firstChannel(home), "c1", "a workspace does not open where the mentions are");

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
const work = { task: "t1", requester: "dana", status: "running", steps: 1, doing: "team_read_file", title: "Pull together the alpha feedback", long: true, thread: "m1", reason: null };
const workItem = w => draw({ id: "w1", sender: "agent", requester: w.requester, text: "", thread: null, at: "", work: w });
const card = workItem(work);
assert.match(card, /class="reqgroup chatcard"[^]*class="tcard"/, "the work card is not the personal chat's task row");
assert.match(card, /for Dana/, "the work card does not name the requester");
assert.match(card, /Reading a team file/);
// A team reply is a chat turn: no step count, as no Magnus chat shows one (owner, 2026-11-13).
assert.doesNotMatch(card, /step \d/, "the work card counts steps");
assert.match(card, /href="#\/team\/w\/c\/c1\/t\/m1"/, "long work has no thread to open");
assert.match(workItem({ ...work, status: "suspended" }), /Waiting for Dana to answer an ask/);
assert.match(workItem({ ...work, status: "failed", reason: "<no access>" }), /Could not finish: &#60;no access&#62;/);
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
assert.match(answer, /class="msg magnus"[^]*Magnus · for Dana/, "the agent's answer does not say who asked");
assert.match(answer, /<p>Done/, "the agent's answer is not rich content");
const mine = draw({ id: "m2", sender: "sam", text: "fixed", thread: null, at: "", edited: true });
assert.match(mine, /class="entry me"[^]*class="msg me"[^]*You/, "my own message is not drawn as the personal chat draws mine");
assert.match(mine, /data-msg-act="edit"/, "my own message cannot be edited");
assert.match(mine, / · edited/);
assert.match(draw({ id: "m2", sender: "sam", text: "fixed", thread: null, at: "" }, { ui: { editing: "m2" } }), /msg me editing[^]*data-msg-act="edit-send"/, "a team message is not edited as a personal one is");
Team.home = { ...home, role: "guest" };
assert.doesNotMatch(draw({ id: "m3", sender: "leo", text: "hi", thread: null, at: "" }), /data-msg-act="(pin|task|save)"/, "a guest is offered pins and tasks");
Team.home = home;
assert.match(draw({ id: "m4", sender: "leo", text: "see this\n📎 plan.pdf", thread: null, at: "" }), /class="previews"[^]*class="fprev"[^]*plan\.pdf[^]*data-team-download="plan.pdf"/, "a dropped file does not show as a personal chat's file does");
assert.match(draw({ id: "m5", sender: "leo", text: "\n📎 shot.png", thread: null, at: "" }, { s: { files: { "shot.png": "data:image/png;base64,AA" } } }), /<img alt="" src="data:image\/png;base64,AA"/, "a dropped image shows no preview");

// Sending: what the composer sends to a channel. The composer is the
// personal chat's: the words stay in the box until the server has them.
reply = async () => ({ id: "m8", agent: "mentioned" });
TeamChat.refresh = async () => {};
let then = await sandbox.teamComposer({ ...channelState }, null).send({ text: "@Magnus what is left?", files: [], form: null });
assert.deepEqual(plain(calls.at(-1)), ["POST", "/v1/workspaces/w/channels/c1/messages", { text: "@Magnus what is left?" }], "asking the agent needed more than a mention");
await then();
await sandbox.teamComposer({ ...channelState }, "m1").send({ text: "see", files: [{ name: "plan.pdf" }], form: null });
assert.deepEqual(plain(calls.at(-1)), ["POST", "/v1/workspaces/w/channels/c1/messages", { text: "see\n📎 plan.pdf", thread: "m1" }], "a reply in a thread lost its thread or its file");
// The composer's model picker is the personal one: a request runs on its pick.
const picked = { adapter: "a", model: "fast" };
await sandbox.teamComposer({ ...channelState }, null).send({ text: "@Magnus plan it", files: [], form: { picked } });
assert.deepEqual(plain(calls.at(-1)[2].model), picked, "a team request does not carry the model the composer picked");
assert.match(TeamChat.mount.toString(), /modelPicker\(\)/, "the team composer has no model picker");
reply = async () => ({});

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
const overview = sandbox.overviewHtml({ project: null, channels: ["c1"], files: [{ name: "notes.md", size: 1, by: "leo", by_name: "Leo", project: null }], tasks: [], work: [{ task: "t1", channel: "c1", requester: "dana", requester_name: "Dana", waiting: false, steps: 1 }], members: [{ id: "dana", name: "Dana" }], knowledge: 0, progress: null, computer: "pc1" }, "w");
assert.match(overview, /data-team-screen="pc1"/, "the panel does not show the team computer's screen");
assert.match(overview, /Magnus is working for Dana/, "the panel does not say whom the agent works for");
assert.match(overview, /data-take-over="pc1"/, "the team computer cannot be taken over from the panel");
assert.match(overview, /notes\.md/);
const noComputer = { project: null, channels: [], files: [], tasks: [], work: [], members: [], knowledge: 0, progress: null, computer: null };
// One computer, always there (spec rev 142): nobody sets the team computer
// up, an admin no more than a member; before the server has made it, the
// panel says it is coming.
for (const role of ["member", "admin", "owner"]) {
  Team.home = { ...home, role };
  const html = sandbox.overviewHtml(noComputer, "w");
  assert.doesNotMatch(html, /data-team-computer-new|a computer<\/button>/, `the ${role} is offered to set up the team computer`);
  assert.match(html, /being set up/, `the ${role} is not told the computer is coming`);
}
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
assert.ok(!sandbox.commandsFor(home, Team.place, "ann@example.com").some(c => c.run.kind === "invite"), "a member who is not an admin is offered to invite");
assert.ok(sandbox.commandsFor({ ...home, role: "admin" }, Team.place, "ann@example.com").some(c => c.run.kind === "invite"));
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

// Spend limits: an owner types an amount; nobody else is offered it.
const owner = { ...home, role: "owner", spend: { spent_usd: 12.5, limit_usd: 50, channels: { c1: { spent_usd: 4, limit_usd: 10 } } } };
const fifty = sandbox.commandsFor(owner, Team.place, "$25");
assert.equal(fifty[0].label, "Limit #magnus-alpha to $25 a month", "an amount typed by an owner does not come first as a limit");
assert.deepEqual(plain(fifty[0].run), { kind: "limit", channel: "c1", usd: 25 });
assert.deepEqual(plain(fifty[1].run), { kind: "limit", usd: 25 });
assert.ok(!sandbox.commandsFor({ ...owner, role: "admin" }, Team.place, "25").some(c => c.run.kind === "limit"), "an admin who is not an owner is offered a spend limit");
const lifts = sandbox.commandsFor(owner, Team.place, "").filter(c => c.run.kind === "limit").map(c => c.label);
assert.deepEqual(plain(lifts), ["Lift the spend limit on #magnus-alpha", "Lift the workspace's spend limit"]);
assert.equal(sandbox.limitCommands(owner, null, "lots").length, 0, "words were read as an amount");
Team.home = owner;
const spendCard = sandbox.overviewHtml({ project: null, channels: [], files: [], tasks: [], work: [], members: [], knowledge: 0, progress: null, computer: null }, "w", "c1");
assert.match(spendCard, /Workspace<\/span><span class="small faint">\$12\.50 of \$50/, "the panel does not show the month's spend against the limit");
assert.match(spendCard, /#magnus-alpha<\/span><span class="small faint">\$4 of \$10/, "the panel does not show the channel's spend");
Team.home = home;
assert.equal(sandbox.teamUsd(0.004), "under $0.01", "a few calls' spend reads as nothing");
assert.equal(sandbox.teamUsd(12), "$12");
assert.match(workItem({ ...work, doing: null, steps: 1 }), /Thinking it through/, "work with no action yet says it is starting");

assert.doesNotMatch(workItem({ ...work, doing: null, steps: 0 }), /step|Starting/, "a reply not begun yet counts steps");
assert.doesNotMatch(draw({ id: "w1", sender: "agent", requester: "dana", text: "", thread: "m1", at: "", work }, { thread: "m1" }), /\/t\/m1/, "a work card inside its own thread links to that thread");
assert.match(workItem({ ...work, doing: "task_complete" }), /Finishing up/);

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

// Ctrl+K right after Escape opens the menu: the shut dialog still awaiting
// its close event once made the keys "close" it instead.
const { TeamCommand } = sandbox.__x;
let opened = 0;
TeamCommand.open = () => { opened++; };
TeamCommand.dialog = { open: false, close() {} };
for (const f of listeners.keydown) f({ ctrlKey: true, metaKey: false, altKey: false, key: "k", preventDefault() {} });
assert.equal(opened, 1, "Ctrl+K after Escape did not open the menu again");

console.log("team ok");
