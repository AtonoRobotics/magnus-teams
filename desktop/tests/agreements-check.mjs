// The agreements (owner, 2026-11-11): the Terms of Use accepted before the
// app opens, with signing out the only other way; the usage-data question
// with two answers of equal weight and neither chosen beforehand; the
// tester program joined only by accepting the Tester Terms; and Admin ›
// People showing who is a tester.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = join(here, "../ui");
const root = join(here, "../..");
const read = f => readFileSync(join(ui, f), "utf8");
const same = (a, b) => assert.equal(JSON.stringify(a), JSON.stringify(b));

// Wired: loaded by the page, served by the server, run by the fast tier.
const page = read("index.html");
const assets = readFileSync(join(root, "crates/magnus/src/ui/assets.rs"), "utf8");
const fast = readFileSync(join(root, ".github/workflows/fast.yml"), "utf8");
for (const f of ["agreements.js", "admin-testers.js"]) {
  assert.ok(page.includes(`<script src="${f}"></script>`), `index.html loads ${f}`);
  assert.ok(assets.includes(`include_bytes!("../../../../desktop/ui/${f}")`), `the server serves ${f}`);
}
assert.ok(fast.includes("node desktop/tests/agreements-check.mjs"), "the fast tier runs this check");
const app = read("app.js");
assert.match(app, /App\.me = await api\("GET", "\/v1\/me"\);\s*\/\/[^\n]*\n\s*if \(!\(await agreementsGate\(\)\)\)/, "boot does not ask for the agreements before the app opens");
assert.ok(app.includes(`["agreements", "Agreements", (el) => agreementsSection(el)]`), "Settings has no Agreements section");
assert.ok(read("admin-people.js").includes("adminTesters("), "Admin › People does not show the testers");

const src = read("agreements.js");
assert.ok(!/primary|checked|selected/.test(src), "an answer is styled or chosen beforehand");

const doc = (id, title) => ({ id, title, version: `${id}-v1`, path: `/legal/${id}`, sections: [{ heading: "H", paragraphs: ["P"] }], attribution: "Adapted from the Basecamp policies" });
const standing = over => ({
  terms_due: false, usage_due: false, usage: null, tester: null, invited_by: null, contact: "magnus@example.com",
  documents: { terms: doc("terms", "Terms of Use"), privacy: doc("privacy", "Privacy notice"), testers: doc("testers", "Tester Terms") },
  usage_prompt: { version: "u1", title: "Help improve Magnus", text: ["Share?"], yes: "Share anonymous usage data", no: "Don't share" },
  ...over,
});

function load(states, answers) {
  const calls = [], asked = [], toasts = [];
  let el = { html: "" };
  const ctx = vm.createContext({
    console,
    esc: s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]),
    $: () => null,
    paint: (e, h) => { e.html = h; },
    toast: t => toasts.push(t),
    ask: async () => true,
    api: async (m, p, b) => {
      calls.push([m, p, b]);
      if (m === "GET") return states.length > 1 ? states.shift() : states[0];
      return states[0];
    },
    document: {},
  });
  vm.runInContext(src, ctx);
  ctx.agreementAsk = async (label, body, choices, escape) => { asked.push({ label, body, choices, escape }); return answers.shift(); };
  vm.runInContext("agreementAsk = globalThis.agreementAsk;", ctx);
  return { ctx, calls, asked, toasts, el };
}

// Someone new: the terms, then the question.
{
  const t = load([standing({ terms_due: true, usage_due: true }), standing({ usage_due: true }), standing({})], ["accept", "no"]);
  assert.equal(await t.ctx.agreementsGate(), true);
  assert.equal(t.asked[0].label, "Terms of Use");
  assert.equal(t.asked[0].escape, undefined, "the terms could be dismissed without an answer");
  assert.ok(t.asked[0].body.includes("Privacy notice") && t.asked[0].body.includes("Basecamp"), "the Privacy notice or attribution is not shown with the terms");
  same(t.calls.find(c => c[1] === "/v1/legal/terms")?.[2], { terms: "terms-v1", privacy: "privacy-v1" });
  const q = t.asked[1];
  same(q.choices.map(c => c[1]), ["Share anonymous usage data", "Don't share"]);
  same(t.calls.find(c => c[1] === "/v1/legal/usage")?.[2], { share: false, version: "u1" });
}

// Declining the terms signs out and records nothing.
{
  const t = load([standing({ terms_due: true, usage_due: true })], ["decline"]);
  assert.equal(await t.ctx.agreementsGate(), false);
  assert.ok(!t.calls.some(c => c[0] === "POST"), "declining recorded an answer");
}

// Putting the question off records nothing and opens the app.
{
  const t = load([standing({ usage_due: true })], ["later"]);
  assert.equal(await t.ctx.agreementsGate(), true);
  assert.ok(!t.calls.some(c => c[0] === "POST"), "putting it off recorded an answer");
}

// An invitation is answered by the person: join is accepting the terms.
{
  const t = load([standing({ invited_by: "Sam" }), standing({})], ["join"]);
  await t.ctx.agreementsGate();
  assert.ok(t.asked[0].body.includes("Sam invited you") && t.asked[0].body.includes("Tester Terms"));
  same(t.calls.find(c => c[1] === "/v1/legal/testers/join")?.[2], { version: "testers-v1" });
}

// Settings: both answers alike, the current one marked, and a change sent.
{
  const t = load([standing({ usage: { share: true, current: true } })], []);
  const el = { html: "" };
  await t.ctx.agreementsSection(el);
  assert.match(el.html, /data-agreement-usage-set="yes" aria-pressed="true"/);
  assert.match(el.html, /data-agreement-tester="join">Accept the Tester Terms and join/);
  const target = { closest: sel => (sel === "[data-agreement-usage-set]" ? { getAttribute: () => "no" } : null) };
  await el.onclick({ target });
  same(t.calls.find(c => c[1] === "/v1/legal/usage")?.[2], { share: false, version: "u1" });
}

// Admin › People › Testers.
{
  const ctx = vm.createContext({ esc: s => String(s ?? ""), console });
  vm.runInContext(read("admin-testers.js"), ctx);
  const html = ctx.adminTestersHtml([{ person: "ann", version: "v", current: true }], [{ person: "bo", by: "sam" }], [{ id: "ann", name: "Ann" }, { id: "bo", name: "Bo" }, { id: "sam", name: "Sam" }, { id: "cy", name: "Cy" }]);
  assert.match(html, /data-tester="ann"><td>Ann<\/td><td><span class="chip ok">Tester/);
  assert.match(html, /Invited<\/span> <span class="small muted">by Sam/);
  assert.match(html, /<option value="cy">Cy<\/option>/);
  assert.ok(!/<option value="ann">/.test(html), "a tester is offered an invitation");
}

console.log("agreements check passed");
