// A person's own password (owner, 2026-11-10: "There should also be a forgot
// password and change password under settings"). Settings → Account has a
// Password card: current, new and new again, checked against the server's
// minimum before anything is sent, and sent to /v1/account/password. The
// sign-in screen, in the app and in a browser, has "Forgot password?", which
// asks for the email address and says the same thing whatever the server
// knows about it.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = join(here, "../ui");
const read = f => readFileSync(join(ui, f), "utf8");
const page = read("index.html"), app = read("app.js"), bridge = read("bridge.js"), pw = read("password.js");
const tauri = readFileSync(join(here, "../src-tauri/src/lib.rs"), "utf8");

// Wiring.
const scripts = [...page.matchAll(/<script src="([^"]+)"><\/script>/g)].map(m => m[1]);
assert.ok(scripts.includes("password.js") && scripts.indexOf("password.js") > scripts.indexOf("app.js"), "index.html loads password.js after app.js");
assert.match(app, /<div data-password><\/div>/, "Settings → Account has a place for the Password card");
assert.match(app, /await passwordCard\(pw\)/, "Settings → Account draws the Password card");
assert.match(app, /id="forgot">Forgot password\?<\/button>/, "the sign-in screen offers Forgot password?");
assert.match(app, /stage === "forgot"\) \{\s*forgotPasswordStep\(box, data,/, "Forgot password? opens its own step");
assert.match(bridge, /forgotPassword\(email\) \{ return this\.invoke\("forgot_password", \{ email \}\); \}/, "the app asks through its own connection");
assert.match(bridge, /forgotPassword\(email\) \{ return this\.api\("POST", "\/v1\/password-reset\/request", \{ email \}\); \}/, "a browser asks the server directly");
assert.match(tauri, /account::forgot_password/, "the app registers its forgot_password command");

// Behaviour, with the page's helpers stubbed.
function harness() {
  const els = new Map();
  const el = sel => {
    if (!els.has(sel)) els.set(sel, { value: "", hidden: true, disabled: false, textContent: "", style: {}, reset() { for (const [k, e] of els) if (k.startsWith("#pw-")) e.value = ""; } });
    return els.get(sel);
  };
  const calls = [], toasts = [];
  const ctx = {
    els, calls, toasts, location: { origin: "https://teams.example" },
    $: (sel) => el(sel),
    esc: s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`),
    paint: (box, html) => { box.html = html; },
    toast: m => toasts.push(m),
    api: async (m, p, b) => { calls.push([m, p, b]); return m === "GET" ? { here: true, min_length: 12, provider: null } : { ok: true, signed_out: 2 }; },
    Magnus: { forgotPassword: async email => { calls.push(["forgot", email]); return { ok: true }; } },
  };
  vm.createContext(ctx);
  vm.runInContext(pw, ctx);
  return ctx;
}

{
  const h = harness();
  const card = { set innerHTML(v) { this.html = v; }, get innerHTML() { return this.html; } };
  await h.passwordCard(card);
  assert.match(card.html, /autocomplete="current-password"/, "the card asks for the current password");
  assert.equal((card.html.match(/autocomplete="new-password"/g) || []).length, 2, "the card asks for the new password twice");
  assert.match(card.html, /At least 12 characters/, "the card says the server's minimum");
  const form = h.$("[data-password-form]"), note = h.$("[data-password-note]");
  const submit = async (current, next, again) => {
    h.$("#pw-current").value = current; h.$("#pw-new").value = next; h.$("#pw-again").value = again;
    h.calls.length = 0;
    await form.onsubmit({ preventDefault() {} });
  };
  await submit("old-password-1", "short", "short");
  assert.equal(h.calls.length, 0, "a short new password was sent");
  assert.match(note.textContent, /at least 12 characters/);
  await submit("old-password-1", "a-new-password-1", "a-new-password-2");
  assert.equal(h.calls.length, 0, "two different new passwords were sent");
  assert.match(note.textContent, /do not match/);
  await submit("old-password-1", "a-new-password-1", "a-new-password-1");
  assert.deepEqual(JSON.parse(JSON.stringify(h.calls)), [["POST", "/v1/account/password", { current_password: "old-password-1", new_password: "a-new-password-1" }]]);
  assert.match(h.toasts.at(-1), /Your password is changed\. 2 other devices were signed out\./);
}

{
  // The step says the same whatever answer comes back.
  const said = [];
  for (const answer of [{ ok: true }, { ok: true, anything: "else" }]) {
    const h = harness();
    h.Magnus.forgotPassword = async () => answer;
    const box = {};
    h.forgotPasswordStep(box, { url: "https://teams.example" }, () => {});
    assert.match(box.html, /type="email"/, "Forgot password? asks for the email address");
    h.$("#forgot-email").value = "someone@example.com";
    await h.$("#forgot").onsubmit({ preventDefault() {} });
    said.push(box.html);
  }
  assert.equal(said[0], said[1], "the answer to Forgot password? depends on what the server said");
  assert.match(said[0], /If someone@example\.com belongs to someone here/);
  assert.doesNotMatch(pw, /const \w+ = await Magnus\.forgotPassword/, "the step reads the server's answer");
}

console.log("password check passed");
