// The person's keys reach the computer they control. Production,
// 2026-11-27: "Computer has no keyboard input." The screen took focus only
// when its first frame came; a press on it prevented the browser's own
// focus, so once anything else had focus (the composer, a button) the
// screen never got it back and every key went elsewhere.
import assert from "node:assert/strict";
import { load, settle } from "./fake-screen.mjs";

const { page, stream, sandbox } = load({
  reply: (method, path) => {
    if (path.endsWith("/screen")) return { image_b64: "LAST", status: "running" };
    if (path.endsWith("/takeover")) return { ticket: "t", protocol_prefix: "p" };
    return {};
  },
});
sandbox.computerScreen(page.body, { id: "c", status: "running" }, "#/computer/c/screen");
await settle();
await page.els["[data-take]"].onclick();
await settle();
stream.rfb.onSize(800, 600);
stream.rfb.onRect(0, 0, 1, 1, new Uint8ClampedArray(4));
const canvas = page.els["#display"];
const first = canvas.focused || 0;
assert.ok(first >= 1, "the live screen takes focus when it first draws");

// Focus went elsewhere; a press on the screen brings it back.
canvas.dispatch("mousedown");
assert.ok((canvas.focused || 0) > first, "a press on the screen gives it the keyboard");

const e = canvas.dispatch("keydown", { key: "a" });
assert.deepEqual(stream.keys.at(-1), ["a", true], "a key on the screen reaches the computer");
assert.ok(e.defaultPrevented, "the key does not also reach the page");

// Ctrl+V pastes the person's own clipboard. Sent as keys, the computer
// pasted its own, and a shell took Ctrl+V as "insert the next key as it
// is": Enter came out as ^M (installed app, 2026-11-28).
sandbox.navigator = { clipboard: { readText: async () => "echo pasted" } };
stream.keys.length = 0;
const paste = canvas.dispatch("keydown", { key: "v", ctrlKey: true });
await settle();
assert.ok(paste.defaultPrevented, "Ctrl+V does not also reach the page");
assert.equal(stream.cut, "echo pasted", "the person's clipboard reaches the computer");
assert.deepEqual(stream.keys, [["Control", true], ["v", true], ["v", false], ["Control", false]], "the computer pastes what it was sent, once");
canvas.dispatch("keyup", { key: "v", ctrlKey: true });
assert.equal(stream.keys.length, 4, "the V let go is not sent on its own");
console.log("screen-keyboard-check: ok");
