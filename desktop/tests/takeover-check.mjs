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
assert.match(page.els["[data-screen-bar]"].innerHTML, /You have control\. The agent is paused\./);
assert.ok(page.els["[data-done]"], "no I'm done");

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

// Clicking into the preview takes control too. A stream that then fails
// leaves the screenshot showing, with the reason.
const takes = calls.filter(c => c.endsWith("/takeover")).length;
assert.equal(typeof preview.onclick, "function", "clicking into the preview does not take control");
await preview.onclick();
await settle();
assert.equal(calls.filter(c => c.endsWith("/takeover")).length, takes + 1, "clicking into the preview did not take control");
closed("The connection to the screen ended.");
assert.equal(preview.hidden, false, "a failed stream left no screen");
assert.equal(page.els["#display"].hidden, true);
assert.equal(page.els["#display-error"].hidden, false);

// The live display can fill the screen, and the strip offers it throughout.
assert.ok(page.els["[data-full]"], "no Full screen");
await page.els["[data-full]"].onclick();
assert.equal(page.els["[data-screen]"].fullScreens, 1, "Full screen did not fill the screen with the display");
assert.equal(page.els["#preview"], preview, "going full screen replaced the screen");
console.log("takeover ok");
