// Admin › Connectors (spec rev 141): which connectors people may connect
// (turn one off, or limit it to groups), which are connected for the whole
// workspace, and who has connected what. Suspending a connector at once is
// the kill switch's connector level, in the Admin top bar: this page points
// there rather than keeping a second switch.
"use strict";

/**
 * @typedef {{name: string, title: string, enabled: boolean, status: string, source: string, sign_in: string, access_groups: ?string[], credential: {kind: string, set: boolean}, connected_by: Array<{id: string, name: string}>, workspaces: Array<{id: string, name: string, roles: string[]}>, last_used: ?string}} AdminConnector
 */

/** @param {AdminConnector} c */
function connectorRow(c) {
  const who = c.connected_by || [], ws = c.workspaces || [];
  const groups = (c.access_groups || []).join(", ");
  return `<tr data-connector="${esc(c.name)}">
    <td><strong>${esc(c.title)}</strong><div class="small muted mono">${esc(c.name)} · ${esc(c.source)}</div></td>
    <td><label class="small"><input type="checkbox" data-connector-allow${c.enabled ? " checked" : ""}> People may connect it</label>
      <div class="small"><input data-connector-groups value="${esc(groups)}" placeholder="Every group" aria-label="Only these groups" size="18"></div></td>
    <td class="small">${ws.length ? ws.map(w => `${esc(w.name)} <span class="muted">(${esc(w.roles.join(", "))})</span>`).join("<br>") : `<span class="muted">No workspace</span>`}</td>
    <td class="small">${who.length ? who.map(p => esc(p.name)).join(", ") : `<span class="muted">No one</span>`}</td>
    <td class="small">${c.credential?.kind !== "none" ? (c.credential?.set ? "Credential set" : `<span class="bad">Needs its credential</span>`) : ""}
      ${c.credential?.kind === "key" ? `<button data-connector-key>${c.credential.set ? "Replace key" : "Set key"}</button>` : ""}</td>
  </tr>`;
}

/** @param {AdminConnector[]} list */
function connectorsHtml(list) {
  return `<h2>Connectors</h2>
    <p class="small muted">To cut a connector off from every agent at once, use the kill switch above and choose the connector: running work that needs it stops.</p>
    <table class="t" data-connectors><thead><tr><th>Connector</th><th>Who may connect it</th><th>Connected for workspaces</th><th>Connected by</th><th></th></tr></thead>
      <tbody>${list.map(connectorRow).join("")}</tbody></table>`;
}

/** Ask for the organisation's key, hidden as it is typed.
 * @param {string} name @returns {Promise<?string>} */
function connectorKeySheet(name) {
  return new Promise(done => {
    const d = document.createElement("dialog");
    d.className = "ask";
    d.innerHTML = `<form class="stack"><h2>The key for ${esc(name)}</h2><p class="small muted">It goes into the vault and is never shown again.</p>
      <label>Key <input type="password" name="value" autocomplete="off" required></label>
      <div class="row"><button type="button" data-cancel>Cancel</button><button class="primary">Save</button></div></form>`;
    let out = null;
    $("[data-cancel]", d).onclick = () => d.close();
    $("form", d).onsubmit = e => { e.preventDefault(); out = String(new FormData(/** @type {HTMLFormElement} */ (e.target)).get("value") || ""); d.close(); };
    d.addEventListener("close", () => { d.remove(); done(out); });
    document.body.append(d);
    d.showModal();
  });
}

registerAdminSection("connectors", async el => {
  /** @type {{connectors: AdminConnector[]}} */
  const v = await adminApi("GET", "/v1/admin/connectors");
  paint(el, connectorsHtml(v.connectors || []));
  const again = () => AdminState.sections.get("connectors")?.(el);
  /** @param {any} err */
  const failed = err => { if (!err.cancelled) toast(err.body?.error || err.message); };
  /** @param {HTMLElement} t */
  const nameOf = t => t.closest("[data-connector]")?.getAttribute("data-connector") || "";
  el.onchange = async e => {
    const t = /** @type {HTMLInputElement} */ (e.target), name = nameOf(t);
    if (!name) return;
    try {
      if (t.matches("[data-connector-allow]")) await adminApi("PATCH", `/v1/admin/connectors/${enc(name)}`, { enabled: t.checked });
      else if (t.matches("[data-connector-groups]")) await adminApi("PATCH", `/v1/admin/connectors/${enc(name)}`, { access_groups: t.value.split(",").map(s => s.trim()).filter(Boolean) });
      else return;
      toast("Saved.");
    } catch (err) { failed(err); }
    await again();
  };
  el.onclick = async e => {
    const t = /** @type {HTMLElement} */ (e.target);
    if (!t.matches("[data-connector-key]")) return;
    const name = nameOf(t), value = await connectorKeySheet(name);
    if (!value) return;
    try { await adminApi("PUT", `/v1/admin/connectors/${enc(name)}/credential`, { value }); toast("Key saved."); }
    catch (err) { failed(err); }
    await again();
  };
});
