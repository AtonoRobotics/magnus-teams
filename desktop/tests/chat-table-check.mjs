// A table in a chat answer is as tall as its rows' text (owner, 2026-11-27:
// a Priority / Finding row had hundreds of pixels of blank space above and
// below it). Its cells held Markdown links to long addresses: drawn as source,
// the addresses made the table far wider than the conversation, a column off
// to the right set the row's height, and the cells in view sat in its middle.
// The browser check is desktop/tests/chat-table.mjs (scratch test chat_table);
// this one holds the three causes without a browser.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const ctx = { URL, location: { origin: "https://magnus.example" } };
vm.runInNewContext(readFileSync(new URL("../ui/compose.js", import.meta.url), "utf8") + ";globalThis.renderer=Compose;", ctx);
const { renderNative, markdownStyles, documentFor } = /** @type {any} */ (ctx).renderer;

const url = "https://github.com/AtonoRobotics/magnus-teams/blob/4d82f95b9604b988f7b5c711b535e9bc1f945a25/crates/magnus/src/memory/store.rs#L210";
const view = [{ type: "table", columns: ["Priority", "Finding"], rows: [["P1 · Replace stale facts", `The [duplicate check](${url}) uses a 0.85 threshold, see ${url}`], ["Total", 3]] }];

// 1. A link in a cell reads as its words.
const html = renderNative(view);
assert.ok(!html.includes("](https://"), "a link in a cell shows its Markdown source");
assert.match(html, /<a [^>]*>duplicate check<\/a>/, "a link in a cell is not drawn as a link");
assert.match(html, /<td class="num">3<\/td>/, "a number in a cell is still a number");

// 2 and 3. In the app and in a frame: cells start at the top, and a link
// breaks anywhere, so it cannot widen the table past its card.
const rule = (/** @type {string} */ css, /** @type {RegExp} */ sel) => (css.match(new RegExp(`${sel.source}\\s*\\{([^}]*)\\}`)) || [])[1] || "";
assert.match(rule(markdownStyles, /\.ui-native th, \.ui-native td/), /vertical-align:\s*top/, "a native table's cells sit in the middle of a tall row");
assert.match(rule(markdownStyles, /\.ui-native td :is\(a, code\)/), /overflow-wrap:\s*anywhere/, "a long link in a native table's cell cannot break");
const doc = documentFor(view, {}, "");
assert.match(rule(doc, /\n\s*th, td/), /vertical-align:\s*top/, "a framed table's cells sit in the middle of a tall row");
assert.match(rule(doc, /td :is\(a, code, \[data-prose-url\]\)/), /overflow-wrap:\s*anywhere/, "a long link in a framed table's cell cannot break");
console.log("ok - a chat table's rows are as tall as their text");
