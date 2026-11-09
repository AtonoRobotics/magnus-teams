// The client's spec compiler against the same cases as the appliance's.
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import assert from "node:assert";
const here = new URL(".", import.meta.url).pathname;
const UiSpec = createRequire(import.meta.url)(`${here}../ui/uispec.js`);
UiSpec.setCatalog(JSON.parse(readFileSync(`${here}fixtures/ui-catalog.json`, "utf8")));

for (const c of JSON.parse(readFileSync(`${here}fixtures/ui-spec.json`, "utf8"))) {
  if (c.error) assert.throws(() => UiSpec.compile(c.spec), e => e.message.includes(c.error), c.spec);
  else assert.deepStrictEqual(UiSpec.compile(c.spec), c.components, c.spec);
}

// As text streams only finished lines are drawn, and a bad line waits.
assert.deepStrictEqual(UiSpec.compilePartial("stat label=a value=1\nstat lab"), [{ type: "stat", label: "a", value: 1 }]);
assert.deepStrictEqual(UiSpec.compilePartial("stat lab"), []);
const parts = UiSpec.split("before\n```magnus-ui id=q\nstat label=a value=1\n```\nafter\n```magnus-html\n<p>");
assert.deepStrictEqual(parts.map(p => [p.kind, p.closed ?? null, p.id ?? null]), [["text", null, null], ["ui", true, "q"], ["text", null, null], ["html", false, null]]);
console.log("ui spec ok");
