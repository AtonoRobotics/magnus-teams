// A release's notes show whole sentences, items as a list and no trailers,
// though they arrive as a commit message wrapped short. On 2026-11-26 the
// installed app's What's new card broke each sentence at the wrap and showed
// Co-Authored-By.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ui = join(dirname(fileURLToPath(import.meta.url)), "../ui");
const src = readFileSync(join(ui, "release-notes.js"), "utf8");
const esc = (/** @type {string} */ s) => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const releaseNotesHtml = new Function("esc", `${src}; return releaseNotesHtml;`)(esc);

const notes = `Frame every Nova screen as its mockup does

The frame around each screen never looked like the
mockup, so releases changed details nobody could see.

- One top bar across every screen: where you are, this
  month's spend.
- Titles show no Markdown.

Co-Authored-By: Someone <someone@example.test>`;
assert.equal(releaseNotesHtml(notes),
  "<p>Frame every Nova screen as its mockup does</p>"
  + "<p>The frame around each screen never looked like the mockup, so releases changed details nobody could see.</p>"
  + "<ul><li>One top bar across every screen: where you are, this month&#39;s spend.</li><li>Titles show no Markdown.</li></ul>");
assert.equal(releaseNotesHtml(""), "<p>Fixes and improvements.</p>");
assert.equal(releaseNotesHtml("Signed-off-by: A <a@b.test>"), "<p>Fixes and improvements.</p>");
assert.equal(releaseNotesHtml("<b>x</b>"), "<p>&#60;b&#62;x&#60;/b&#62;</p>");

const app = readFileSync(join(ui, "app.js"), "utf8");
const settings = readFileSync(join(ui, "settings-sections.js"), "utf8");
assert.match(app, /<div class="notes">\$\{releaseNotesHtml\(n\.notes\)\}<\/div>/, "What's new uses the shared notes");
assert.match(settings, /const para = releaseNotesHtml;/, "Settings › Updates uses the shared notes");
assert.match(readFileSync(join(ui, "index.html"), "utf8"), /<script src="release-notes.js"><\/script>/);
console.log("release-notes-check: ok");
