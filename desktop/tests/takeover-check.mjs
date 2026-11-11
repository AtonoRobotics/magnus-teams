// Taking control of a computer never empties its screen (spec rev 131: the
// stream never disappears). It went blank on takeover (2026-11-09): the
// preview was thrown away for a display that had drawn nothing yet, and
// stayed empty when the stream was slow or failed. computers.js runs here
// against a small fake page, with the stream stubbed.
import assert from "node:assert/strict";
import { load, settle } from "./fake-screen.mjs";

const { page, calls, stream, sandbox } = load({
  reply: (method, path) => {
    if (path.endsWith("/screen")) return { image_b64: "LAST", status: "running" };
    if (path.endsWith("/takeover")) return { ticket: "t", protocol_prefix: "p" };
    return {};
  },
});
const rfb = new Proxy({}, { get: (_, k) => stream.rfb[k] });
const closed = reason => stream.closed(reason);

sandbox.computerScreen(page.body, { id: "c", status: "running" }, "/v1/computers/c", "#/computer/c/screen");
await settle();
const preview = page.els["#preview"];
assert.equal(preview.src, "data:image/png;base64,LAST", "the preview shows no screenshot");
assert.ok(page.els["[data-take]"], "no Take control");

// Take control, in place.
await page.els["[data-take]"].onclick();
await settle();
assert.ok(calls.includes("POST /v1/computers/c/takeover"), "Take control did not take control");
assert.equal(page.els["#preview"], preview, "taking control threw the screen away");
assert.equal(preview.hidden, false, "the screen disappeared on takeover, before the stream drew anything");
assert.equal(preview.src, "data:image/png;base64,LAST", "the last screenshot is gone");
assert.match(page.els["[data-screen-bar]"].innerHTML, /You have control\. Magnus is paused\./);
assert.ok(page.els["[data-done]"], "no I'm done");
// On the screen itself too, as in the mockup: in control, Magnus paused, I'm done.
assert.equal(page.els["[data-hold]"].hidden, false, "the screen does not say the person is in control");
assert.match(page.els["[data-hold-words]"].textContent, /You're in control\. Magnus is paused\./);
assert.equal(typeof page.els["[data-done-here]"].onclick, "function", "no I'm done on the screen");
assert.match(page.els["[data-screen]"].className, /\bheld\b/);
assert.equal(page.els["[data-screen]"].onclick, null, "a click on the screen being driven would take control again");
assert.match(page.els["[data-caption]"].textContent, /Your mouse and keyboard reach the computer\. Magnus sees nothing until you hand back\./);

// The stream's first frame replaces the preview in the same frame.
rfb.onSize(800, 600);
rfb.onRect(0, 0, 1, 1, new Uint8ClampedArray(4));
assert.equal(page.els["#display"].hidden, false, "the live screen is not shown");
assert.equal(preview.hidden, true);

// I'm done: the last live frame stays as the preview.
await page.els["[data-done]"].onclick();
await settle();
assert.ok(calls.includes("POST /v1/computers/c/handback"));
assert.equal(page.els["#preview"], preview);
assert.equal(preview.hidden, false, "the screen disappeared on handing back");
assert.ok(["data:frame", "data:image/png;base64,LAST"].includes(preview.src), `handing back left no picture of the screen: ${preview.src}`);
assert.equal(page.els["#display"].hidden, true);
assert.ok(page.els["[data-take]"], "Take control is not offered again");

assert.equal(page.els["[data-hold]"].hidden, true, "the screen still says the person is in control after handing back");

// Clicking into the preview takes control too. A stream that then fails
// leaves the screenshot showing, with the reason.
const takes = calls.filter(c => c.endsWith("/takeover")).length;
const frame = page.els["[data-screen]"];
assert.equal(typeof frame.onclick, "function", "clicking into the preview does not take control");
assert.match(page.els["[data-caption]"].textContent, /click the screen to take control/i, "the screen does not say a click takes control");
await frame.onclick();
await settle();
assert.equal(calls.filter(c => c.endsWith("/takeover")).length, takes + 1, "clicking into the preview did not take control");
closed("The connection to the screen ended.");
assert.equal(preview.hidden, false, "a failed stream left no screen");
assert.equal(page.els["#display"].hidden, true);
assert.equal(page.els["#display-error"].hidden, false);

// The live display can fill the screen, and the strip offers it throughout.
// What goes full screen holds the strip too, so who holds control and I'm
// done stay in sight (it was the frame alone, without them).
assert.ok(page.els["[data-full]"], "no Full screen");
await page.els["[data-full]"].onclick();
assert.equal(page.els["[data-view]"].fullScreens, 1, "Full screen did not fill the screen with the display and its strip");
assert.equal(page.els["#preview"], preview, "going full screen replaced the screen");
console.log("takeover ok");
