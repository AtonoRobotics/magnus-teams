// Co-editing a project's file in the app (coedit.js, decisions/2026-11-23-co-editing-shared-files.md):
// - sharing offers "Share a copy" and "Co-edit", and each upload goes where it should:
//   a copy as before, a new file shared to co-edit, a co-edited file's change on the version seen;
// - a co-edited file shows its version, its history, and a check-out for a binary file;
// - a conflicted change shows both versions of each part, and the member's choice is what is saved;
// - an agent's edits show on its owner's Airlock card, and only there;
// - the page loads it, and the server serves it.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const read = f => readFileSync(join(here, "../ui", f), "utf8");
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const calls = [], toasts = [];
let opened = null, press = "land";

/** A dialog that, once shown, presses `press` with the radios it was drawn with. */
const fakeDialog = () => {
  const handlers = {};
  const d = {
    innerHTML: "", returnValue: "", className: "",
    addEventListener: (k, f) => { handlers[k] = f; },
    remove() {},
    close(v) { d.returnValue = v; handlers.close?.(); },
    showModal() {
      opened = d.innerHTML;
      const target = { closest: s => s === "[data-coedit-do]" ? { dataset: { coeditDo: press } } : null };
      handlers.click({ target });
    },
    // Each conflict's "Keep theirs" chosen, as a member deciding would.
    querySelectorAll: () => [...d.innerHTML.matchAll(/value="theirs" data-at="([^"]*)"/g)].map(m => ({ value: "theirs", dataset: { at: m[1].replace(/&#32;/g, " ") } })),
  };
  return d;
};
const ctx = {
  esc, enc: encodeURIComponent, Date,
  toast: m => toasts.push(m),
  document: { createElement: fakeDialog, body: { append() {} } },
  Magnus: { putBytes: async (path, bytes) => { calls.push(["PUT", path, bytes.length]); if (path.includes("base=1")) throw Object.assign(new Error("conflict"), { status: 409 }); } },
  Server: {
    workspaces: {
      coeditChanges: async () => ({ changes: [{ id: "e1", name: "sow.md", base: 1, author: { person: "mo" }, at: "", mission: null, lane: null, diff: [], conflicts: [{ at: "paragraph 2", base: "Price: 40,000.", mine: "Price: <41,000>.", theirs: "Price: 39,000." }] }] }),
      landChange: async (w, id, choices) => { calls.push(["land", w, id, choices]); return { ok: true, version: 3 }; },
      discardChange: async (w, id) => { calls.push(["discard", w, id]); },
      checkOut: async (w, n) => { calls.push(["checkout", w, n]); },
      checkIn: async (w, n) => { calls.push(["checkin", w, n]); },
    },
  },
};
vm.createContext(ctx);
vm.runInContext(read("coedit.js") + "\nthis.Coedit = Coedit;", ctx);
const { Coedit } = /** @type {any} */ (ctx);

// 1. The share choice, and where each upload goes.
assert.match(Coedit.choice(), /<option value="copy">Share a copy<\/option><option value="coedit">Co-edit<\/option>/);
const files = [{ name: "sow.md", coedit: { version: 4, format: "text", checked_out: null, open_changes: 0 } }, { name: "notes.md", coedit: null }];
assert.equal(Coedit.path("w 1", "notes.md", null, "copy", files), "/v1/workspaces/w%201/files/notes.md", "a copy is shared as before");
assert.equal(Coedit.path("w", "plan.md", "p1", "coedit", files), "/v1/workspaces/w/files/plan.md?share=coedit&project=p1");
assert.equal(Coedit.path("w", "sow.md", null, "copy", files), "/v1/workspaces/w/files/sow.md?base=4", "a co-edited file's change is made on the version seen");

// 2. Its line in a files list.
assert.equal(Coedit.badge(files[1], "sam", true), "", "a copy shows nothing new");
const text = Coedit.badge(files[0], "sam", true);
assert.match(text, /Co-edited · v4/);
assert.match(text, /data-coedit-history/);
assert.doesNotMatch(text, /data-coedit-checkout/, "a text file merges: it is not checked out");
const drawing = { name: "a.dwg", coedit: { version: 2, format: "binary", checked_out: null, open_changes: 1 } };
assert.match(Coedit.badge(drawing, "sam", true), /data-coedit-checkout/);
assert.match(Coedit.badge(drawing, "sam", true), /1 change of yours waits/);
assert.doesNotMatch(Coedit.badge(drawing, "sam", false), /data-coedit-checkout/, "a guest checks nothing out");
const held = { name: "a.dwg", coedit: { ...drawing.coedit, checked_out: { person: "ana", until: "2026-11-23T17:00:00Z" } } };
assert.doesNotMatch(Coedit.badge(held, "sam", true), /data-coedit-check(out|in)/, "another's check-out leaves nothing to press");
assert.match(Coedit.badge({ ...held, coedit: { ...held.coedit, checked_out: { person: "sam", until: "2026-11-23T17:00:00Z" } } }, "sam", true), /data-coedit-checkin/);

// 3. A conflicted upload opens both versions; the member's choice is saved.
await Coedit.upload("w", { name: "sow.md", arrayBuffer: async () => new ArrayBuffer(3) }, null, "copy", [{ name: "sow.md", coedit: { version: 1, format: "text" } }]);
assert.ok(opened, "the conflict was not shown");
assert.match(opened, /paragraph 2/);
assert.match(opened, /Price: &#60;41,000&#62;\./, "the member's own version, escaped");
assert.match(opened, /Price: 39,000\./, "the version that landed since");
assert.match(opened, /Keep yours[\s\S]*Keep theirs/);
assert.deepEqual(JSON.parse(JSON.stringify(calls.find(c => c[0] === "land"))), ["land", "w", "e1", { "paragraph 2": "theirs" }]);
assert.ok(toasts.some(t => /version 3/.test(t)), toasts.join("; "));
press = "discard";
await Coedit.resolve("w", "sow.md");
assert.ok(calls.some(c => c[0] === "discard" && c[2] === "e1"), "throwing a change away was not sent");

// 4. Check out and check in, from the list.
const li = name => ({ dataset: { teamFile: name } });
const clickOn = attr => ({ target: { closest: s => s === "[data-team-file]" ? li("a.dwg") : s.includes(attr) ? {} : null } });
assert.equal(await Coedit.click(clickOn("data-coedit-checkout"), "w", true, () => {}), true);
assert.ok(calls.some(c => c[0] === "checkout" && c[2] === "a.dwg"));
assert.equal(await Coedit.click({ target: { closest: s => s === "[data-team-file]" ? li("a.dwg") : null } }, "w", true, () => {}), false, "a plain click is the list's own");

// 5. Coedit.laneEdits escapes agent edits safely.
const edit = { id: "e2", name: "quote.json", base: 1, author: { person: "bob", run: "r1" }, at: "", mission: "m1", lane: "l2", conflicts: [], diff: [{ at: "notes", before: null, after: "<b>net 30</b>" }] };
const bobs = Coedit.laneEdits({ edits: [edit] });
assert.match(bobs, /quote\.json, on version 1/);
assert.match(bobs, /&#60;b&#62;net 30&#60;\/b&#62;/, "an edit is shown escaped");

// 6. The page loads it before the team's scripts, and the server serves it.
const page = read("index.html");
assert.ok(page.indexOf('src="coedit.js"') > 0 && page.indexOf('src="coedit.js"') < page.indexOf('src="team-panel.js"'));
assert.match(readFileSync(join(here, "../../crates/magnus/src/ui/assets.rs"), "utf8"), /"coedit\.js"/);
for (const f of ["team-panel.js", "shared-canvas.js"]) {
  assert.match(read(f), /Coedit\.choice\(\)/, `${f} offers no share choice`);
  assert.match(read(f), /Coedit\.upload\(/, `${f} uploads around co-editing`);
}
console.log("coedit-check: ok");
