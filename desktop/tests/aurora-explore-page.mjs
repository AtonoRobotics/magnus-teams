// The page aurora-explore.mjs checks and aurora-filmstrip-explore.mjs films:
// the Explore page (discover.js) with the app's theme and sheets and
// aurora-explore.js, and what app.js gives them cut to what they read.
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
export const uiDir = join(here, "../ui");
const ui = name => readFileSync(join(uiDir, name), "utf8");

const stubs = `
var App = { seq: 0, me: { id: "p1" } };
var Resources = { read: async () => [{ name: "gmail", title: "Gmail", state: "connected" }, { name: "slack", title: "Slack", state: "connected" }, { name: "drive", title: "Google Drive", state: "available" }] };
function $(q, r) { return (r || document).querySelector(q); }
function esc(s) { return String(s ?? "").replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]); }
function enc(s) { return encodeURIComponent(s); }
function icon(k) { return '<svg class="ic" data-icon="' + k + '"></svg>'; }
function paint(el, html) { if (el._html === html) return false; el._html = html; el.innerHTML = html; return true; }
function firstName() { return "Sam"; }`;

/**
 * @param {string} motion @param {string} style
 * @param {string} [base] where the page's images are, as a file:// URL
 */
export const page = (motion, style = "aurora", base = "") => `<!doctype html><html><head>${base ? `<base href="${base}">` : ""}<style>${["base.css", "layout.css", "chat.css", "pages.css", "aurora.css", "aurora-widgets.css", "aurora-stage.css"].map(ui).join("\n")}
body { margin: 0; } #main { padding: 28px 32px; height: 100vh; overflow: auto; box-sizing: border-box; }</style></head>
<body><script>${ui("theme.js")}</script><script>Theme.set({ style: "${style}", mode: "dark", motion: "${motion}", background: "off" });</script>
<main id="main"></main>
<script>${stubs}</script><script>${ui("discover.js")}</script><script>${ui("aurora-explore.js")}</script>
<script>Discover.show(document.getElementById("main"));</script></body></html>`;
