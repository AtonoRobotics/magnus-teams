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

/** @param {?PersonRule} r */
function ruleForm(r) {
  const m = r?.compiled;
  const effect = m?.effect || "ask";
  return `<form data-rule-form="${esc(r?.id || "")}" class="stack">
    <label>In your words <textarea name="words" rows="2" maxlength="2000" placeholder="Ask me before anything is posted in Slack">${esc(r?.words || "")}</textarea></label>
    <div class="row">${RULE_EFFECTS.map(([v, label]) => `<label class="row"><input type="radio" name="effect" value="${v}"${v === effect ? " checked" : ""}> ${esc(label)}</label>`).join("")}</div>
    <div class="row"><label>App <input name="connector" value="${esc(m?.connector || "")}" placeholder="slack"></label>
    <label>Action <input name="tool" value="${esc(m?.tool || "")}" placeholder="any"></label></div>
    <label>To or where, comma separated <input name="destinations" value="${esc((m?.destinations || []).join(", "))}" placeholder="anywhere"></label>
    <p class="small muted">Allowing needs an app, an action and where it goes. Asking and blocking can cover a whole app.</p>
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
    /** @type {PersonRule[]} */
    const rules = (await api("GET", "/v1/rules")).rules || [];
    const label = v => (RULE_EFFECTS.find(x => x[0] === v) || [v, v])[1];
    paint(el, `<h2>Rules</h2>
      <p class="muted">Tell the agent, in your own words, what it may do without asking, what to ask you first, and what never to do.</p>
      ${rules.length ? rules.map(r => editing === r.id ? `<div class="card">${ruleForm(r)}</div>` : `<div class="card"><div class="spread"><div>
        <strong>${esc(label(r.compiled?.effect || (r.compiled ? "allow" : "words")))}</strong>${r.accepted ? "" : ` <span class="small faint">suggested by the agent</span>`}
        ${r.words ? `<div>${esc(r.words)}</div>` : ""}<div class="small muted">${esc(ruleMatchText(r.compiled))}</div></div>
        <div class="row">${r.accepted ? "" : `<button class="primary" data-rule="accept" data-id="${esc(r.id)}">Accept</button>`}<button data-rule="edit" data-id="${esc(r.id)}">Edit</button><button data-rule="remove" data-id="${esc(r.id)}">Remove</button></div></div></div>`).join("") : `<p class="muted">No rules yet.</p>`}
      ${editing ? "" : `<div class="card"><h3>New rule</h3>${ruleForm(null)}</div>`}
      <p class="small err" data-rule-note></p>`);
    const note = m => { $("[data-rule-note]", el).textContent = m; };
    const run = async fn => { try { await fn(); await draw(); } catch (e) { note(e.message || String(e)); } };
    el.querySelectorAll("[data-rule]").forEach(b => b.onclick = () => run(async () => {
      const id = b.dataset.id;
      if (b.dataset.rule === "edit") editing = id;
      if (b.dataset.rule === "cancel") editing = "";
      if (b.dataset.rule === "accept") await api("POST", `/v1/rules/${enc(id)}/accept`);
      if (b.dataset.rule === "remove" && await ask("Remove this rule?", "Remove")) await api("DELETE", `/v1/rules/${enc(id)}`);
    }));
    el.querySelectorAll("[data-rule-form]").forEach(form => form.onsubmit = e => {
      e.preventDefault();
      const id = form.dataset.ruleForm;
      run(async () => {
        await api(id ? "PUT" : "POST", id ? `/v1/rules/${enc(id)}` : "/v1/rules", ruleBody(form));
        editing = "";
      });
    });
  };
  await draw();
}
