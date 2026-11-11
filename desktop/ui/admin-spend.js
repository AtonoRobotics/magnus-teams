// Admin → Spend and Admin → Models (spec rev 141), and the person's side of
// a spend limit: where a refusal for a spent limit appears, the app offers
// "Ask for more", and an admin approves or declines in Admin → Spend.
// Routes: GET /v1/admin/spend?month=, POST /v1/admin/spend/limits,
// POST /v1/admin/spend/requests/{id}, GET /v1/admin/models,
// POST /v1/admin/models/access, GET /v1/admin/providers,
// PATCH /v1/admin/providers/{id}, POST /v1/admin/providers/{id}/test;
// for the person, GET /v1/spend and POST /v1/spend/requests.
"use strict";

/**
 * @typedef {{id: string, name: string, usd: number, override_usd?: ?number, limit_usd?: ?number, extra_usd?: ?number, left_usd?: ?number}} SpendRow
 * @typedef {{id: string, person: string, name: string, month: string, usd: number, reason: string, at: string, decided: ?{approved: boolean, usd: number, by: string, note: string, at: string}}} SpendRequest
 * @typedef {{id: string, name: string, limit_usd: ?number, spent_usd: number, channels: {id: string, name: string, limit_usd: ?number, spent_usd: number}[]}} TeamLimits
 * @typedef {{month: string, limits: {server_usd: ?number, default_usd: ?number}, total_usd: number, server_left_usd: ?number, by_person: SpendRow[], by_agent: SpendRow[], by_model: SpendRow[], by_project: SpendRow[], workspaces: TeamLimits[], requests: SpendRequest[], alerts: {limit: string, percent: number}[]}} SpendOverview
 * @typedef {{which: "person"|"server", month: string, spent_usd: number, limit_usd: number, can_ask: boolean, pending: boolean}} SpendStop
 * @typedef {{default: {adapter: string, model: string}, access: {roles: Object<string, string[]>, people: Object<string, string[]>}, roles: string[], people: {id: string, name: string, owner: boolean}[]}} ModelsOverview
 * @typedef {{id: string, enabled: boolean, default: boolean, default_model: ?string, answers: boolean, models: ?string[], error: ?string}} Provider
 */

/** Dollars as the sections show them. @param {?number|undefined} n */
function dollars(n) { return n === null || n === undefined ? "No limit" : `$${Number(n).toFixed(2)}`; }

/** An amount typed in a limit field: a number, or null for "no limit".
 * @param {string} v @returns {?number} */
function amountOf(v) {
  const t = String(v ?? "").trim().replace(/^\$/, "");
  if (!t) return null;
  const n = Number(t);
  if (!Number.isFinite(n) || n < 0) throw new Error("A limit is an amount of dollars from 0 up, or empty for no limit.");
  return n;
}

/** A small table of a breakdown. @param {string} title @param {SpendRow[]} rows */
function spendTable(title, rows) {
  if (!rows.length) return `<h3>${esc(title)}</h3><p class="empty small">Nothing spent.</p>`;
  return `<h3>${esc(title)}</h3><table class="admin-table"><tbody>${rows.map(r => `<tr><td>${esc(r.name)}</td><td class="num">${dollars(r.usd)}</td></tr>`).join("")}</tbody></table>`;
}

/** A limit's form: an amount (empty lifts it) and Save.
 * @param {object} what the request's scope fields @param {?number|undefined} value @param {string} label */
function limitForm(what, value, label) {
  return `<form class="row limit-form" data-limit='${esc(JSON.stringify(what))}'><label>${esc(label)} <input name="usd" inputmode="decimal" placeholder="No limit" value="${value === null || value === undefined ? "" : esc(value)}" size="9"></label><button type="submit">Save</button></form>`;
}

/** Admin → Spend. @param {HTMLElement} el @param {string} [month] */
async function adminSpend(el, month) {
  /** @type {SpendOverview} */
  const o = await adminApi("GET", `/v1/admin/spend${month ? `?month=${encodeURIComponent(month)}` : ""}`);
  const pending = o.requests.filter(r => !r.decided), decided = o.requests.filter(r => r.decided);
  paint(el, `<div class="stack admin-spend">
    <div class="row"><h2>Spend</h2><label>Month <input type="month" data-spend-month value="${esc(o.month)}"></label></div>
    <p><strong>${dollars(o.total_usd)}</strong> spent in ${esc(o.month)}${o.server_left_usd === null ? "" : `, ${dollars(o.server_left_usd)} left of the server's limit`}.</p>
    <h3>Limits</h3>
    <p class="small muted">Monthly, in dollars. Limits cascade: nobody spends past what is left of the server's limit, whatever their own.</p>
    ${limitForm({ scope: "server" }, o.limits.server_usd, "The server")}
    ${limitForm({ scope: "default" }, o.limits.default_usd, "Each person, by default")}
    <h3>Requests for more</h3>
    ${pending.length ? pending.map(r => `<div class="card stack" data-request="${esc(r.id)}">
      <p><strong>${esc(r.name)}</strong> asks for ${dollars(r.usd)} more in ${esc(r.month)}.${r.reason ? ` “${esc(r.reason)}”` : ""}</p>
      <div class="row"><label>Amount <input name="usd" inputmode="decimal" value="${esc(r.usd)}" size="8"></label><label>Note <input name="note" placeholder="Optional"></label>
      <button class="primary" data-decide="approve">Approve</button><button data-decide="decline">Decline</button></div></div>`).join("") : `<p class="empty small">None waiting.</p>`}
    ${decided.length ? `<ul class="small">${decided.map(r => `<li>${esc(r.name)}: ${dollars(r.usd)} asked, ${r.decided?.approved ? `${dollars(r.decided.usd)} approved` : "declined"} by ${esc(r.decided?.by)}</li>`).join("")}</ul>` : ""}
    <h3>People</h3>
    <table class="admin-table"><thead><tr><th>Person</th><th class="num">Spent</th><th>Own limit</th><th class="num">Left</th></tr></thead><tbody>
    ${o.by_person.map(p => `<tr><td>${esc(p.name)}</td><td class="num">${dollars(p.usd)}</td><td>${limitForm({ scope: "person", person: p.id }, p.override_usd, "")}${p.override_usd === null || p.override_usd === undefined ? `<span class="small muted">Default${p.extra_usd ? `, +${dollars(p.extra_usd)} approved` : ""}</span>` : p.extra_usd ? `<span class="small muted">+${dollars(p.extra_usd)} approved</span>` : ""}</td><td class="num">${dollars(p.left_usd)}</td></tr>`).join("")}
    </tbody></table>
    <h3>Team agent</h3>
    ${o.workspaces.length ? o.workspaces.map(w => `<div class="stack"><p><strong>${esc(w.name)}</strong>: ${dollars(w.spent_usd)} spent this month.</p>
      ${limitForm({ scope: "workspace", workspace: w.id }, w.limit_usd, "Whole workspace")}
      ${w.channels.map(c => limitForm({ scope: "channel", workspace: w.id, channel: c.id }, c.limit_usd, `#${c.name} (${dollars(c.spent_usd)} spent)`)).join("")}</div>`).join("") : `<p class="empty small">No workspaces.</p>`}
    <h3>Breakdown</h3>
    ${spendTable("By person", o.by_person.filter(p => p.usd > 0))}${spendTable("By agent", o.by_agent)}${spendTable("By model", o.by_model)}${spendTable("By project", o.by_project)}
    ${o.alerts.length ? `<h3>Alerts this month</h3><ul class="small">${o.alerts.map(a => `<li>${esc(a.limit)} reached ${a.percent}%</li>`).join("")}</ul>` : ""}
  </div>`);
  const again = () => adminSpend(el, o.month);
  $("[data-spend-month]", el).onchange = e => adminSpend(el, /** @type {HTMLInputElement} */ (e.target).value);
  for (const f of el.querySelectorAll("form[data-limit]")) /** @type {HTMLFormElement} */ (f).onsubmit = async e => {
    e.preventDefault();
    try {
      const body = { ...JSON.parse(f.dataset.limit || "{}"), usd: amountOf(/** @type {HTMLInputElement} */ ($("[name=usd]", f)).value) };
      await adminApi("POST", "/v1/admin/spend/limits", body);
      toast(body.usd === null ? "Limit lifted." : `Limit set to ${dollars(body.usd)} a month.`);
      await again();
    } catch (err) { if (!err.cancelled) toast(err.message); }
  };
  for (const b of el.querySelectorAll("[data-decide]")) /** @type {HTMLElement} */ (b).onclick = async () => {
    const card = b.closest("[data-request]"), approve = b.dataset.decide === "approve";
    try {
      const amount = amountOf($("[name=usd]", card).value);
      await adminApi("POST", `/v1/admin/spend/requests/${encodeURIComponent(card.dataset.request)}`, { approve, usd: approve ? amount : null, note: $("[name=note]", card).value });
      toast(approve ? "Approved. They are told." : "Declined. They are told.");
      await again();
    } catch (err) { if (!err.cancelled) toast(err.message); }
  };
}

/** Admin → Models. @param {HTMLElement} el */
async function adminModels(el) {
  /** @type {[ModelsOverview, {providers: Provider[]}]} */
  const [o, p] = await Promise.all([adminApi("GET", "/v1/admin/models"), adminApi("GET", "/v1/admin/providers")]);
  const all = p.providers.flatMap(x => (x.models || []).map(m => `${x.id}/${m}`));
  const def = `${o.default.adapter}/${o.default.model}`;
  /** @param {string} key @param {?string[]} rule */
  const ruleForm = (key, rule) => `<form class="stack rule-form" data-rule="${esc(key)}">
    <p class="small muted">${rule ? `May use ${rule.length} model${rule.length === 1 ? "" : "s"}.` : "No rule: every model."}</p>
    <div class="row wrap">${all.map(m => `<label class="small"><input type="checkbox" name="m" value="${esc(m)}"${rule?.includes(m) ? " checked" : ""}> ${esc(m)}</label>`).join("")}</div>
    <div class="row"><button type="submit">Save rule</button>${rule ? `<button type="button" data-rule-remove>Remove rule</button>` : ""}</div></form>`;
  paint(el, `<div class="stack admin-models">
    <h2>Models</h2>
    <p>New work runs on <strong>${esc(def)}</strong> unless a person may not use it; then it runs on the first model they may.</p>
    <h3>Providers</h3>
    ${p.providers.map(x => `<div class="card stack" data-provider="${esc(x.id)}">
      <div class="row"><strong>${esc(x.id)}</strong><span class="chip${x.answers ? "" : " bad"}">${x.answers ? "Answering" : esc(x.error || "Not answering")}</span>
      <button data-provider-toggle>${x.enabled ? "Disable" : "Enable"}</button><button data-provider-test>Test</button></div>
      ${(x.models || []).length ? `<ul class="small">${(x.models || []).map(m => `<li>${esc(m)}${`${x.id}/${m}` === def ? ` <span class="chip">Default</span>` : x.enabled ? ` <button class="link" data-default="${esc(m)}">Make default</button>` : ""}</li>`).join("")}</ul>` : ""}</div>`).join("")}
    <h3>Who may use which</h3>
    <p class="small muted">A person's own rule wins over their roles'. Several roles' rules add up, and everyone has the role member. The owner may use every model.</p>
    ${o.roles.map(r => `<details><summary>Role: ${esc(r)}</summary>${ruleForm(`role:${r}`, o.access.roles[r] || null)}</details>`).join("")}
    ${o.people.filter(x => !x.owner).map(x => `<details><summary>${esc(x.name)}</summary>${ruleForm(`person:${x.id}`, o.access.people[x.id] || null)}</details>`).join("")}
  </div>`);
  const again = () => adminModels(el);
  const act = async (/** @type {() => Promise<any>} */ f, /** @type {string} */ said) => {
    try { await f(); toast(said); await again(); } catch (err) { if (!err.cancelled) toast(err.message); }
  };
  for (const card of el.querySelectorAll("[data-provider]")) {
    const id = /** @type {HTMLElement} */ (card).dataset.provider || "", x = p.providers.find(y => y.id === id);
    $("[data-provider-toggle]", card).onclick = () => act(() => adminApi("PATCH", `/v1/admin/providers/${encodeURIComponent(id)}`, { enabled: !x?.enabled }), x?.enabled ? "Disabled." : "Enabled.");
    $("[data-provider-test]", card).onclick = async () => {
      try { const t = await adminApi("POST", `/v1/admin/providers/${encodeURIComponent(id)}/test`, {}); toast(t.ok ? `${id} answered in ${t.ms} ms.` : `${id}: ${t.error}`); }
      catch (err) { if (!err.cancelled) toast(err.message); }
    };
    for (const b of card.querySelectorAll("[data-default]")) /** @type {HTMLElement} */ (b).onclick = () =>
      act(() => adminApi("PATCH", `/v1/admin/providers/${encodeURIComponent(id)}`, { default_model: b.dataset.default }), "New work runs on it now.");
  }
  for (const f of el.querySelectorAll("form[data-rule]")) {
    const [kind, ...rest] = (/** @type {HTMLElement} */ (f).dataset.rule || "").split(":"), who = rest.join(":");
    const send = (/** @type {?string[]} */ models) => adminApi("POST", "/v1/admin/models/access", { [kind]: who, models });
    /** @type {HTMLFormElement} */ (f).onsubmit = e => {
      e.preventDefault();
      const models = [...f.querySelectorAll("[name=m]:checked")].map(i => /** @type {HTMLInputElement} */ (i).value);
      act(() => send(models), "Saved.");
    };
    const remove = $("[data-rule-remove]", f);
    if (remove) remove.onclick = () => act(() => send(null), "Rule removed.");
  }
}

/** Where a refusal for a spent limit appears: offer to ask for more, once
 * per refusal shown. A request already waiting is said, not asked again.
 * @param {{error: string, spend_limit: SpendStop}} body */
function offerMoreSpend(body) {
  const s = body?.spend_limit;
  if (!s || s.which !== "person" || !s.can_ask || $("dialog.spend-ask")) return;
  const d = document.createElement("dialog");
  d.className = "ask spend-ask";
  d.setAttribute("aria-label", "Ask for more");
  d.innerHTML = `<form class="stack" data-spend-ask>
    <h2>Ask for more</h2>
    <p>${esc(body.error)}</p>
    <label>How much more this month, in dollars <input name="usd" inputmode="decimal" required value="${esc(Math.max(5, Math.ceil(s.limit_usd / 2)))}"></label>
    <label>Why (optional) <textarea name="reason" rows="3" maxlength="2000"></textarea></label>
    <p class="small bad" role="alert" data-spend-ask-error hidden></p>
    <div class="row"><button type="button" data-spend-ask-cancel>Not now</button><button class="primary" type="submit">Ask an admin</button></div>
  </form>`;
  const form = $("form", d), error = $("[data-spend-ask-error]", d);
  $("[data-spend-ask-cancel]", d).onclick = () => d.close();
  form.onsubmit = async e => {
    e.preventDefault();
    const f = Object.fromEntries(new FormData(form));
    try {
      const amount = amountOf(String(f.usd));
      if (!amount) throw new Error("Ask for an amount above $0.");
      await api("POST", "/v1/spend/requests", { usd: amount, reason: String(f.reason || "") });
      d.close();
      toast("Asked. You will hear in your inbox when an admin decides.");
    } catch (err) { error.textContent = err.message; error.hidden = false; }
  };
  d.addEventListener("close", () => d.remove());
  document.body.append(d);
  d.showModal();
}

registerAdminSection("spend", el => adminSpend(el));

// Overview's card: this month against the server's limit, who has reached
// their own, and requests waiting, from Overview's answer (`spend`).
if (typeof registerOverviewCard === "function") registerOverviewCard("spend", async el => {
  /** @type {{month: string, total_usd: number, server_usd: ?number, server_left_usd: ?number, at_limit: string[], requests_waiting: number}|null} */
  const s = (await adminApi("GET", "/v1/admin/overview")).spend;
  if (!s) { paint(el, `<h3>Spend this month</h3><p class="small muted">Spend is not available.</p>`); return; }
  const of = s.server_usd === null || s.server_usd === undefined ? "no server limit" : `of ${dollars(s.server_usd)}`;
  paint(el, `<h3>Spend this month</h3>
    <p class="stat"><strong data-overview-spend-total>${dollars(s.total_usd)}</strong> ${esc(of)}</p>
    <p class="small muted">${s.at_limit.length ? `${esc(s.at_limit.join(", "))} at their limit · ` : ""}${s.requests_waiting ? `${s.requests_waiting} waiting for more · ` : ""}<a href="#/admin/spend">Open Spend</a></p>`);
});
registerAdminSection("models", el => adminModels(el));
