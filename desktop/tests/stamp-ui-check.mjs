// The release stamps the desktop app's page so an update loads its new
// interface: every stylesheet and script carries the version, and the
// release runs the stamp before it builds the app.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdtempSync } from "node:fs";
import { join, dirname } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const page = readFileSync(join(root, "desktop/ui/index.html"), "utf8");
const dir = mkdtempSync(join(tmpdir(), "stamp-"));
const copy = join(dir, "index.html");
writeFileSync(copy, page);
execFileSync("sh", [join(root, "release/stamp-ui.sh"), "0.7.1-test.9", copy]);
const stamped = readFileSync(copy, "utf8");
const loads = [...stamped.matchAll(/(?:href|src)="([^"]+\.(?:css|js)[^"]*)"/g)].map(m => m[1]);
assert.ok(loads.length >= 10, `the page loads ${loads.length} files`);
for (const url of loads) assert.match(url, /\?v=0\.7\.1-test\.9$/, `${url} is not stamped`);
assert.equal(stamped.replace(/\?v=0\.7\.1-test\.9/g, ""), page, "the stamp changes only the URLs");
assert.throws(() => execFileSync("sh", [join(root, "release/stamp-ui.sh"), "1.0; rm -rf /", copy], { stdio: "ignore" }));

const release = readFileSync(join(root, ".github/workflows/release.yml"), "utf8");
const stamp = release.indexOf('release/stamp-ui.sh "$MAGNUS_VERSION"'), build = release.indexOf("cargo tauri build");
assert.ok(stamp > 0, "the release stamps the page");
assert.ok(stamp < build, "the release stamps the page before it builds the app");
console.log("stamp-ui-check: ok");
