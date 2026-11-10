// What the person reads says "server", never "appliance": the appliance was
// rejected and the server took its place (decisions; the Install toast still
// said "the appliance restarts" on 2026-11-10). Comments and identifiers may
// keep the old word; text in the page's scripts may not.
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ui = join(dirname(fileURLToPath(import.meta.url)), "../ui");
const said = [];
for (const file of readdirSync(ui).filter(f => f.endsWith(".js") || f.endsWith(".html"))) {
  readFileSync(join(ui, file), "utf8").split("\n").forEach((line, i) => {
    const code = line.replace(/^\s*(\/\/|\*|\/\*).*$/, "").replace(/\s\/\/\s.*$/, "");
    for (const m of code.matchAll(/\b([Tt]he|[Aa]n|[Yy]our|[Tt]his|[Mm]y|[Oo]ur) appliance\b(?!['’]?s?[.\w])|\bAppliance\b(?![.\w(])/g)) said.push(`${file}:${i + 1}: ${m[0]}`);
  });
}
assert.deepEqual(said, [], `the page says "appliance" where it means the server:\n${said.join("\n")}`);
console.log("wording check passed");
