// Handing back closes what the person opened during a takeover
// (decisions/2026-11-13-takeover-leaves-nothing-running.md), so "I'm done"
// first asks the computer what that would be. A list that is not empty is
// shown on a sheet, and only "Close them and hand back" hands back; an
// empty one hands back at once, as before. computers.js runs against the
// fake page of takeover-check.mjs.
import assert from "node:assert/strict";
import { load, settle } from "./fake-screen.mjs";

let open = [];
const { page, calls, sandbox } = load({
  reply: (method, path) => {
    if (path.endsWith("/screen")) return { image_b64: "LAST", status: "running" };
    if (path.endsWith("/takeover")) return { ticket: "t", protocol_prefix: "p" };
    if (path.endsWith("/handback/closes")) return { programs: open };
    if (path.endsWith("/handback")) return { left_running: { stopped_count: open.length } };
    return {};
  },
});

// The sheet names each program, escaped, and offers both ways.
const sheet = sandbox.handBackSheet(["gedit", "<b>x</b>"]);
assert.match(sheet, /Handing back closes these programs you opened: <b>gedit, &#60;b&#62;x&#60;\/b&#62;<\/b>\. Save your work in them first\./);
assert.match(sheet, /data-ask="close">Close them and hand back</);
assert.match(sheet, /data-ask="keep">Keep control</);
assert.match(sandbox.handBackSheet(null), /could not say what is still open.*Save your work in them first/, "a computer that cannot list them hands back without warning");

const asked = [];
let answer = false;
sandbox.chooseHandBack = async programs => { asked.push(programs); return answer; };
const handbacks = () => calls.filter(c => c === "POST /v1/computers/c/handback").length;

sandbox.computerScreen(page.body, { id: "c", status: "running" }, "/v1/computers/c", "#/computer/c/screen");
await settle();
await page.els["[data-take]"].onclick();
await settle();

// Something open: the sheet, and Keep control keeps it.
open = ["gedit"];
await page.els["[data-done]"].onclick();
await settle();
assert.deepEqual(asked, [["gedit"]], "the sheet did not name what handing back closes");
assert.equal(handbacks(), 0, "Keep control handed back and closed the person's programs");
assert.ok(page.els["[data-done]"], "after Keep control the person is no longer in control");

// Close them and hand back.
answer = true;
await page.els["[data-done]"].onclick();
await settle();
assert.equal(asked.length, 2);
assert.equal(handbacks(), 1, "Close them and hand back did not hand back");

// Nothing open: no sheet, handed back at once.
await page.els["[data-take]"].onclick();
await settle();
open = [];
await page.els["[data-done]"].onclick();
await settle();
assert.equal(asked.length, 2, "a sheet was shown though nothing would be closed");
assert.equal(handbacks(), 2, "a handback that closes nothing was held up");
console.log("handback-sheet-check: ok");
