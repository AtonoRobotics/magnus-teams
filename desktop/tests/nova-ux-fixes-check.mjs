// What the 2026-11-26 review of Nova against its spec found broken, each
// checked so it stays fixed (decisions/2026-11-26-nova-matches-its-spec.md):
// batch approval (⌘K call shape, Tier 1 never batched, same kind only), the
// Briefing (summary from /v1/briefing, live routines only, honest dry run,
// spend and snooze), the de-cluttered top bar, one inbox count, and the
// Apps spotlight at the top.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const read = f => readFileSync(join(here, "../ui", f), "utf8");
const app = read("app.js");
const pages = read("nova-pages.js");
const ux = read("nova-ux.js");
const rail = read("aurora-rail.js");
const stage = read("aurora-stage.js");
const discover = read("discover.js");

// 1. Batch approval: ⌘K passes ids then body, as Server.inbox.batch takes them.
assert.match(app, /Server\.inbox\.batch\(batchable, \{ decision: "approve", scope: "once" \}\)/, "⌘K Approve all calls batch(ids, body)");
assert.doesNotMatch(app, /Server\.inbox\.batch\(\{\s*ids/, "no batch call passes one object");
const decl = app.match(/const BATCH_EXCLUDED_TIERS = [^\n]+\n(const batchableApprovals = pending => \{[\s\S]*?\n\};)/);
assert.ok(decl, "batchableApprovals is defined in app.js");
const sb = {};
vm.runInNewContext(`${app.match(/const BATCH_EXCLUDED_TIERS = [^\n]+/)[0]}\n${decl[1]}\nthis.batchableApprovals = batchableApprovals;`, sb);
const ap = (id, tier, tool) => ({ id, kind: "approval", can_respond: true, tier, details: { tool } });
assert.deepEqual([...sb.batchableApprovals([ap("a", "reversible_write", "mail__send"), ap("b", "reversible_write", "mail__send"), ap("c", "reversible_write", "slack__post")])], ["a", "b"], "only approvals of one kind batch");
assert.deepEqual([...sb.batchableApprovals([ap("a", "irreversible", "mail__send"), ap("b", "irreversible", "mail__send")])], [], "irreversible approvals (Tier 1) never batch");
assert.deepEqual([...sb.batchableApprovals([ap("a", "financial", "pay"), ap("b", "financial", "pay")])], [], "financial approvals never batch");
assert.deepEqual([...sb.batchableApprovals([ap("a", "read", "x")])], [], "one approval is no batch");
assert.match(stage, /batchableApprovals\(pending\)/, "the stage's batch bar uses the same rule");

// 2. The Briefing page, drawn with what the server sends.
{
  let html = "";
  const main = { onclick: null };
  const ctx = {
    App: {
      thread: null, tasks: [{ status: "running" }],
      schedules: [
        { id: "s1", title: "Morning digest", every_minutes: 1440, next: "soon" },
        { id: "s2", title: "Heartbeat", system: true, every_minutes: 30 },
        { id: "s3", title: "Old report", ended: "done", every_minutes: 60 },
        { id: "s4", title: "Paused check", paused: "x", every_minutes: 120 },
      ],
    },
    ENDED: ["completed", "failed", "cancelled"],
    needsMe: () => [],
    esc: s => String(s ?? "").replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]),
    enc: encodeURIComponent,
    until: s => `in ${s}`,
    rule: s => (s.every_minutes === 1440 ? "Every day" : `Every ${s.every_minutes} minutes`),
    paint: (_el, h) => { html = h; },
    toast: () => {},
    Notices: { isDnd: () => false, snooze: () => {} },
    Server: {
      call: async () => ({ stats: { completed_24h: 2, active_count: 1, staged_count: 0, overdue_count: 1 }, upcoming_schedules: [{ id: "s2", next: "x" }, { id: "s1", next: "soon" }] }),
      spend: { get: async () => ({ spent_usd: 3.5, limit_usd: 25 }) },
    },
  };
  vm.runInNewContext(`${pages}\nthis.NovaPages = NovaPages;`, ctx);
  await ctx.NovaPages.showBriefing(main);
  assert.match(html, /nova-brief-summary/, "the Briefing shows a summary written from /v1/briefing");
  assert.match(html, /2 tasks finished in the last day/);
  assert.match(html, /1 commitment is overdue/);
  assert.match(html, /next up: Morning digest/, "the next routine is named, skipping a system one");
  assert.match(html, /data-routine-id="s1"/);
  assert.doesNotMatch(html, /data-routine-id="s2"/, "a system schedule is no routine to run");
  assert.doesNotMatch(html, /data-routine-id="s3"/, "an ended schedule is no routine to run");
  assert.match(html, /Every day/, "a routine shows its cadence");
  assert.doesNotMatch(html, /data-routine-run="s4"/, "a paused routine offers no Run now");
  assert.match(html, /Spend This Month[\s\S]*\$3\.50[\s\S]*of \$25\.00 limit/, "the Briefing shows the month's spend");
  assert.match(html, /data-brief-dnd/, "the Briefing offers to snooze notifications");
}
assert.match(pages, /prompt_preview/, "a dry run says what the routine would ask");
assert.doesNotMatch(pages, /brief\?\.summary/, "no field the server never sends");

// 3. Top bar: in Nova, Share and the computer chip are in the ••• menu, and
// the quick controls are in the top bar across every screen (nova-frame.js),
// which also says where the person is and this month's spend.
const frame = read("nova-frame.js");
assert.match(app, /const inMenu = nova \? `\$\{chip\}\$\{shareBtn\}` : "";/, "Nova puts Share and the computer chip in the menu");
assert.match(app, /<div class="row">\$\{nova \? "" : `\$\{chip\}\$\{shareBtn\}`\}/, "Nova's head shows neither outside the menu");
assert.doesNotMatch(app + ux, /topBarHtml/, "the conversation head no longer paints the quick controls");
assert.match(frame, /data-nova-set-avatar/, "the top bar has the companion pill");
assert.match(frame, /data-nova-toggle-mode/, "the top bar has Light / Dark");
assert.match(frame, /Spend this month: /, "the top bar shows the month's spend, the only period the server keeps");
assert.match(read("aurora-pages.js"), /dataset\.style === "nova"\) \{ mine\?\.remove\(\); return; \}/, "Nova pages get no lone close");
assert.match(read("index.html"), /<script src="nova-frame\.js"><\/script>/, "the app loads the top bar");
assert.match(read("nova-shell.css"), /\.shell > \.nova-frame-bar \{[^}]*grid-row: 1;[^}]*grid-column: 2 \/ -1;/, "the bar is the shell's first row, beside the rail");

// Titles are words: a conversation started with Markdown is named without it.
{
  const src = app.slice(app.indexOf("function displayTitle("), app.indexOf("function navigationTree("));
  const displayTitle = new Function(`${src}; return displayTitle;`)();
  assert.equal(displayTitle("**Plan** the offsite"), "Plan the offsite");
  assert.equal(displayTitle("# Read [the doc](https://x.test) now"), "Read the doc now");
  assert.equal(displayTitle("snake_case_name stays"), "snake_case_name stays");
  // A title cut mid-sentence keeps no unpaired marks, and a list item inside
  // bold loses its number, as seen in the installed app on 2026-11-26.
  assert.equal(displayTitle("**1. Product Review: What Magnus Actually Is (and Its Real Moat)** Magnus is **"), "Product Review: What Magnus Actually Is (and Its Real Moat) Magnus is");
  assert.equal(displayTitle("Ship it `now"), "Ship it now");
}

// A label in the navigation shows beside its item, not over the next one,
// which it held open and whose click it took (the installed app, 2026-11-26).
assert.match(app, /if \(trigger\.closest\("nav\.side"\)\) \{\n\s+tip\.style\.left = `\$\{Math\.min\(r\.right \+ 8/, "navigation labels sit beside their item");

// 4. One inbox count in Nova: not on the sidebar toggle, and the rail's
// sidebar button counts only running work.
assert.match(app, /pending && !Panel\.what && document\.documentElement\.dataset\.style !== "nova"/, "Nova's sidebar toggle carries no waiting count");
assert.match(rail, /const count = nova \? active : waiting \|\| active;/, "Nova's rail button counts running work only");
assert.match(app, /inbox: "Inbox \/ Approvals", briefing: "Briefing & Routines", apps: "Apps & Skills"/, "Nova's rail labels follow the spec");

// 5. Home title and the Apps spotlight.
assert.match(discover, /"Where should we start\?"/, "Nova's home asks where to start");
assert.match(ux, /:scope > \.lhead[\s\S]*head\.insertAdjacentElement\("afterend", hero\)/, "the spotlight goes straight under the Apps header");
assert.match(stage, /\$\{v\.foot \|\| ""\}/, "the stage shell can carry the computer footer");

console.log("nova-ux-fixes-check: ok");
