// Wave A regression checks: schedule Run now / Edit, task Pause, deck & table exports,
// deliverable bar, personal spend meter, Memory & Promises settings tab, truthful onboarding,
// and approval card fixes.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const read = f => readFileSync(join(root, f), "utf8");

const app = read("desktop/ui/app.js");
const ts = read("desktop/ui/tasks-schedules.js");
const compose = read("desktop/ui/compose.js");
const frames = read("desktop/ui/composed-frames.js");
const chatView = read("desktop/ui/chat-view.js");
const settings = read("desktop/ui/settings-sections.js");
const onboarding = read("desktop/ui/onboarding.js");

// 1. Schedule controls: Run now + Edit form, no duplicate Stop + Delete buttons on active schedules.
assert.ok(ts.includes('data-act="run-schedule"'), "scheduleButtons offers Run now");
assert.ok(!ts.includes('data-act="stop-schedule"'), "scheduleButtons no longer duplicates Stop alongside Delete");
assert.ok(ts.includes('data-edit-schedule='), "scheduleView offers inline schedule edit form");
assert.ok(app.includes('case "run-schedule":') && app.includes("Server.schedules.run("), "wirePanel handles run-schedule");
assert.ok(app.includes("form[data-edit-schedule]") && app.includes("Server.schedules.edit("), "wirePanel handles schedule editing");

// 2. Task Pause & paused_by_user waiting text.
assert.ok(app.includes('paused_by_user: "paused by you"'), "WAITING includes paused_by_user");
assert.ok(app.includes('data-act="pause-task"') && app.includes('case "pause-task":') && app.includes("Server.tasks.pause("), "taskView and wirePanel expose task Pause");

// 3. Deck PPTX/PDF export, Table Copy/CSV, HTML Fullscreen, Deliverable toolbar, data-ui-id.
assert.ok(compose.includes('class="deck-export" data-format="pptx"') && compose.includes('data-format="pdf"'), "compose.js renders deck PPTX and PDF export buttons");
assert.ok(compose.includes('class="table-btn table-copy"') && compose.includes('table-btn table-csv'), "compose.js renders table Copy and CSV buttons");
assert.ok(chatView.includes('data-ui-id="${esc(p.id || "")}"'), "chat-view.js emits data-ui-id on .ui-block");
assert.ok(frames.includes("wireFrameExports") && frames.includes("/v1/ui/docs/") && frames.includes("data-html-fullscreen"), "composed-frames.js wires deck/table exports and HTML fullscreen");
assert.ok(app.includes("data-deliverable-bar") && app.includes('data-act="copy-deliverable"') && app.includes('data-act="export-deliverable"'), "taskView includes deliverable action toolbar");

// 4. Personal Spend Meter and Memory & Promises tab.
assert.ok(settings.includes("function personalSpendCard(") && settings.includes("Server.spend.get()"), "settings-sections.js renders personal spend meter");
assert.ok(settings.includes("async function memorySection(") && settings.includes("Server.memory.summary()") && settings.includes("Server.memory.playbook()"), "settings-sections.js provides personal Memory & Playbook section");
assert.ok(app.includes('["memory", "Memory & Promises", memorySection'), "Settings modal includes Memory & Promises tab");

// 5. Onboarding truthfulness & approval card fixes.
assert.ok(!onboarding.includes("Auto-approve verified supplier orders under $500"), "onboarding simulator does not promise auto-approved spending");
assert.ok(!onboarding.includes("data-disc-mode"), "Discovery wizard does not render fake prompt-stuffed Safety mode select");
assert.ok(!app.includes("Settings → Rules"), "Allow always toast does not reference nonexistent Settings -> Rules tab");
assert.ok(app.includes('approval: ["approve", "reject"') && app.includes('Open →</a>'), "needsInlineCard gates quick approve/deny verbs to approval/review/endorsement");
assert.ok(!app.includes('<div class="needs-item appr-hist-row"'), "needsView does not duplicate card header above card(i)");

console.log("wave-a check passed");
