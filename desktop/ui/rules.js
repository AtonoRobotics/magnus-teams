// The person's own rules, in their words: allow automatically, ask me
// first, or block. The words say what the rule means; the match (an app,
// optionally one of its actions, and where it goes) is what the server
// enforces. Ask first wins over allow, and block over both.

/**
 * @typedef {{connector: string, tool: string, destinations: string[], once: boolean, effect: "allow"|"ask"|"block"}} RuleMatch
 * @typedef {{id: string, person: string, words: ?string, compiled: ?RuleMatch, accepted: boolean, used: number}} PersonRule
 */

const RULE_EFFECTS = [["ask", "Ask me first"], ["block", "Block it"], ["allow", "Allow automatically"]];

/** @param {?RuleMatch} m */
function ruleMatchText(m) {
  if (!m) return "Words only: Auto Review reads them, and nothing is matched by them alone.";
  const what = m.tool ? m.tool.replace(/^[^_]+__/, "").replace(/_/g, " ") : "anything";
  const where = m.destinations.length ? ` to ${m.destinations.join(", ")}` : "";
  return `${what} in ${m.connector}${where}${m.once ? ", once" : ""}`;
}

const RULE_ACTION_PRESETS = [
  ["gmail__send", "Send email (Gmail)"],
  ["gmail__draft", "Draft email (Gmail)"],
  ["outlook__send", "Send email (Outlook)"],
  ["slack__post_message", "Post message (Slack)"],
  ["calendar__create_event", "Create event (Calendar)"],
  ["calendar__update_event", "Update event (Calendar)"],
  ["github__create_issue", "Create issue (GitHub)"],
  ["github__comment", "Comment on PR/issue (GitHub)"],
  ["notion__create_page", "Create page (Notion)"],
  ["linear__create_issue", "Create issue (Linear)"],
];

/** @param {?PersonRule} r */
function ruleForm(r) {
  const m = r?.compiled;
  const effect = m?.effect || "ask";
  const apps = typeof App !== "undefined" && Array.isArray(App.connectors)
    ? App.connectors.filter(c => (typeof stateOf === "function" ? stateOf(c) : c.state) === "connected")
    : [];
  const datalist = apps.length
    ? `<datalist id="rule-apps-list">${apps.map(a => `<option value="${esc(a.name)}">${esc(a.title || a.name)}</option>`).join("")}</datalist>`
    : "";
  const actionList = `<datalist id="rule-actions-list">${RULE_ACTION_PRESETS.map(([id, lbl]) => `<option value="${esc(id)}">${esc(lbl)}</option>`).join("")}</datalist>`;
  return `<form data-rule-form="${esc(r?.id || "")}" class="stack">
    <label>In your words <textarea name="words" rows="2" maxlength="2000" placeholder="Ask me before anything is posted in Slack">${esc(r?.words || "")}</textarea></label>
    <div class="row">${RULE_EFFECTS.map(([v, label]) => `<label class="row"><input type="radio" name="effect" value="${v}"${v === effect ? " checked" : ""}> ${esc(label)}</label>`).join("")}</div>
    <details class="rule-advanced" open><summary class="small muted">Specific app, action, or destination</summary>
      <div class="row" style="margin-top:8px"><label>App <input name="connector" list="rule-apps-list" value="${esc(m?.connector || "")}" placeholder="Pick an app (e.g. gmail, slack)"></label>
      <label>Action <input name="tool" list="rule-actions-list" value="${esc(m?.tool || "")}" placeholder="Pick an action or leave blank for any"></label></div>
      <label style="margin-top:8px">To or where, comma separated <input name="destinations" value="${esc((m?.destinations || []).join(", "))}" placeholder="e.g. bob@acme.com, @acme.com, #updates"></label>
      <p class="small muted">Supports exact emails, domain wildcards (<span class="mono">@acme.com</span>), and channels (<span class="mono">#general</span>). Allowing needs an app, action, and destination; spending money always asks.</p>
    </details>
    ${datalist}${actionList}
    <div class="row"><button class="primary" type="submit">${r ? "Save" : "Add rule"}</button>${r ? `<button type="button" data-rule="cancel">Cancel</button>` : ""}</div>
  </form>`;
}

/** @param {HTMLFormElement} form */
function ruleBody(form) {
  const d = Object.fromEntries(new FormData(form));
  const destinations = String(d.destinations || "").split(",").map(s => s.trim()).filter(Boolean);
  const connector = String(d.connector || "").trim();
  return { words: String(d.words || ""), effect: d.effect || "ask", connector: connector || null, tool: String(d.tool || "").trim() || null, destinations };
}

async function rulesSection(el) {
  let editing = "";
  const draw = async () => {
    const [rulesRes, suggRes, grantsRes] = await Promise.all([
      Server.rules.list(),
      Server.rules.suggestions ? Server.rules.suggestions().catch(() => ({ suggestions: [] })) : Promise.resolve({ suggestions: [] }),
      Server.memory?.grants ? Server.memory.grants().catch(() => ({ grants: [] })) : Promise.resolve({ grants: [] }),
    ]);
    /** @type {PersonRule[]} */
    const rules = rulesRes.rules || [];
    const suggestions = suggRes.suggestions || [];
    const activeGrants = (grantsRes.grants || []).filter(g => !g.revoked);
    const label = v => (RULE_EFFECTS.find(x => x[0] === v) || [v, v])[1];
    const suggHtml = suggestions.length ? `<section data-trust-suggestions style="margin-bottom:14px">
      <h3>Suggested rules <span class="count">${suggestions.length}</span></h3>
      <p class="small muted">Actions you've repeatedly approved — graduate them into standing rules with one click.</p>
      ${suggestions.map((sg, idx) => `<div class="card flat" data-rule-suggestion="${idx}"><div class="spread"><div>
        <strong>${esc(sg.tool.replace(/^[^_]+__/, "").replace(/_/g, " "))}</strong> in <span class="mono">${esc(sg.connector)}</span> → <span class="mono">${esc(sg.destination)}</span>
        <div class="small muted">Approved ${esc(String(sg.count))} times</div></div>
        <button type="button" class="primary small" data-rule-graduate="${idx}">Allow automatically</button>
      </div></div>`).join("")}
    </section>` : "";
    const grantsHtml = activeGrants.length ? `<section data-rules-grants style="margin-top:14px">
      <h3>Standing delegations <span class="count">${activeGrants.length}</span></h3>
      <p class="small muted">Categories Magnus may handle within your monthly limits. Spending money always asks.</p>
      ${activeGrants.map(g => `<div class="card flat"><div class="spread"><div>
        <strong>${esc(g.title)}</strong>
        <div class="small muted">Apps: ${esc((g.apps || []).join(", "))} · Still asks before: ${esc((g.still_asks || []).join(", ") || "payments")} · $${esc(Number(g.spent || 0).toFixed(2))} of $${esc(g.max_usd)}/${esc(g.period)}</div>
      </div><button type="button" class="small" data-grant-revoke="${esc(g.id)}">Revoke</button></div></div>`).join("")}
    </section>` : "";
    paint(el, `<h2>Guardrails &amp; Rules</h2>
      <p class="muted">Tell Magnus what it may do automatically, what to ask first, and what never to do.</p>
      ${suggHtml}
      ${rules.length ? rules.map(r => editing === r.id ? `<div class="card">${ruleForm(r)}</div>` : `<div class="card"><div class="spread"><div>
        <strong>${esc(label(r.compiled?.effect || (r.compiled ? "allow" : "words")))}</strong>${r.accepted ? "" : ` <span class="small faint">suggested by the agent</span>`}
        ${r.words ? `<div>${esc(r.words)}</div>` : ""}<div class="small muted">${esc(ruleMatchText(r.compiled))}</div></div>
        <div class="row">${r.accepted ? "" : `<button class="primary" data-rule="accept" data-id="${esc(r.id)}">Accept</button>`}<button data-rule="edit" data-id="${esc(r.id)}">Edit</button><button data-rule="remove" data-id="${esc(r.id)}">Remove</button></div></div></div>`).join("") : `<p class="muted">No rules yet.</p>`}
      ${editing ? "" : `<div class="card"><h3>New rule</h3>${ruleForm(null)}</div>`}
      ${grantsHtml}
      <p class="small err" data-rule-note></p>`);
    const note = m => { $("[data-rule-note]", el).textContent = m; };
    const run = async fn => { try { await fn(); await draw(); } catch (e) { note(e.message || String(e)); } };
    el.querySelectorAll("[data-rule-graduate]").forEach(b => b.onclick = () => run(async () => {
      const sg = suggestions[Number(b.dataset.ruleGraduate)];
      if (!sg) return;
      await Server.rules.save(null, { connector: sg.connector, tool: sg.tool, destinations: [sg.destination], effect: "allow", words: sg.words });
      toast("Standing rule saved.");
    }));
    el.querySelectorAll("[data-grant-revoke]").forEach(b => b.onclick = () => run(async () => {
      if (!await ask("Revoke this delegation? Magnus will ask before acting again.", "Revoke")) return;
      await Server.memory.removeGrant(b.dataset.grantRevoke);
      toast("Delegation revoked.");
    }));
    el.querySelectorAll("[data-rule]").forEach(b => b.onclick = () => run(async () => {
      const id = b.dataset.id;
      if (b.dataset.rule === "edit") editing = id;
      if (b.dataset.rule === "cancel") editing = "";
      if (b.dataset.rule === "accept") await Server.rules.accept(id);
      if (b.dataset.rule === "remove" && await ask("Remove this rule?", "Remove")) await Server.rules.remove(id);
    }));
    el.querySelectorAll("[data-rule-form]").forEach(form => form.onsubmit = e => {
      e.preventDefault();
      const id = form.dataset.ruleForm;
      run(async () => {
        await Server.rules.save(id, ruleBody(form));
        editing = "";
      });
    });
  };
  await draw();
}
