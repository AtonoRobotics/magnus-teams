// A running task's status line changes with every step (spec rev 147). It
// was drawn into the task card's HTML, so each step changed the
// conversation's HTML and the poll redrew the whole thread every few
// seconds (0.2.6-test.109: "a constant refresh of the chat window"). The
// card's HTML stays the same as the line changes, and the line is set in
// place.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const ui = name => readFileSync(new URL(`../ui/${name}`, import.meta.url), "utf8");
const context = { setTimeout, clearTimeout, console, crypto: { randomUUID: () => "u" },
  localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
  document: { addEventListener() {} }, window: { addEventListener() {} }, location: { hash: "" } };
vm.runInNewContext(`${ui("chat-view.js")};\n${ui("app.js")};\n${ui("evidence.js")};\nglobalThis.T = { taskCard, liveStatusLines };`, context);
const { taskCard, liveStatusLines } = context.T;

const task = line => ({ task_id: "t1", title: "Plan the launch", text: "Plan the launch", status: "running", status_line: line });
const first = taskCard(task("Searching the web")), next = taskCard(task("Reading a page"));
assert.equal(first, next, "the card's HTML changed with the status line, so the thread is redrawn at every step");
assert.ok(!first.includes("Reading a page"), "the status line is in the card's HTML");

const slot = { dataset: { statusLine: "t1" }, textContent: "Working on it" };
liveStatusLines({ querySelectorAll: sel => (sel === "[data-status-line]" ? [slot] : []) });
assert.equal(slot.textContent, "Reading a page", "the status line is not shown");

assert.match(taskCard({ ...task(null), status: "suspended", waiting_on: "approval" }), /class="sub why"/, "a waiting task lost its reason");
console.log("status-line-check: ok");
