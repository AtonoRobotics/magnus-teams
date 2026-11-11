// Admin → Spend and Admin → Models (spec rev 141), and the person's "Ask for
// more" where a spent limit stops their work. Wiring is checked in the page's
// files; the sections and the ask are run with the page's helpers stubbed.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = join(here, "../ui");
const read = f => readFileSync(join(ui, f), "utf8");
const page = read("index.html"), app = read("app.js"), spend = read("admin-spend.js");
const fast = readFileSync(join(here, "../../.github/workflows/fast.yml"), "utf8");

// Wiring.
const scripts = [...page.matchAll(/<script src="([^"]+)"><\/script>/g)].map(m => m[1]);
assert.ok(scripts.indexOf("admin-spend.js") > scripts.indexOf("admin.js"), "index.html loads admin-spend.js after admin.js");
assert.match(spend, /registerAdminSection\("spend", el => adminSpend\(el\)\);/, "Admin → Spend is drawn");
assert.match(spend, /registerAdminSection\("models", el => adminModels\(el\)\);/, "Admin → Models is drawn");
assert.match(app, /error\.status===402 && error\.body\?\.spend_limit && typeof offerMoreSpend==="function"\)offerMoreSpend\(error\.body\)/, "a spent limit's refusal offers to ask, wherever it appears");
assert.match(app, /spend_limit: "paused: a monthly spend limit is reached"/, "a run stopped at a limit says so");
assert.match(fast, /node desktop\/tests\/spend-check\.mjs/, "the fast tier runs this check");
assert.ok(!/adminApi\("(POST|PATCH)", "\/v1\/spend/.test(spend), "the person's ask went through the admin guard");

const plain = v => JSON.parse(JSON.stringify(v));

function element(extra = {}) {
  return { html: "", hidden: true, value: "", textContent: "", dataset: {}, listeners: {}, setAttribute() {}, addEventListener(t, f) { this.listeners[t] = f; }, remove() { this.removed = true; }, querySelectorAll: () => [], close() { this.listeners.close?.(); }, showModal() { this.open = true; }, ...extra };
}

function harness(answers = {}) {
  const calls = [], toasts = [], sections = new Map(), els = new Map(), appended = [];
  const ctx = {
    calls, toasts, sections, els, appended, console,
    esc: s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`),
    paint: (box, html) => { box.html = html; },
    toast: m => toasts.push(m),
    registerAdminSection: (n, f) => sections.set(n, f),
    $: (sel, root) => {
      if (sel === "dialog.spend-ask") return appended.find(d => !d.removed && d.className === "spend-ask ask") || appended.find(d => !d.removed) || null;
      const key = `${root ? "in:" : ""}${sel}`;
      if (!els.has(key)) els.set(key, element());
      return els.get(key);
    },
    document: { createElement: () => { const d = element(); return d; }, body: { append: d => appended.push(d) } },
  };
  // FormData reads the form's fields as the test set them.
  ctx.FormData = function (f) { return Object.entries(f.fields || {}); };
  const reply = (m, p, b) => {
    calls.push([m, p, b]);
    const key = `${m} ${p.split("?")[0]}`;
    if (answers[key] instanceof Error) throw answers[key];
    return answers[key] ?? { ok: true };
  };
  ctx.api = async (m, p, b) => reply(m, p, b);
  ctx.adminApi = async (m, p, b) => reply(m, p, b);
  vm.createContext(ctx);
  vm.runInContext(spend, ctx);
  return ctx;
}

// Amounts.
{
  const h = harness();
  assert.equal(h.amountOf(""), null, "an empty field lifts the limit");
  assert.equal(h.amountOf("$12.50"), 12.5);
  assert.throws(() => h.amountOf("-1"), /from 0 up/);
  assert.throws(() => h.amountOf("lots"), /from 0 up/);
  assert.equal(h.dollars(null), "No limit");
  assert.equal(h.dollars(3), "$3.00");
  assert.ok(h.sections.has("spend") && h.sections.has("models"));
}

// Admin → Spend shows the limits, the requests, people and the breakdowns.
{
  const overview = {
    month: "2026-11", limits: { server_usd: 500, default_usd: 20 }, total_usd: 120, server_left_usd: 380,
    by_person: [{ id: "ada", name: "Ada", usd: 20, override_usd: null, limit_usd: 20, extra_usd: null, left_usd: 0 }],
    by_agent: [{ id: "personal", name: "Personal agents", usd: 100 }, { id: "team:w", name: "Team agent · Ops", usd: 20 }],
    by_model: [{ id: "a/costly", name: "a/costly", usd: 120 }], by_project: [{ id: "", name: "No project", usd: 120 }],
    workspaces: [{ id: "w", name: "Ops", limit_usd: 50, spent_usd: 20, channels: [{ id: "c1", name: "general", limit_usd: null, spent_usd: 20 }] }],
    requests: [{ id: "q1", person: "ada", name: "Ada", month: "2026-11", usd: 15, reason: "the <report>", at: "2026-11-10T00:00:00Z", decided: null }],
    alerts: [{ limit: "person:ada", percent: 90 }],
  };
  const h = harness({ "GET /v1/admin/spend": overview });
  const el = element();
  await h.adminSpend(el);
  assert.match(el.html, /<strong>\$120\.00<\/strong> spent in 2026-11, \$380\.00 left of the server's limit/);
  assert.match(el.html, /data-limit='\{&#34;scope&#34;:&#34;server&#34;\}'.*value="500"/s, "the server's limit is shown to change");
  assert.match(el.html, /Ada<\/strong> asks for \$15\.00 more in 2026-11\. “the &#60;report&#62;”/, "a request shows, its words escaped");
  assert.match(el.html, /data-decide="approve">Approve<\/button><button data-decide="decline">Decline/);
  for (const t of ["By person", "By agent", "By model", "By project", "Team agent · Ops", "#general", "Whole workspace", "person:ada reached 90%"]) assert.ok(el.html.includes(t), `Spend lacks ${t}`);
  await h.adminSpend(el, "2026-10");
  assert.deepEqual(h.calls.at(-1), ["GET", "/v1/admin/spend?month=2026-10", undefined], "another month is asked for by name");
}

// Saving a limit posts its scope and amount; an empty one lifts it.
{
  const h = harness({ "GET /v1/admin/spend": { month: "2026-11", limits: {}, total_usd: 0, server_left_usd: null, by_person: [], by_agent: [], by_model: [], by_project: [], workspaces: [], requests: [], alerts: [] } });
  const input = element({ value: "40" });
  const form = element({ dataset: { limit: JSON.stringify({ scope: "person", person: "ada" }) } });
  const el = element({ querySelectorAll: sel => (sel === "form[data-limit]" ? [form] : []) });
  h.$ = (sel, root) => (root === form && sel === "[name=usd]" ? input : element());
  await h.adminSpend(el);
  await form.onsubmit({ preventDefault() {} });
  assert.deepEqual(plain(h.calls.find(c => c[0] === "POST")), ["POST", "/v1/admin/spend/limits", { scope: "person", person: "ada", usd: 40 }]);
  input.value = "";
  h.calls.length = 0;
  await form.onsubmit({ preventDefault() {} });
  assert.deepEqual(h.calls.find(c => c[0] === "POST")[2].usd, null, "an empty amount did not lift the limit");
  assert.match(h.toasts.at(-1), /Limit lifted/);
}

// The person's ask: offered for their own limit, not the server's, and sent.
{
  const h = harness();
  h.offerMoreSpend({ error: "The server has reached its spend limit", spend_limit: { which: "server", can_ask: false, limit_usd: 100 } });
  assert.equal(h.appended.length, 0, "the server's limit offered an ask");
  h.offerMoreSpend({ error: "You have reached your spend limit", spend_limit: { which: "person", can_ask: false, pending: true, limit_usd: 20 } });
  assert.equal(h.appended.length, 0, "a second ask was offered while one waits");
  const form = element({ fields: { usd: "15", reason: "quarterly report" } });
  h.$ = (sel, root) => (sel === "dialog.spend-ask" ? null : sel === "form" ? form : element());
  h.offerMoreSpend({ error: "You have reached your spend limit for this month ($20.00 of $20.00). You can ask an admin for more.", spend_limit: { which: "person", can_ask: true, pending: false, limit_usd: 20 } });
  const d = h.appended[0];
  assert.ok(d?.open, "the ask was not shown");
  assert.match(d.innerHTML, /You have reached your spend limit/);
  await form.onsubmit({ preventDefault() {} });
  assert.deepEqual(plain(h.calls.at(-1)), ["POST", "/v1/spend/requests", { usd: 15, reason: "quarterly report" }]);
  assert.match(h.toasts.at(-1), /You will hear in your inbox/);
}

// A run stopped partway by a spent limit leaves a notice: where the person
// can ask, the notice offers "Ask for more", which opens the same ask.
{
  assert.match(app, /const askMore = typeof spendNoticeAction === "function" \? spendNoticeAction\(d\) : null;\s*if \(askMore\) list\.push\(askMore\);/, "the stopped run's notice does not offer to ask for more");
  assert.match(app, /if \(decision === "ask-more"\) \{ if \(typeof askMoreFromNotice === "function"\) askMoreFromNotice\(btn\); return; \}/, "Ask for more answers the notice instead of opening the ask");
  const h = harness();
  const stop = { which: "person", month: "2026-11", spent_usd: 20, limit_usd: 20, can_ask: true, pending: false };
  const action = h.spendNoticeAction({ reason: "spend_limit", evidence: stop });
  assert.equal(action?.[0], "ask-more");
  assert.equal(action[1], "Ask for more");
  assert.equal(h.spendNoticeAction({ reason: "spend_limit", evidence: { ...stop, can_ask: false, pending: true } }), null, "a second ask was offered while one waits");
  assert.equal(h.spendNoticeAction({ reason: "spend_limit", evidence: { ...stop, which: "server", can_ask: false } }), null, "the server's limit offered an ask");
  assert.equal(h.spendNoticeAction({ reason: "budget", evidence: stop }), null);
  const form = element({ fields: { usd: "10", reason: "" } });
  h.$ = (sel) => (sel === "dialog.spend-ask" ? null : sel === "form" ? form : element());
  h.askMoreFromNotice({ dataset: { spend: JSON.stringify(stop) } });
  assert.match(h.appended[0]?.innerHTML || "", /You have reached your spend limit for this month \(\$20\.00 of \$20\.00\)/, "Ask for more did not open the ask");
  await form.onsubmit({ preventDefault() {} });
  assert.deepEqual(plain(h.calls.at(-1)), ["POST", "/v1/spend/requests", { usd: 10, reason: "" }]);
}

// Admin → Models: providers, the default, and a rule per role and person.
{
  const h = harness({
    "GET /v1/admin/models": { default: { adapter: "a", model: "cheap" }, access: { roles: { member: ["a/cheap"] }, people: {} }, roles: ["admin", "member"], people: [{ id: "sam", name: "Sam", owner: true }, { id: "ada", name: "Ada", owner: false }] },
    "GET /v1/admin/providers": { providers: [{ id: "a", enabled: true, default: true, answers: true, models: ["cheap", "costly"] }] },
  });
  const el = element();
  await h.adminModels(el);
  assert.match(el.html, /New work runs on <strong>a\/cheap<\/strong>/);
  assert.match(el.html, /<li>cheap <span class="chip">Default<\/span>/);
  assert.match(el.html, /data-default="costly">Make default/);
  assert.match(el.html, /Role: member.*value="a\/cheap" checked/s, "the member rule shows its model");
  assert.match(el.html, /<summary>Ada<\/summary>/);
  assert.ok(!/<summary>Sam<\/summary>/.test(el.html), "the owner was offered a rule");
}

console.log("spend check passed");
