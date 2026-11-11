// Admin › Security (spec rev 141): the sign-in policy (identity provider,
// two-factor, session length, lockout), the audit log with export, how long
// chats, files and logs are kept, and the sessions where a person let
// developers look at their computer.
"use strict";

/**
 * @typedef {{code_required: boolean, idle_minutes: number, max_hours: number, lockout_enabled: boolean, lockout_failures: number, lockout_wait_minutes: number, lockout_max_wait_minutes: number, lockout_permanent: boolean, identity_provider: ?string}} AdminSignIn
 * @typedef {{seq: number, at: string, admin: string, name: string, action: string, target: ?string, source: ?string}} AuditEntry
 * @typedef {{kept: Array<{what: string, how: string}>, purge_window_days?: number, settable?: {trash_days: number, backup_days: number, set_here: boolean, min_days: number, trash_days_max: number, backup_days_max: number}}} AdminRetention
 * @typedef {{computer: string, owner: {person?: string, workspace?: string}, operator: string, until: string, ended: boolean}} DevSession
 */

/** @param {AdminSignIn} p */
function signInHtml(p) {
  const n = (name, label, min, max) => `<label>${label} <input type="number" name="${name}" min="${min}" max="${max}" value="${esc(p[name])}"></label>`;
  return `<section class="card"><h3>Sign-in</h3>
    <p class="small">${p.identity_provider ? `People sign in through ${esc(p.identity_provider)}.` : "People sign in with a password kept on this server."}</p>
    <form class="stack" data-sign-in>
      <label><input type="checkbox" name="code_required"${p.code_required ? " checked" : ""}> Everyone uses a code from an authenticator app (two-factor)</label>
      ${n("idle_minutes", "Sign out after this many minutes unused", 5, 1440)}
      ${n("max_hours", "Sign in again after this many hours", 1, 720)}
      <label><input type="checkbox" name="lockout_enabled"${p.lockout_enabled ? " checked" : ""}> Lock an account after failed attempts</label>
      ${n("lockout_failures", "Failures before a lockout", 1, 100)}
      ${n("lockout_wait_minutes", "First wait, in minutes", 1, 1440)}
      ${n("lockout_max_wait_minutes", "Longest wait, in minutes", 1, 10080)}
      <div class="row"><button class="primary">Save</button></div>
    </form></section>`;
}

/** The identity provider, for an owner: settings, a test sign-in through
 * it as themselves, then the switch (the server refuses one untested).
 * @param {{source: string, settings: ?Record<string, ?string>, settable: string[], in_use: ?string}} idp */
function idpHtml(idp) {
  const v = idp.settings || {};
  const labels = { issuer: "Issuer", client_id: "Client ID", person_claim: "Claim naming the person", scope: "Scope", label: "Name on the sign-in button", groups_claim: "Groups claim", roles_claim: "Roles claim", status_client: "Status client", status_credential: "Vault secret of the status client", status_admin_url: "Admin API address" };
  return `<form class="stack" data-idp>
      <h4>Identity provider</h4>
      ${(idp.settable || []).map(k => `<label>${esc(labels[k] || k)} <input name="${esc(k)}" value="${esc(v[k] ?? "")}"></label>`).join("")}
      <p class="small muted" data-idp-status>Test a sign-in through the provider as yourself before switching: a provider that cannot sign you in would lock you out.</p>
      <div class="row"><button type="button" data-idp-test>Test sign-in</button><button type="button" class="primary" data-idp-use disabled>Use this provider</button>${idp.in_use ? `<button type="button" data-idp-off>Use passwords kept here</button>` : ""}</div>
    </form>`;
}

/** @param {AuditEntry[]} rows */
function auditHtml(rows) {
  return `<section class="card"><h3>Audit log</h3>
    <form class="row" data-audit-filter><input name="actor" placeholder="Who" aria-label="Who"><input name="action" placeholder="What" aria-label="What"><button>Filter</button><button type="button" data-audit-export>Export CSV</button></form>
    <table class="t"><tbody data-audit>${auditRows(rows)}</tbody></table></section>`;
}

/** The audit's rows, newest first, the latest 200. @param {AuditEntry[]} rows */
function auditRows(rows) {
  return rows.slice(0, 200).map(r => `<tr><td class="small muted">${esc(when(r.at))}</td><td class="small">${esc(r.name || r.admin)}</td><td class="small mono">${esc(r.action)}</td><td class="small">${esc(r.target || "")}</td></tr>`).join("") || `<tr><td class="small muted">Nothing recorded yet.</td></tr>`;
}

/** @param {AdminRetention} r */
function retentionHtml(r) {
  return `<section class="card" data-retention><h3>How long things are kept</h3>
    <dl>${(r.kept || []).map(k => `<dt>${esc(k.what[0].toUpperCase() + k.what.slice(1))}</dt><dd class="small">${esc(k.how)}</dd>`).join("")}</dl>
    ${r.settable ? retentionForm(r.settable, r.purge_window_days) : ""}</section>`;
}

/** The two windows an admin sets, each within the purge window.
 * @param {NonNullable<AdminRetention["settable"]>} s @param {number|undefined} purge */
function retentionForm(s, purge) {
  const n = (name, label, max) => `<label>${label} <input type="number" name="${name}" min="${s.min_days}" max="${max}" step="1" value="${esc(s[name])}"></label>`;
  return `<form class="stack" data-retention-form>
      ${n("trash_days", "Days a deleted item waits in the trash", s.trash_days_max)}
      ${n("backup_days", "Days each backup is kept", s.backup_days_max)}
      <p class="small muted">At most ${esc(purge ?? s.backup_days_max)} days each: a deletion still leaves the backups within that window.</p>
      <div class="row"><button class="primary">Save</button></div>
    </form>`;
}

/** @param {DevSession[]} rows */
function devAccessHtml(rows) {
  const now = Date.now();
  return `<section class="card" data-dev-access><h3>Developer access</h3>
    <p class="small muted">Each time a person let a developer look at their computer.</p>
    ${rows.length ? `<table class="t"><tbody>${rows.map(s => {
      const open = !s.ended && new Date(s.until).getTime() > now;
      return `<tr><td class="small">${esc(s.owner.person || s.owner.workspace || "")}</td><td class="small">${esc(s.operator)}</td><td class="small">${open ? `<span class="chip">Open until ${esc(new Date(s.until).toLocaleString())}</span>` : "Ended"}</td></tr>`;
    }).join("")}</tbody></table>` : `<p class="small muted">No one has granted access.</p>`}</section>`;
}

/** Save `csv` as a file named `filename`. @param {string} filename @param {string} csv */
function securitySave(filename, csv) {
  const a = document.createElement("a");
  a.href = URL.createObjectURL(new Blob([csv], { type: "text/csv" }));
  a.download = filename;
  document.body.append(a);
  a.click();
  setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 1000);
}

registerAdminSection("security", async el => {
  const owner = !!AdminState.standing?.owner;
  const [policy, audit, retention, dev, idp] = await Promise.all([
    adminApi("GET", "/v1/admin/sign-in-policy"), adminApi("GET", "/v1/admin/audit"), adminApi("GET", "/v1/admin/retention"), adminApi("GET", "/v1/admin/developer-access"),
    owner ? adminApi("GET", "/v1/admin/identity-provider") : null,
  ]);
  paint(el, `<h2>Security</h2>${signInHtml(policy)}${idp ? `<section class="card">${idpHtml(idp)}</section>` : ""}${auditHtml(audit.entries || [])}${retentionHtml(retention)}${devAccessHtml(dev.sessions || [])}`);
  /** @param {any} err */
  const failed = err => { if (!err.cancelled) toast(err.body?.message || err.body?.error || err.message); };
  let query = "";
  $("[data-sign-in]", el).onsubmit = async e => {
    e.preventDefault();
    const form = /** @type {HTMLFormElement} */ (e.target), f = new FormData(form);
    const body = { code_required: f.get("code_required") === "on", lockout_enabled: f.get("lockout_enabled") === "on", lockout_permanent: !!policy.lockout_permanent };
    for (const k of ["idle_minutes", "max_hours", "lockout_failures", "lockout_wait_minutes", "lockout_max_wait_minutes"]) body[k] = Number(f.get(k));
    try { await adminApi("PUT", "/v1/admin/sign-in-policy", body); toast("Sign-in rules saved."); }
    catch (err) { failed(err); }
  };
  const keep = $("[data-retention-form]", el);
  if (keep) keep.onsubmit = async e => {
    e.preventDefault();
    const f = new FormData(/** @type {HTMLFormElement} */ (e.target));
    try { await adminApi("PUT", "/v1/admin/retention", { trash_days: Number(f.get("trash_days")), backup_days: Number(f.get("backup_days")) }); toast("Saved."); await AdminState.sections.get("security")?.(el); }
    catch (err) { failed(err); }
  };
  if (idp) wireIdp(el, failed);
  $("[data-audit-filter]", el).onsubmit = async e => {
    e.preventDefault();
    const f = new FormData(/** @type {HTMLFormElement} */ (e.target));
    query = new URLSearchParams([...f].filter(([, v]) => v)).toString();
    try { const v = await adminApi("GET", `/v1/admin/audit${query ? `?${query}` : ""}`); paint($("[data-audit]", el), auditRows(v.entries || [])); }
    catch (err) { failed(err); }
  };
  $("[data-audit-export]", el).onclick = async () => {
    try { const v = await adminApi("GET", `/v1/admin/audit/export${query ? `?${query}` : ""}`); securitySave(v.filename, v.csv); }
    catch (err) { failed(err); }
  };
});

/** The provider form's buttons: test, then switch; or back to passwords.
 * @param {HTMLElement} el @param {(err: any) => void} failed */
function wireIdp(el, failed) {
  const form = /** @type {HTMLFormElement} */ ($("[data-idp]", el));
  const status = $("[data-idp-status]", el), use = $("[data-idp-use]", el);
  const settings = () => Object.fromEntries([...new FormData(form)].filter(([, v]) => String(v).trim()));
  /** @type {any} */ let tested = null;
  $("[data-idp-test]", el).onclick = async () => {
    try {
      const body = settings();
      const s = await adminApi("POST", "/v1/admin/identity-provider/test", body);
      const url = s.verification_uri_complete || s.verification_uri;
      paint(status, `Open <a href="${esc(url)}" target="_blank" rel="noopener">${esc(url)}</a>, enter ${esc(s.user_code)} and sign in as yourself.`);
      const every = Math.max(1, s.interval);
      for (let waited = 0; waited < s.expires_in; waited += every) {
        await new Promise(r => setTimeout(r, every * 1000));
        const p = await adminApi("POST", "/v1/admin/identity-provider/test/poll", {});
        if (p.status === "pending") continue;
        paint(status, esc(p.message));
        if (p.status === "passed") { tested = body; use.disabled = false; }
        return;
      }
    } catch (err) { failed(err); }
  };
  use.onclick = async () => {
    try { await adminApi("PUT", "/v1/admin/identity-provider", { provider: tested }); toast("People sign in through the new provider."); await AdminState.sections.get("security")?.(el); }
    catch (err) { failed(err); }
  };
  const off = $("[data-idp-off]", el);
  if (off) off.onclick = async () => {
    if (!confirm("Sign in with passwords kept here? People without one here cannot sign in until they set one.")) return;
    try { await adminApi("PUT", "/v1/admin/identity-provider", { provider: null }); toast("People sign in with passwords kept here."); await AdminState.sections.get("security")?.(el); }
    catch (err) { failed(err); }
  };
}
