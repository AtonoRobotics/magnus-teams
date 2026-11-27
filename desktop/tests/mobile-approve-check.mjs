// Regression tests for Tier 3 Phase 2: Out-of-Band Mobile / Async Approval Channel
import assert from "node:assert/strict";
import { readAppBundle } from "./app-bundle.mjs";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";

const ROOT = path.resolve(import.meta.dirname, "..", "..");
const read = rel => fs.readFileSync(path.join(ROOT, rel), "utf8");

// 1. Verify manifest.webmanifest is valid JSON with standalone PWA config
const manifest = JSON.parse(read("desktop/ui/manifest.webmanifest"));
assert.equal(manifest.start_url, "/#/approve");
assert.equal(manifest.display, "standalone");
assert.ok(manifest.name && manifest.short_name);

// 2. Load mobile-approve.js in VM and test pure helpers
const src = read("desktop/ui/mobile-approve.js");
const ctx = vm.createContext({ URLSearchParams, location: { origin: "https://magnus.example", pathname: "/" } });
vm.runInContext(`${src}\nthis.__MobileApprove = MobileApprove;`, ctx);
const MobileApprove = ctx.__MobileApprove;
assert.ok(MobileApprove, "MobileApprove should be defined");

assert.equal(MobileApprove.parseAskId("ask=ask_123"), "ask_123");
assert.equal(MobileApprove.parseAskId("?ask=ask_456&foo=1"), "ask_456");
assert.equal(MobileApprove.parseAskId(""), null);
assert.equal(MobileApprove.approveUrl(), "https://magnus.example/#/approve");
assert.equal(MobileApprove.approveUrl("ask_7"), "https://magnus.example/#/approve?ask=ask_7");

// isReversibleAsk must strictly exclude irreversible and financial asks
assert.equal(MobileApprove.isReversibleAsk({ id: "a1", risk: "reversible" }), true);
assert.equal(MobileApprove.isReversibleAsk({ id: "a2", risk: "irreversible" }), false);
assert.equal(MobileApprove.isReversibleAsk({ id: "a3", risk: "financial" }), false);
assert.equal(MobileApprove.isReversibleAsk({ id: "a4", financial: true }), false);
assert.equal(MobileApprove.isReversibleAsk({ id: "a5", irreversible: true }), false);

// 3. Verify security invariant: reuses asks.js askCard() rather than bespoke unverified rendering
assert.match(src, /askCard\(a,\s*true\)/, "MobileApprove must reuse server-authored askCard()");

// 4. Verify wiring across index.html, assets.rs, app.js, settings-sections.js, and pages.css
assert.match(read("desktop/ui/index.html"), /<link rel="manifest" href="manifest\.webmanifest">/);
assert.match(read("desktop/ui/index.html"), /<script src="mobile-approve\.js"><\/script>/);
assert.match(read("crates/magnus/src/ui/assets.rs"), /manifest\.webmanifest/);
assert.match(read("crates/magnus/src/ui/assets.rs"), /mobile-approve\.js/);
assert.match(readAppBundle(), /route\(\/\^#\\\/approve/);
assert.match(readAppBundle(), /Mobile Approvals/);
assert.match(read("desktop/ui/settings-sections.js"), /data-copy-mobile-approve/);
assert.match(read("desktop/ui/pages.css"), /\.mobile-approve-shell/);

console.log("mobile-approve-check: ok");
