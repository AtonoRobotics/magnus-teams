// Regression checks for rich agent findings, approvals, sub-agents, evidence,
// dynamic bento boxes, right-sidebar top-bar tabs, schedule deletion, and
// multi-task deletion (decisions/2026-11-23-rich-agent-ux-bento-and-concurrency.md).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const uiDir = join(here, "../ui");
const read = f => readFileSync(join(uiDir, f), "utf8");
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);

// 1. Approval cards surface structured human-facing preview without leaking policy/reasoning.
{
  const sandbox = { esc };
  sandbox.globalThis = sandbox;
  vm.runInNewContext(read("asks.js"), sandbox);
  const item = {
    id: "a1", kind: "approval", tier: "irreversible",
    consequence: "Sent as you and cannot be undone.",
    ask: { sentence: "Send the Q3 board update to board@acme.example?", offers: ["allow", "dont_allow"], allow_scope: "once" },
    details: {
      destinations: { recipients: { to: ["board@acme.example"] }, content: { subject: "Q3 Board Update", body: "ARR reached $14.2M (+28% YoY), gross margin 78%." } },
      input: { draft_id: "d1", to: "eve@evil.example", subject: "Model's subject", body: "The model's own words, not the draft." },
      rule: "person_rule:secret", why: "Auto Review: outside request", taint: { untrusted: true },
    },
  };
  const card = sandbox.askCard(item, true);
  assert.match(card, /data-ask-preview/, "askCard renders visible structured preview above details");
  assert.match(card, /board@acme\.example/, "preview shows recipient up front");
  assert.match(card, /Q3 Board Update/, "preview shows subject up front");
  assert.match(card, /ARR reached \$14\.2M/, "preview shows body excerpt up front");
  assert.ok(!card.includes("person_rule"), "preview never leaks policy rule");
  assert.ok(!card.includes("Auto Review"), "preview never leaks reviewer reasoning");
  const fields = sandbox.askSummaryFields(item);
  assert.equal(fields.rows[0][0], "To");
  assert.equal(fields.rows[0][1], "board@acme.example");
  // The preview is what the gate read, never the model's input (2026-11-23).
  const preview = card.match(/<div class="ask-preview" data-ask-preview>([\s\S]*?)<\/div>\s*<details/);
  assert.ok(preview, "the preview sits above the content");
  for (const model of ["eve@evil.example", "Model's subject", "Model&#39;s subject", "The model's own words", "The model&#39;s own words"]) {
    assert.ok(!preview[1].includes(model), `the preview shows the model's ${model}`);
  }
  assert.equal(fields.body, "ARR reached $14.2M (+28% YoY), gross margin 78%.");
  assert.ok(!fields.rows.some(([, v]) => v.includes("eve@evil.example") || v.includes("Model's subject")), "a row came from the model's input");
  // Where the gate read no recipients, the preview names where the server says it goes, and nothing from the input.
  const plain = sandbox.askSummaryFields({ ask: { destinations: ["bob@corp.example"] }, details: { input: { to: "eve@evil.example", body: "hi eve" } } });
  assert.equal(JSON.stringify(plain.rows), JSON.stringify([["Destination", "bob@corp.example"]]));
  assert.equal(plain.body, "");
}

// 2. Sub-agents are clickable, show live status, expanded outcome_view findings, and files.
{
  const sandbox = {
    esc, enc: encodeURIComponent, firstLine: s => String(s || "").split("\n")[0],
    when: iso => iso ? `at ${iso}` : "",
    WAITING: { approval: "waiting for approval" },
    waitingCard: () => "",
    statusLineSlot: (t, cls) => `<span class="${cls}" data-status-line="${esc(t.task_id)}">${esc(t.status_line || "Working")}</span>`,
  };
  sandbox.globalThis = sandbox;
  vm.runInNewContext(read("subagents.js"), sandbox);
  const html = sandbox.subAgentsCard({
    id: "s:1", kind: "sub_agents", task_id: "t0", from_run: "r0", at: "2026-11-23T10:00:00Z",
    agents: [
      { run: "r1", parent: "r0", depth: 1, task_id: "t1", goal: "Audit Q3 spend", status: "running", waiting_on: null, waiting_item: null, status_line: "Scanning 142 invoices", summary: null, tools: [], at: "", ended_at: null },
      {
        run: "r2", parent: "r0", depth: 1, task_id: "t2", goal: "Benchmark pricing", status: "completed", waiting_on: null, waiting_item: null,
        summary: "Compared 3 tiers.",
        outcome_view: [
          { type: "stat", label: "Median Pro", value: "$49/mo" },
          { type: "table", headers: ["Vendor", "Price"], rows: [["Acme", "$45"], ["Beta", "$55"]] },
        ],
        outputs: [{ id: "f9", name: "pricing.csv" }],
        tools: [], at: "", ended_at: "2026-11-23T10:05:00Z",
      },
    ],
  });
  assert.match(html, /data-open="task:t1"/, "running sub-agent opens its task on click");
  assert.match(html, /Scanning 142 invoices/, "running sub-agent shows live status_line");
  assert.match(html, /<details open data-key="sub-agent:r2">/, "completed sub-agent findings are expanded by default");
  assert.match(html, /Median Pro: \$49\/mo/, "sub-agent renders outcome_view stats");
  assert.match(html, /Acme \| \$45/, "sub-agent renders outcome_view table rows");
  assert.match(html, /data-open="file:f9">pricing\.csv<\/a>/, "sub-agent shows delivered file chips");
}

// 3. Evidence shows receipt recipients, inline read-back summary, full sources, and not_done tools.
{
  const sandbox = { esc, when: t => String(t) };
  sandbox.globalThis = sandbox;
  vm.runInNewContext(read("evidence.js"), sandbox);
  const html = sandbox.evidenceSection({
    evidence: {
      receipts: [{
        call_id: "c1", tool: "mail__send", what: "Sent quarterly report",
        to: ["cfo@acme.example", "ceo@acme.example"], at: "2026-11-23T10:00:00Z",
        read_back: { call_id: "c1", tool: "mail__send", matches: true, note: "Verified in Sent folder.", found: { subject: "Q3 Report", status: "delivered" } },
      }],
      sources: [{ claim: "ARR is $14.2M", call_id: "c0", tool: "sheets__read", at: "Q3-Model.xlsx" }],
      not_done: [{ call_id: "c2", tool: "slack__post", status: "denied", what: "Post to #general" }],
    },
  });
  assert.match(html, /data-receipt-to>To: cfo@acme\.example, ceo@acme\.example<\/div>/, "receipt renders verified destinations");
  assert.match(html, /data-read-back-summary>subject: Q3 Report · status: delivered<\/div>/, "read-back surfaces concise inline verified fields");
  assert.match(html, /Q3-Model\.xlsx · sheets__read/, "source shows both location and tool");
  assert.match(html, /Post to #general.*\(slack__post\)/, "not_done shows tool context");
}

// 4. Tasks & Schedules: inline task delete on ended rows, Delete on active/paused/ended schedules, and no Heartbeat card.
{
  const sandbox = {
    esc, enc: encodeURIComponent,
    when: t => String(t || ""), until: t => String(t || ""), rule: () => "Every 30m", ends: () => "until you stop it",
    firstLine: s => String(s || "").split("\n")[0],
    chip: s => `<span class="chip">${s}</span>`,
    icon: k => `<svg data-ic="${k}"></svg>`,
    moveControl: () => "",
    fieldValue: () => "",
    latestOutputs: () => [],
    WAITING: {},
    ENDED: ["completed", "failed", "cancelled"],
    ExploreIdeas: { open: () => [] },
    App: {
      tasks: [
        { task_id: "t-run", title: "Running job", text: "Running job", status: "running", created_at: "2026-11-23T10:00:00Z" },
        { task_id: "t-done", title: "Done job", text: "Done job", status: "completed", created_at: "2026-11-23T09:00:00Z", ended_at: "2026-11-23T09:05:00Z" },
      ],
      schedules: [
        { id: "sc-hb", title: "Heartbeat", text: "Heartbeat check", paused: false, ended: null, next: "2026-11-23T11:00:00Z", runs: [] },
        { id: "sc-act", title: "Morning brief", text: "Daily brief", paused: false, ended: null, next: "2026-11-24T08:00:00Z", runs: [] },
        { id: "sc-end", title: "Old routine", text: "Old", paused: false, ended: "reached limit", next: null, runs: [] },
      ],
      inbox: [], projects: [],
    },
  };
  sandbox.globalThis = sandbox;
  vm.runInNewContext(read("tasks-schedules.js"), sandbox);
  const tHtml = sandbox.tasksView();
  assert.match(tHtml, /data-trash="t-done"/, "ended task row offers inline Delete button");
  assert.doesNotMatch(tHtml, /data-trash="t-run"/, "running task row does not offer Delete");
  const sHtml = sandbox.schedulesView();
  assert.doesNotMatch(sHtml, /data-id="sc-hb"|data-enable-heartbeat|Heartbeat/, "the retired heartbeat is neither shown nor offered");
  assert.match(sHtml, /data-act="delete-schedule" data-id="sc-act">Delete<\/button>/, "active schedule offers Delete");
  assert.match(sHtml, /data-act="delete-schedule" data-id="sc-end">Delete schedule<\/button>/, "ended schedule offers Delete schedule");
}

// 5. Right sidebar top-bar tabs (both standard & Aurora), keeping sidebar open on task delete, and dynamic bento wiring.
const app = read("app.js");
const stage = read("aurora-stage.js");
const chatCss = read("chat.css");

assert.doesNotMatch(chatCss, /\.chatcard\s+\.rg-main\s+\.sub\s*\{\s*display:\s*none/, "chat.css no longer hides task/sub-agent status subtitles");
assert.match(app, /<details open data-key="result:/, "inline completed results render expanded by default");
assert.match(app, /if \(Panel\.what === `task:\$\{itemId\}`\) await openPanel\("tasks"\)/, "deleting a task keeps right sidebar open on Tasks");
assert.doesNotMatch(app, /Panel\.what === `task:\$\{itemId\}` \|\| Panel\.what === `project:\$\{itemId\}`\) closePanel\(\)/, "deleting a task never closes the right sidebar");
assert.match(app, /if \(Panel\.what === `schedule:\$\{id\}`\) await openPanel\("schedules"\)/, "deleting an open schedule navigates back to Schedules");
for (const tab of ['["activity", "Activity"', '["tasks", "Tasks"', '["schedules", "Routines"', '["needs", "Approvals"', '["files", "Files"', '["apps", "Apps"']) {
  assert.ok(app.includes(tab), `right sidebar top bar includes tab ${tab}`);
}
assert.match(app, /AuroraStage\.shell\(\{ kind, title, head, backBtn, tabsHtml, body[,} ]/, "Aurora right sidebar receives top-bar tabsHtml");
assert.match(stage, /MAX_CONCURRENT_TASK_BOXES/, "Aurora bento surfaces concurrent autonomous tasks");
assert.match(stage, /MAX_APPROVAL_BOXES/, "Aurora bento surfaces multiple approvals");
assert.match(stage, /t\.receipts_count > 0 \|\| t\.has_deliverable/, "Verified badge checks server-returned receipts_count/has_deliverable");
assert.ok(stage.indexOf("slide(bento, 1);") < stage.indexOf("grow(bento, was);"), "FLIP slide runs before container height animation");

// 6. Stop keeps a schedule and Delete removes it; a card put off is kept,
// to accept or delete later (owner, 2026-11-23).
{
  const chat = read("personal-chat.js");
  const caseOf = (src, name) => src.slice(src.indexOf(`case "${name}"`), src.indexOf("break;", src.indexOf(`case "${name}"`)));
  assert.match(caseOf(app, "stop-schedule"), /Server\.schedules\.stop\(id\)/, "Stop does not stop the schedule");
  assert.doesNotMatch(caseOf(app, "stop-schedule"), /Server\.schedules\.remove/, "Stop deletes the schedule");
  assert.match(caseOf(app, "delete-schedule"), /Server\.schedules\.remove\(id\)/, "Delete does not remove the schedule");
  assert.match(chat, /k === "stop"[^]*?Server\.schedules\.stop\(id\)/, "the thread's Stop does not stop the schedule");
  assert.match(app, /act\("stop", "Stop"\)\}\$\{act\("cancel", "Delete"\)/, "a schedule in a thread does not offer Stop and Delete");
  const suggestion = app.slice(app.indexOf('case "suggestion_card"'), app.indexOf('case "grant_card"'));
  assert.match(suggestion, /e\.state !== "open" && !later/, "a card put off is drawn as decided");
  assert.match(suggestion, /cardActs\(e, ui, "Accept", !later, later \? "Delete" : "Decline"\)/, "a card put off offers no Accept and Delete");
}

console.log("agent ux check passed");
