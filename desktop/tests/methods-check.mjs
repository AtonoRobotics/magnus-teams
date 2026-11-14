// The installed app sends the page's requests itself (desktop/src-tauri,
// `api`), and refuses a method it does not know. It did not know PATCH, so
// changing a role, a connector or a provider worked in a browser and failed
// in the app. Every method the page sends is one the app sends.
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const ui = join(here, "../ui");
const rust = readFileSync(join(here, "../src-tauri/src/methods.rs"), "utf8");
const list = rust.match(/pub\(crate\) const METHODS: \[&str; \d+\] = \[([^\]]*)\];/);
assert.ok(list, "methods.rs lists the methods the app sends");
const sent = new Set([...list[1].matchAll(/"([A-Z]+)"/g)].map(m => m[1]));
for (const m of sent) assert.ok(rust.includes(`"${m}" => Ok(reqwest::Method::${m})`), `methods.rs sends ${m} as itself`);

/** Each method a script names as the first argument of a request. */
const used = new Map();
for (const file of readdirSync(ui).filter(f => f.endsWith(".js"))) {
  for (const m of readFileSync(join(ui, file), "utf8").matchAll(/\b(?:api|adminApi|call|request)\(\s*["'`]([A-Za-z]+)["'`]/g)) {
    used.set(m[1], [...(used.get(m[1]) || []), file]);
  }
}
assert.ok(used.has("GET") && used.has("POST"), `the scan finds the page's requests: ${[...used.keys()]}`);
const missing = [...used].filter(([m]) => !sent.has(m)).map(([m, files]) => `${m} (${[...new Set(files)].join(", ")})`);
assert.deepEqual(missing, [], `the page sends methods the app refuses: ${missing.join("; ")}`);
console.log(`methods check passed: ${[...used.keys()].sort().join(", ")} all sent by the app`);
