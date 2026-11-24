// The page aurora-alerts.mjs checks and aurora-filmstrip-alerts.mjs films:
// shaped like the app's, with its theme and sheets, asks.js, notices.js and
// the Aurora scripts, and what app.js gives them cut to what they read.
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const read = (/** @type {string} */ name) => readFileSync(join(here, "../ui", name), "utf8");

// What app.js gives these scripts, cut to what they read; the answer, the
// thread and the canvas are drawn again at once, as app.js does.
const stubs = `
var FX = { pending: [], opened: [] };
var ENDED = ["completed", "failed", "stopped", "cancelled"];
var App = { me: { id: "p1" }, thread: "c1", tasks: [], schedules: [], connectors: [], computers: [] };
var Panel = { what: null, autoOpened: false };
var Server = { files: { list: async () => ({ files: [] }) }, tasks: { get: async () => ({}) } };
var Magnus = { kind: "desktop", notifyPermission: () => "granted" };
function $(q, r) { return (r || document).querySelector(q); }
function esc(s) { return String(s ?? "").replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]); }
function icon(k) { return '<svg class="ic" data-icon="' + k + '"></svg>'; }
function when() { return "just now"; } function until() { return ""; } function firstLine(s) { return String(s || "").split("\\n")[0]; }
function enc(s) { return encodeURIComponent(s); } function stateOf(c) { return c.state; }
function needsYou() { return false; } function needsMe() { return FX.pending; }
function toast(msg, ms = 3200) {
  if (typeof AuroraReasoning !== "undefined" && AuroraReasoning.tell(String(msg), ms)) return;
  const t = document.createElement("div"); t.className = "toast"; t.textContent = msg; t.setAttribute("role", "status");
  let box = $("#toasts"); if (!box) { box = document.createElement("div"); box.id = "toasts"; document.body.append(box); }
  box.append(t); setTimeout(() => t.remove(), ms);
}
function card(i) { return '<article class="card item approval" data-id="' + esc(i.id) + '"><div class="spread"><span class="kind">Approval</span></div>' + askCard(i, true) + '</article>'; }
function drawThread() {
  const mine = FX.pending.filter(i => i.thread === "c1");
  document.getElementById("thread").innerHTML = '<div class="entry me" data-entry="m1" data-kind="message"><div class="msg me">Book the trip</div></div>'
    + '<div class="entry agent" data-entry="a:t1" data-kind="turn"><div class="msg magnus"><div class="small faint">Waiting on you</div></div>' + mine.map(card).join("") + '</div>';
}
function drawPanel() {
  const d = document.getElementById("detail");
  const open = !!Panel.what;
  // As app.js's Workspace does: the canvas and the conversation share the stage.
  d.hidden = !open; document.getElementById("workspace-splitter").hidden = !open;
  document.body.classList.toggle("with-detail", open); document.body.dataset.workspace = open ? "split" : "conversation";
  if (!open) { d.innerHTML = ""; return; }
  const id = Panel.what.slice(5), i = FX.pending.find(x => x.id === id);
  d.innerHTML = AuroraStage.shell({ kind: "item", title: "Decision", backBtn: "", body: i ? card(i) : '<p class="empty">This decision is gone.</p>' });
}
function openPanel(what, o) { FX.opened.push(what + (o && o.auto ? " (auto)" : "")); Panel.what = what; Panel.autoOpened = !!(o && o.auto); drawPanel(); }
function closePanel() { Panel.what = null; Panel.autoOpened = false; drawPanel(); AuroraStage.closed(); }
document.addEventListener("click", e => {
  const b = e.target.closest("button[data-item]");
  if (!b || b.disabled) return;
  b.disabled = true;
  // As app.js's respond(): the answer is told once the server took it, then redrawn.
  setTimeout(() => { document.dispatchEvent(new CustomEvent("magnus:answered", { detail: { item: b.dataset.item, decision: b.dataset.decision.split(":")[0], button: b } })); FX.pending = FX.pending.filter(i => i.id !== b.dataset.item); drawThread(); drawPanel(); toast("Allowed."); }, 60);
});`;

export const ask = (id, thread, sentence) => ({ id, thread, kind: "approval", can_respond: true, summary: sentence, ask: { sentence, offers: ["allow", "dont_allow"] } });

/**
 * @param {string} motion @param {string} [background]
 * @param {Record<string, string>} [files] sheets or scripts used in place of
 *   desktop/ui's, by name, such as an earlier version to film beside this one
 */
export const page = (motion, background = "off", files = {}) => { const ui = (/** @type {string} */ name) => files[name] ?? read(name); return `<!doctype html><html><head><style>${["base.css", "layout.css", "chat.css", "teams.css", "aurora.css", "aurora-widgets.css", "aurora-stage.css"].map(ui).join("\n")}</style></head>
<body data-navigation="expanded" data-workspace="conversation"><script>${ui("theme.js")}</script><script>Theme.set({ style: "aurora", mode: "dark", motion: "${motion}", background: "${background}" });</script>
<div class="shell" style="--nav-width:228px;--panel-width:360px;--splitter-width:16px;--conversation-min-width:560px;--workspace-height:900px;height:900px"><div class="workspace-bar"></div>
  <nav class="side"></nav>
  <main id="main"><section class="conv"><header class="conv-head" id="conv-head"><div class="conv-title"><h1>Plan the trip</h1></div><div class="row"></div></header>
  <div class="thread" id="thread"></div>
  <div id="composer" class="composer-slot"><form class="say" onsubmit="return false"><textarea id="msg"></textarea></form></div></section></main>
  <div id="workspace-splitter" hidden></div><aside class="detail" id="detail" hidden></aside>
</div>
<script>${stubs}</script><script>${ui("asks.js")}</script><script>${ui("notices.js")}</script>
<script>${ui("aurora-boxes.js")}</script><script>${ui("aurora-stage.js")}</script><script>${ui("aurora-reasoning.js")}</script><script>${ui("aurora-alerts.js")}</script></body></html>`; };

