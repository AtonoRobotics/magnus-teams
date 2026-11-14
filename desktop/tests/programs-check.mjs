// Programs on this computer, in Apps (connectors spec, Local desktop
// connectors): a program found here and not yet allowed asks "Use <program>
// on this computer" with Allow and Not now; Allow sends the consent for this
// computer and asks the app to report again; Not now leaves it unallowed;
// an allowed one shows where it stands and can be stopped; other computers
// are listed by name, each with its own set; names are escaped.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const calls = [];
let refreshed = 0, confirm = true;
const sandbox = {
  esc, enc: encodeURIComponent, toast: () => {},
  ask: async () => confirm,
  api: async (method, path, body) => { calls.push([method, path, body]); return {}; },
  Magnus: { desktopDevice: async () => ({ id: "desk-1", name: "Mo's desk" }), desktopRefresh: async () => { refreshed++; } },
};
sandbox.globalThis = sandbox;
vm.runInNewContext(ui("programs.js"), sandbox);

const program = (id, title, state, allowed) => ({ program: id, title, state, allowed, version: state === "not_installed" ? null : "4.2" });
const list = [
  { id: "desk-1", name: "Mo's desk", online: true, programs: [program("blender", "Blender", "not_enabled", false), program("freecad", "FreeCAD", "ready", true), program("fusion", "Autodesk Fusion", "not_installed", false)] },
  { id: "lap-1", name: "Mo's <b>laptop</b>", online: false, programs: [program("blender", "Blender", "closed", true), program("freecad", "FreeCAD", "not_installed", false)] },
];

const html = sandbox.programsSection({ id: "desk-1" }, list, new Set());
assert.match(html, /data-section="programs-here"[\s\S]*On this computer/, "this computer's programs are not shown");
assert.match(html, /data-program="blender" data-state="consent"[\s\S]*Use Blender on this computer[\s\S]*data-act="program-allow"[\s\S]*>Allow<[\s\S]*data-act="program-later"[\s\S]*>Not now</, "the consent is not Use Blender on this computer, Allow or Not now");
assert.match(html, /data-program="freecad" data-state="ready"[\s\S]*Stop using FreeCAD here/, "an allowed program does not say where it stands with a way to stop");
assert.ok(!/data-program="fusion"/.test(html.split("programs-elsewhere")[0]), "a program not installed here was offered");
assert.match(html, /data-section="programs-elsewhere"[\s\S]*data-device="lap-1"[\s\S]*Blender: Closed/, "another computer is not listed by name with its own set");
assert.ok(!html.includes("<b>laptop</b>") && html.includes("Mo&#39;s &#60;b&#62;laptop&#60;/b&#62;"), "a computer's name was not escaped");
assert.match(html, /not connected now/);

// Not now: the card steps back to a plain way to allow it later, and nothing is sent.
const later = sandbox.programsSection({ id: "desk-1" }, list, new Set(["blender"]));
assert.match(later, /data-program="blender" data-state="not_enabled"[\s\S]*Use Blender on this computer/);
assert.ok(!/data-state="consent"/.test(later));

// In a browser there is no "this computer": every computer is listed, and nothing can be allowed.
const web = sandbox.programsSection(null, list, new Set());
assert.ok(!web.includes("programs-here") && !web.includes("program-allow"), "a browser offered to allow a program");
assert.match(web, /On your computers/);
assert.equal(sandbox.programsSection({ id: "desk-1" }, null, new Set()), "", "programs drew with no list");

await sandbox.loadPrograms();
calls.length = 0;
await sandbox.programAct("program-later", "blender");
assert.equal(calls.length, 0, "Not now sent a consent");
await sandbox.programAct("program-allow", "blender");
assert.equal(JSON.stringify(calls.pop()), JSON.stringify(["PUT", "/v1/desktop/devices/desk-1/programs/blender", { allowed: true }]), "Allow did not send this computer's consent");
assert.equal(refreshed, 1, "Allow did not ask the app to report again");
confirm = false;
await sandbox.programAct("program-stop", "blender", "lap-1");
assert.equal(calls.length, 0, "Stop went through though the person said no");
confirm = true;
await sandbox.programAct("program-stop", "blender", "lap-1");
assert.equal(JSON.stringify(calls.pop()), JSON.stringify(["PUT", "/v1/desktop/devices/lap-1/programs/blender", { allowed: false }]), "Stop did not take the consent back on that computer");

assert.match(ui("index.html"), /<script src="programs\.js"><\/script>/, "the page does not load programs.js");
assert.match(ui("app.js"), /id="lib-programs"[\s\S]*mountPrograms\(main\)/, "Apps does not draw the programs");
assert.match(ui("bridge.js"), /desktopDevice\(\) \{ return this\.invoke\("desktop_device"\)/, "the app does not name this computer to the page");
console.log("programs-check: ok");
