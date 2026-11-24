// Fast-tier tests for rich text, syntax highlighting, user Markdown, and native
// composable magnus-ui blocks (rich-text.js, compose.js, uispec.js, chat-view.js).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ctx = { URL, location: { origin: "https://magnus.example" }, console };
ctx.globalThis = ctx;
vm.createContext(ctx);
for (const f of ["uispec.js", "rich-text.js", "compose.js"]) {
  vm.runInContext(readFileSync(join(here, "../ui", f), "utf8"), ctx, { filename: f });
}
vm.runInContext("globalThis.__x = { UiSpec, RichText, Compose };", ctx);
const { UiSpec, RichText, Compose } = /** @type {any} */ (ctx.__x);
UiSpec.setCatalog(JSON.parse(readFileSync(join(here, "fixtures/ui-catalog.json"), "utf8")));

// 1. Zero-dependency syntax highlighting escapes raw HTML and highlights tokens.
const jsHi = RichText.highlightCode('const x = "<script>"; // note', "js");
assert.match(jsHi, /<span class="tok-kw">const<\/span>/);
assert.match(jsHi, /<span class="tok-str">&quot;&lt;script&gt;&quot;<\/span>/);
assert.match(jsHi, /<span class="tok-com">\/\/ note<\/span>/);
assert.doesNotMatch(jsHi, /<script>/);

// 2. Upgraded Markdown: nested lists, task checkboxes, strikethrough, callouts,
//    citation pills, sortable tables + Copy CSV, and secret masking.
const md = [
  "~~old~~ **new** [1] with [hidden card number ending 4242]",
  "",
  "- [x] Ship phase 1",
  "  - Nested detail",
  "- [ ] Ship phase 2",
  "",
  "> [!TIP] Key takeaway",
  "> Keep answers visual and compact.",
  "",
  "| Region | ARR |",
  "| --- | --- |",
  "| North | $41.7M |",
].join("\n");
const html = Compose.markdown(md);
assert.match(html, /<del>old<\/del>/, "strikethrough missing");
assert.match(html, /<sup class="cite-pill">1<\/sup>/, "citation pill missing");
assert.match(html, /class="masked-secret"[^]*•••• 4242/, "typed secret not masked inside Markdown");
assert.match(html, /<li class="task-item"><input type="checkbox" disabled checked>/, "checked task item missing");
assert.match(html, /<ul><li>Nested detail<\/li><\/ul><\/li>/, "nested list not preserved");
assert.match(html, /<div class="md-callout tone-tip"><div class="md-callout-title">Key takeaway<\/div>/, "callout missing");
assert.match(html, /data-sort-col="0"/, "sortable table header missing");
assert.match(html, /class="num">\$41\.7M<\/td>/, "numeric table cell not right-aligned");
assert.match(html, /data-table-csv/, "Copy CSV button missing");

// 3. Native composable magnus-ui: KPI strip auto-grouping with delta, callout,
//    sources (safe URL filtering), and interactive chart tooltips.
const spec = [
  'stat label="Q4 ARR" value=41.7 unit=M delta="+18%"',
  'stat label="Churn" value=1.2 unit="%" delta="-0.3%"',
  'callout tone=tip title="Bottom line" text="Growth accelerated in **Q4**."',
  'bar_chart title="By Region" labels=North,South values=24,17.7 unit=M',
  'line_chart title="Trend" labels=Oct,Nov,Dec unit=M',
  '  series 2026 values=34.2,37.9,41.7',
  'sources title="References"',
  '  source title="Board Deck" url="https://docs.example.com/q4"',
  '  source title="Bad Link" url="javascript:alert(1)"',
].join("\n");
const compiled = UiSpec.compile(spec);
const nativeHtml = Compose.renderNative(compiled);
assert.match(nativeHtml, /<div class="ui-native">/, "native wrapper missing");
assert.match(nativeHtml, /<div class="stat-grid">[^]*stat-delta up">\+18%[^]*stat-delta down">-0\.3%/, "consecutive stats did not auto-group into KPI grid with deltas");
assert.match(nativeHtml, /md-callout tone-tip[^]*Bottom line[^]*<strong>Q4<\/strong>/, "callout component failed");
assert.match(nativeHtml, /data-chart-tip="North: 24 M"/, "bar chart tooltip missing");
assert.match(nativeHtml, /data-chart-tip="2026 · Dec: 41\.7 M"/, "line chart tooltip missing");
assert.match(nativeHtml, /class="source-pill" href="https:\/\/docs\.example\.com\/q4"[^]*docs\.example\.com/, "safe source pill missing");
assert.doesNotMatch(nativeHtml, /javascript:alert/, "unsafe URL leaked into sources");

console.log("rich text check passed");
