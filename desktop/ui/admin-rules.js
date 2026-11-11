// Admin › Rules (spec rev 141): workspace rules (allow, ask first or block)
// that cap every person's own rules, and whether Auto Review acts for the
// workspace. A workspace rule is checked as one of each person's, so its
// block and its ask first win over anyone's allow. The policy layers in
// Cedar are below, folded, for the admin who writes them.
"use strict";

/**
 * @typedef {{effect: "allow"|"ask"|"block", connector: string, tool: string, destinations: string[], words: ?string, by: string}} WorkspaceRule
 * @typedef {{rules: Array<{id: string, rule: WorkspaceRule}>, auto_review: {acts: boolean, set_here: boolean, config: boolean}}} AdminRules
 */

const WORKSPACE_RULE_EFFECTS = [["block", "Block"], ["ask", "Ask first"], ["allow", "Allow"]];

/** A rule as a sentence. @param {WorkspaceRule} r */
function ruleText(r) {
  const what = r.tool ? `${r.tool} in ${r.connector}` : `anything in ${r.connector}`;
  const where = r.destinations?.length ? ` to ${r.destinations.join(", ")}` : "";
  return `${WORKSPACE_RULE_EFFECTS.find(([e]) => e === r.effect)?.[1] || r.effect}: ${what}${where}`;
}

/** @param {AdminRules} v */
function rulesHtml(v) {
  const ar = v.auto_review || { acts: true, set_here: false, config: true };
  return `<h2>Rules</h2>
    <section class="card" data-auto-review><h3>Auto Review</h3>
      <label><input type="checkbox" data-auto-review-acts${ar.acts ? " checked" : ""}> Auto Review clears actions on its own for the workspace</label>
      <p class="small muted">Off, it still reviews every action that leaves a computer, and what it would allow asks the person first unless something they set up covers it.${ar.set_here ? "" : " Now as the server's config says."}</p></section>
    <h3>Workspace rules</h3>
    <p class="small muted">They apply to everyone. A block or ask first here wins over anyone's own allow.</p>
    ${v.rules?.length ? `<ul class="admin-rules" data-rules>${v.rules.map(({ id, rule }) => `<li data-rule="${esc(id)}"><span class="chip${rule.effect === "block" ? " bad" : ""}">${esc(ruleText(rule))}</span>${rule.words ? ` <span class="small">"${esc(rule.words)}"</span>` : ""} <button class="link" data-rule-remove>Remove</button></li>`).join("")}</ul>` : `<p class="small muted" data-rules>No workspace rules yet.</p>`}
    <form class="row" data-rule-new>
      <select name="effect" aria-label="What it does">${WORKSPACE_RULE_EFFECTS.map(([e, l]) => `<option value="${e}">${esc(l)}</option>`).join("")}</select>
      <input name="connector" placeholder="Connector, e.g. gmail" aria-label="Connector" required>
      <input name="tool" placeholder="Tool (any)" aria-label="Tool">
      <input name="destinations" placeholder="Destinations, comma-separated (any)" aria-label="Destinations">
      <input name="words" placeholder="In your words" aria-label="In your words">
      <button class="primary">Add rule</button>
    </form>
    <details data-policy><summary>The policy, in Cedar</summary><div data-policy-body><p class="small muted">Loading…</p></div></details>`;
}

/** The org layer's history, and a draft to test and apply. @param {HTMLElement} box */
async function rulesPolicy(box) {
  const [sets, history] = await Promise.all([adminApi("GET", "/v1/admin/policy"), adminApi("GET", "/v1/admin/policy/history?layer=org")]);
  const org = (sets.sets || []).find(s => s.id === "org") || {};
  paint(box, `<form class="stack" data-policy-form>
      <textarea name="cedar" rows="10" class="mono" aria-label="The organisation's policy">${esc(org.draft ?? org.live ?? "")}</textarea>
      <div class="row"><button type="button" data-policy-draft>Save draft</button><button class="primary" type="submit">Apply</button></div></form>
    <h4>History</h4><ul class="small">${(history.versions || []).slice(-10).reverse().map(h => `<li>Version ${esc(h.version)} by ${esc(h.by || "")}, ${esc(when(h.at))}</li>`).join("") || "<li class=muted>No versions yet.</li>"}</ul>`);
  const form = /** @type {HTMLFormElement} */ ($("[data-policy-form]", box));
  const cedar = () => String(new FormData(form).get("cedar") || "");
  $("[data-policy-draft]", box).onclick = () => adminApi("POST", "/v1/admin/policy/draft", { layer: "org", cedar: cedar() }).then(() => toast("Draft saved."), e => e.cancelled || toast(e.body?.error || e.message));
  form.onsubmit = e => { e.preventDefault(); adminApi("POST", "/v1/admin/policy/apply", { layer: "org", cedar: cedar() }).then(() => toast("Applied."), err => err.cancelled || toast(err.body?.error || err.message)); };
}

registerAdminSection("rules", async el => {
  /** @type {AdminRules} */
  const v = await adminApi("GET", "/v1/admin/rules");
  paint(el, rulesHtml(v));
  const again = () => AdminState.sections.get("rules")?.(el);
  /** @param {any} err */
  const failed = err => { if (!err.cancelled) toast(err.body?.message || err.message); };
  $("[data-auto-review-acts]", el).onchange = async e => {
    try { await adminApi("PUT", "/v1/admin/auto-review", { acts: /** @type {HTMLInputElement} */ (e.target).checked }); toast("Saved."); }
    catch (err) { failed(err); }
    await again();
  };
  $("[data-rule-new]", el).onsubmit = async e => {
    e.preventDefault();
    const f = Object.fromEntries(new FormData(/** @type {HTMLFormElement} */ (e.target)));
    const destinations = String(f.destinations || "").split(",").map(s => s.trim()).filter(Boolean);
    try { await adminApi("POST", "/v1/admin/rules", { effect: f.effect, connector: f.connector, tool: f.tool, destinations, words: f.words }); toast("Rule added."); await again(); }
    catch (err) { failed(err); }
  };
  el.onclick = async e => {
    const b = /** @type {HTMLElement} */ (e.target).closest("[data-rule-remove]");
    if (!b) return;
    try { await adminApi("DELETE", `/v1/admin/rules/${enc(b.closest("[data-rule]")?.getAttribute("data-rule"))}`); toast("Rule removed."); }
    catch (err) { failed(err); }
    await again();
  };
  $("[data-policy]", el).addEventListener("toggle", e => {
    if (/** @type {HTMLDetailsElement} */ (e.target).open) rulesPolicy($("[data-policy-body]", el)).catch(err => paint($("[data-policy-body]", el), `<p class="small bad">${esc(err.message)}</p>`));
  }, { once: true });
});
