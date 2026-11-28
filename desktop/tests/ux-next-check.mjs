// Fast-tier unit checks for consolidated UX Next modules:
// ComposerCommands (/ and @), InlineComments, UndoToast, SplitCoedit, WorkQueue, and CommandBar.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const read = f => readFileSync(join(here, "../ui", f), "utf8");

function loadModule(file, exportName, extra = {}) {
  const ctx = {
    console, setTimeout, clearTimeout,
    App: {
      projects: [{ id: "p1", name: "Q4 Launch" }],
      files: [{ id: "f1", name: "pricing-memo.md" }],
      connectors: [{ name: "gmail", title: "Gmail", state: "connected" }],
      threads: [{ thread: "t1", title: "Vendor Audit" }],
      tasks: [{ task_id: "k1", title: "Renewals", status: "running" }],
      schedules: [{ id: "s1", title: "Weekly Digest", paused: false }],
      workspaces: [],
    },
    displayTitle: s => s,
    enc: encodeURIComponent,
    esc: s => String(s ?? ""),
    ...extra,
  };
  ctx.globalThis = ctx;
  vm.createContext(ctx);
  vm.runInContext(`${read(file)}\nglobalThis.${exportName} = ${exportName};`, ctx);
  return ctx[exportName];
}

// 1. ComposerCommands: / workflows and @ typed context mentions.
const ComposerCommands = loadModule("composer-commands.js", "ComposerCommands");
const slashTrig = ComposerCommands.triggerAt("hello /pl", 9);
assert.deepEqual({ ...slashTrig }, { kind: "/", query: "pl", start: 6, end: 9 });
const slashChoices = ComposerCommands.items(slashTrig);
assert.equal(slashChoices[0].token, "/plan");
const applied = ComposerCommands.apply("hello /pl", slashTrig, slashChoices[0]);
assert.equal(applied.text, "hello ");
assert.equal(applied.chip.token, "/plan");

const atTrig = ComposerCommands.triggerAt("check @pric", 11);
assert.equal(atTrig.kind, "@");
const atChoices = ComposerCommands.items(atTrig, { mentions: () => [{ name: "alice", agent: false }] });
assert.ok(atChoices.some(c => c.token === "@file:pricing-memo.md"), "@ context resolves files");
assert.ok(atChoices.some(c => c.token === "@alice" && c.inline === true), "@ context preserves inline people");
const payload = ComposerCommands.composePayload("Ship memo", [slashChoices[0], { token: "@file:pricing-memo.md", kind: "file" }]);
assert.match(payload, /\[\/plan\]/);
assert.match(payload, /Context: @file:pricing-memo\.md/);
assert.match(payload, /Ship memo/);

// 2. InlineComments: keyed notes and structured revision prompt.
const InlineComments = loadModule("inline-comments.js", "InlineComments");
assert.equal(InlineComments.count("plan-1"), 0);
InlineComments.add("plan-1", "step-0", "Exclude pilot accounts");
InlineComments.add("plan-1", "step-2", "Use 60-day notice");
assert.equal(InlineComments.count("plan-1"), 2);
const rev = InlineComments.formatRevision("plan-1", { "step-0": "Step 1", "step-2": "Step 3" });
assert.match(rev, /\[Step 1\]: Exclude pilot accounts/);
assert.match(rev, /\[Step 3\]: Use 60-day notice/);
assert.match(InlineComments.footerHtml("plan-1"), /Revise with 2 comments/);
InlineComments.clear("plan-1");
assert.equal(InlineComments.count("plan-1"), 0);

// 3. UndoToast: 5s window, cancel restores without committing, flush commits immediately.
const UndoToast = loadModule("undo-toast.js", "UndoToast");
assert.equal(UndoToast.UNDO_WINDOW_MS, 5000);
let committed = 0, undone = 0;
UndoToast.schedule({ label: "Dismissed", delayMs: 5000, commit: () => { committed++; }, undo: () => { undone++; } });
assert.equal(UndoToast.isPending(), true);
assert.equal(UndoToast.cancel(), true);
assert.equal(committed, 0);
assert.equal(undone, 1);
UndoToast.schedule({ label: "Dismissed 2", delayMs: 5000, commit: () => { committed++; }, undo: () => { undone++; } });
UndoToast.flush();
assert.equal(committed, 1);
assert.equal(UndoToast.isPending(), false);

// 4. SplitCoedit: diffHunks, resolveHunk, and buildDocument.
const SplitCoedit = loadModule("split-coedit.js", "SplitCoedit");
const hunks = SplitCoedit.diffHunks("Old intro.\n\nOld body.", "New intro.\n\nOld body.");
assert.equal(hunks.length, 1);
assert.equal(hunks[0].before, "Old intro.");
assert.equal(hunks[0].after, "New intro.");
const accepted = SplitCoedit.resolveHunk(hunks, hunks[0].id, "accept");
assert.equal(SplitCoedit.buildDocument("Old intro.\n\nOld body.", accepted), "New intro.\n\nOld body.");
const rejected = SplitCoedit.resolveHunk(hunks, hunks[0].id, "reject");
assert.equal(SplitCoedit.buildDocument("Old intro.\n\nOld body.", rejected), "Old intro.\n\nOld body.");

// 5. WorkQueue: compact ambient pill strip.
const WorkQueue = loadModule("work-queue.js", "WorkQueue");
assert.equal(WorkQueue.html({ running: 0, waiting: 0, sessionUpdates: false }), "");
const wq = WorkQueue.html({ running: 2, waiting: 1, sessionUpdates: true });
assert.match(wq, /2 running/);
assert.match(wq, /1 needs you/);
assert.match(wq, /data-act="dismiss-updates"/);

// 6. CommandBar: unified personal + files + schedules + Split Co-Edit actions.
const CommandBar = loadModule("command-bar.js", "CommandBar", {
  needsMe: () => [{ id: "a1", summary: "Approve email" }],
  batchableApprovals: () => [],
});
const allCmds = CommandBar.items("");
assert.ok(allCmds.some(c => c.label.includes("Approve next ask")), "CommandBar includes Approve next ask");
assert.ok(allCmds.some(c => c.label.includes("Split Co-Edit")), "CommandBar includes Open Split Co-Edit");
const fileCmds = CommandBar.items("pricing-memo");
assert.ok(fileCmds.some(c => c.kind === "File" && c.label === "pricing-memo.md"), "CommandBar fuzzy-searches files");
const schedCmds = CommandBar.items("weekly digest");
assert.ok(schedCmds.some(c => c.kind === "Schedule"), "CommandBar fuzzy-searches schedules");

console.log("ux-next-check: ok");
