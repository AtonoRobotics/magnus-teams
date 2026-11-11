// The computer wakes on any use and its panel always says who holds control
// (spec rev 131: Lifecycle; User takeover and handback). Opening a sleeping
// computer wakes it with no Start button, its last screenshot showing with
// the wake's progress; the strip on its page and the pinned panel says the
// same thing, with Take control or I'm done, and Full screen.
import assert from "node:assert/strict";
import { load, settle, tick } from "./fake-screen.mjs";

const driving = (by, extra = {}) => ({ by, person: null, runs: by === "agent" ? [{ run: "r1", task: "t1", status: "running" }] : [], request: null, ...extra });
let computer = { id: "c", status: "hibernated", controller: null, driving: driving("no_one"), waking: null };
let shot = { image_b64: "SLEPT", status: "hibernated", waking: null, at: "T", last: true };
const { page, calls, stream, sandbox } = load({
  reply: (method, path) => {
    if (path === "/v1/computers/c") return computer;
    if (path.endsWith("/screen")) return shot;
    if (path.endsWith("/takeover")) return { ticket: "t", protocol_prefix: "p" };
    return {};
  },
});
sandbox.paint = (el, html) => { el.innerHTML = html; };
const caption = () => page.els["[data-caption]"];

// Asleep: opening it wakes it; there is nothing to start by hand.
await sandbox.showComputer(page.body, "c", "screen");
await settle();
assert.ok(calls.includes("POST /v1/computers/c/wake"), "opening a sleeping computer did not wake it");
assert.doesNotMatch(page.body.innerHTML, /data-act="start"|>Start</, "a sleeping computer offers a Start button");
const preview = page.els["#preview"];
assert.equal(preview.src, "data:image/png;base64,SLEPT", "the last screenshot is not shown while it sleeps");
assert.equal(preview.hidden, false);
assert.equal(caption().hidden, false);
assert.match(caption().textContent, /Asleep\. It wakes when Magnus needs it\. Click to wake it now\./);
assert.match(page.els["[data-screen]"].className, /asleep/);
assert.match(page.els["[data-screen-bar]"].innerHTML, /data-pill="asleep"[^>]*>Asleep</);
assert.match(page.els["[data-screen-bar]"].innerHTML, /No one has control\. Magnus is not paused\./);
assert.equal(page.els["[data-veil]"].hidden, false, "a sleeping screen does not say it sleeps");
// A click on a sleeping screen wakes it, as in the mockup; it takes nothing.
const wakes = () => calls.filter(c => c === "POST /v1/computers/c/wake").length, woke = wakes();
await page.els["[data-screen]"].onclick();
assert.equal(wakes(), woke + 1, "a click on the sleeping screen did not wake it");
assert.ok(!calls.includes("POST /v1/computers/c/takeover"), "a click on a sleeping screen took control");
assert.ok(page.els["[data-take]"], "no Take control while it sleeps");
assert.ok(page.els["[data-full]"], "no Full screen");

// Waking: the last screenshot stays, with how far the wake has got.
computer = { ...computer, status: "running", waking: { phase: "restoring", since: "T" } };
shot = { ...shot, status: "running", waking: { phase: "restoring", since: "T" }, image_b64: "SLEPT" };
await tick(stream);
assert.equal(page.els["#preview"], preview);
assert.equal(preview.src, "data:image/png;base64,SLEPT", "the screen emptied while it woke");
assert.equal(caption().hidden, false);
assert.match(caption().textContent, /Waking: restoring your apps and files/);
assert.match(page.els["[data-veil]"].innerHTML, /role="progressbar"[^>]*aria-valuenow="80"/, "the wake shows no progress");
assert.match(page.els["[data-screen-bar]"].innerHTML, /data-pill="waking"[^>]*>Waking</);

// Awake, with the agent at work: the screen is live and the strip says so.
computer = { ...computer, waking: null, driving: driving("agent") };
shot = { image_b64: "NOW", status: "running" };
await tick(stream);
assert.equal(preview.src, "data:image/png;base64,NOW");
assert.doesNotMatch(caption().textContent, /Waking/, "a live screen still says it is waking");
assert.match(caption().textContent, /click the screen to take control/, "the screen does not say a click takes control");
assert.equal(page.els["[data-veil]"].hidden, true, "an awake screen is still veiled");
assert.match(page.els["[data-screen-bar]"].innerHTML, /data-pill="working"[^>]*>Magnus is working</);
assert.match(page.els["[data-screen-bar]"].innerHTML, /Magnus has control\. It is working/);
// Opened asleep, the page offers what a running computer does once it is
// (it kept offering only what an asleep one does until a reload). Who holds
// it is said once, on the strip, not again in the head.
assert.match(page.els["[data-head]"].innerHTML, /data-act="hibernate"/, "a running computer's page does not offer to put it to sleep");
assert.doesNotMatch(page.els["[data-head]"].innerHTML, /state-pill|data-driving/, "the page's head says the state a second time");
const pageStrip = page.els["[data-screen-bar]"].innerHTML;

// Another person holds it, then the agent asks for a takeover.
const words = c => sandbox.controlWords(c, false).text;
assert.equal(words({ ...computer, controller: "bob" }), "bob has control. Magnus is paused.");
assert.equal(words({ ...computer, controller: "alice" }), "You have control. Magnus is paused.");
assert.match(words({ ...computer, driving: driving("no_one", { request: { item: "i1" } }) }), /Magnus is paused: it asks you to take over/);
assert.match(words({ ...computer, status: "paused" }), /paused by the kill switch/);
assert.match(sandbox.controlStrip({ ...computer, controller: "alice" }, false), /data-done/, "the holder is not offered I'm done");
assert.doesNotMatch(sandbox.controlStrip({ ...computer, status: "paused" }, false), /data-take/, "the kill switch's pause offered Take control");

// The pinned panel: the same strip for the same computer, and taking control
// happens in it, without leaving the conversation.
page.fresh();
sandbox.Panel.what = "computer:c";
sandbox.location.hash = "#/task/t1";
const { body, after } = await sandbox.PANELS.computer("c");
page.body.innerHTML = body;
after(page.body);
await settle();
await tick(stream);
assert.equal(page.els["[data-screen-bar]"].innerHTML, pageStrip, "the pinned panel and the computer's page disagree on who holds control");
const pinnedPreview = page.els["#preview"];
assert.equal(pinnedPreview.src, "data:image/png;base64,NOW");
const takes = () => calls.filter(c => c === "POST /v1/computers/c/takeover").length;
await page.els["[data-take]"].onclick();
await settle();
assert.equal(takes(), 1, "Take control on the panel did not take control");
assert.equal(sandbox.location.hash, "#/task/t1", "taking control on the panel left the conversation");
assert.equal(page.els["#preview"], pinnedPreview, "taking control on the panel replaced its screen");
assert.match(page.els["[data-screen-bar]"].innerHTML, /You have control\. Magnus is paused\./);
assert.ok(page.els["[data-done]"], "no I'm done on the panel");
await page.els["[data-full]"].onclick();
assert.equal(page.els["[data-view]"].fullScreens, 1, "the panel's display cannot go full screen");
await page.els["[data-done]"].onclick();
await settle();
assert.ok(calls.includes("POST /v1/computers/c/handback"), "I'm done on the panel did not hand back");
assert.ok(page.els["[data-take]"]);

// A card's Take control with the panel open takes it in the panel.
const [, onClick] = stream.listeners.find(([type, f]) => type === "click" && String(f).includes("data-take-over"));
onClick({ target: { closest: sel => sel === "[data-take-over]" ? { dataset: { takeOver: "c" } } : null }, preventDefault() {} });
await settle();
assert.equal(takes(), 2, "a card's Take control did not take control in the pinned panel");

// Paused by the kill switch: no use wakes it, and Resume is offered.
page.fresh();
calls.length = 0;
sandbox.location.hash = "#/computer/c/screen";
computer = { id: "c", status: "paused", controller: null, driving: driving("no_one"), waking: null };
shot = { image_b64: null, status: "paused", waking: null, at: null, last: true };
await sandbox.showComputer(page.body, "c", "screen");
await settle();
assert.ok(!calls.includes("POST /v1/computers/c/wake"), "opening a computer paused by the kill switch woke it");
assert.match(page.body.innerHTML, /data-act="start">Resume</);
assert.match(caption().textContent, /Paused by the kill switch/);
assert.equal(page.els["[data-take]"], undefined);
console.log("computer panel ok");
