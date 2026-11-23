// Magnus's ideas are in Explore's For you, not in Approvals
// (decisions/2026-11-21-ideas-go-to-explore.md), with the app as it is:
// - For you lists the ideas still waiting, newest first, each drawn as its
//   inbox card, and is not there when none waits;
// - an idea is no decision waiting, so it makes no Approvals entry and no
//   box, and the overview's other items leave it out;
// - the notice that ideas are waiting links to Explore, and is the one
//   notification for them;
// - an empty Tasks or Schedules offers the ideas, linking to Explore;
// - the app loads it before Explore.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const read = (/** @type {string} */ f) => readFileSync(join(here, "../ui", f), "utf8");
const code = read("explore-ideas.js"), app = read("app.js"), tasksSchedules = read("tasks-schedules.js"), discover = read("discover.js"), index = read("index.html");

const ctx = { String };
vm.createContext(ctx);
vm.runInContext(`${code}\nthis.ExploreIdeas = ExploreIdeas;`, ctx);
const { ExploreIdeas } = /** @type {any} */ (ctx);

const idea = (/** @type {string} */ id, /** @type {string} */ at, status = "open") => ({ id, kind: "suggestion", status, created_at: at, summary: `Magnus suggests: ${id}`, details: { title: id } });
const inbox = [idea("old", "2026-11-21T08:00:00Z"), idea("new", "2026-11-21T09:00:00Z"), idea("done", "2026-11-21T10:00:00Z", "resolved"), { id: "a1", kind: "approval", status: "open", created_at: "2026-11-21T11:00:00Z" }];
assert.deepEqual(Array.from(ExploreIdeas.open(inbox), (/** @type {any} */ i) => i.id), ["new", "old"], "the ideas still waiting, newest first");
const html = ExploreIdeas.html(inbox, (/** @type {any} */ i) => `<article data-id="${i.id}"></article>`);
assert.match(html, /<h2 id="explore-ideas-head">For you <span class="muted small">\(2\)<\/span><\/h2>/);
assert.ok(html.indexOf('data-id="new"') < html.indexOf('data-id="old"') && !html.includes('data-id="done"') && !html.includes('data-id="a1"'), "each idea waiting is drawn as its card");
assert.equal(ExploreIdeas.html([inbox[2], inbox[3]], () => "x"), "", "no For you when no idea waits");
assert.ok(ExploreIdeas.isNotice({ kind: "notice", details: { reason: "ideas" } }) && !ExploreIdeas.isNotice({ kind: "notice", details: { reason: "failed" } }));

// No decision waiting: out of Approvals, the boxes and the overview's other items.
const kinds = app.match(/const DECISION_KINDS=new Set\(\[([^\]]*)\]\)/)?.[1] || "";
assert.ok(kinds.includes('"approval"') && !kinds.includes('"suggestion"'), "an idea is a decision waiting in Approvals");
assert.match(app, /const other = App\.inbox\.filter\(i => i\.status === "open" && !needsDecision\(i\) && !ExploreIdeas\.isIdea\(i\)\);/, "the overview lists ideas");
// Its notice links to Explore, and is the notification for them.
assert.match(app, /if \(ExploreIdeas\.isNotice\(i\)\) \{\s*body = `<p class="small"><a href="#\/explore">Open Explore<\/a>/, "the ideas notice does not link to Explore");
assert.match(app, /Notices\.show\(\{ key: `inbox:\$\{i\.id\}`, title: "Ideas in Explore", body: i\.summary, system: i\.summary, href: "#\/explore" \}\)/, "the ideas notice is not shown as a notification");
// An empty Tasks or Schedules offers them, linking to Explore, not to a conversation.
assert.match(tasksSchedules, /const ideas = ExploreIdeas\.open\(App\.inbox\);[\s\S]{0,400}<a href="#\/explore">/, "the empty lists do not offer the ideas in Explore");
assert.ok(!/t\.opener\)\.flatMap/.test(app) && !/t\.opener\)\.flatMap/.test(tasksSchedules), "the empty lists still read conversations Magnus opened");
// Explore draws For you above the apps, and redraws once one is answered.
assert.match(discover, /ExploreIdeas\.html\(App\.inbox, i => card\(i\)\)[\s\S]{0,200}<section class="explore-apps"/, "Explore does not draw For you");
assert.match(discover, /const redraw = async \(\) => \{ if \(typeof pollInbox === "function"\) await pollInbox\(\); draw\(\); \};/, "Explore does not redraw once an idea is answered");
const at = (/** @type {string} */ s) => index.indexOf(`<script src="${s}"></script>`);
assert.ok(at("explore-ideas.js") >= 0 && at("explore-ideas.js") < at("discover.js"), "index.html loads explore-ideas.js before discover.js");
console.log("explore ideas check passed");
