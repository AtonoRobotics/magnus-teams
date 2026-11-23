// A person's own password: changing it in Settings → Account, and the
// "Forgot password?" step of signing in. Uses app.js's helpers ($, esc,
// paint, toast), Server (server.js) and the bridge's forgotPassword.
"use strict";

/**
 * What the server says about changing this person's password.
 * @typedef {{ here: boolean, min_length: number, provider: string|null }} PasswordRules
 */

/**
 * The Password card in Settings → Account: the current password, a new one
 * and the new one again. Changing it signs every other device out.
 * @param {HTMLElement|null} el
 */
async function passwordCard(el) {
  if (!el) return;
  /** @type {PasswordRules} */
  let rules;
  try { rules = await Server.account.password(); }
  catch (err) { el.innerHTML = `<h3>Password</h3><p class="small" role="alert">${esc(err.message)}</p>`; return; }
  if (!rules.here) {
    el.innerHTML = `<h3>Password</h3><p class="small muted">Your password is kept by ${esc(rules.provider || "your sign-in provider")}. Change it there.</p>`;
    return;
  }
  const least = rules.min_length;
  el.innerHTML = `<h3>Password</h3>
    <form class="card" data-password-form>
      <p class="small" data-password-note role="alert" hidden></p>
      <label for="pw-current">Current password</label><input id="pw-current" type="password" style="width:100%" autocomplete="current-password" required>
      <label for="pw-new">New password</label><input id="pw-new" type="password" style="width:100%" autocomplete="new-password" minlength="${least}" required>
      <label for="pw-again">New password again</label><input id="pw-again" type="password" style="width:100%" autocomplete="new-password" minlength="${least}" required>
      <p class="small muted">At least ${least} characters. Changing it signs you out on your other devices; this one stays signed in.</p>
      <div class="row"><button class="primary" data-password-save>Change password</button></div>
    </form>`;
  const form = /** @type {HTMLFormElement} */ ($("[data-password-form]", el));
  const note = /** @type {HTMLElement} */ ($("[data-password-note]", el));
  const say = (/** @type {string} */ msg) => { note.textContent = msg; note.hidden = !msg; note.style.color = msg ? "var(--bad)" : ""; };
  form.onsubmit = async e => {
    e.preventDefault();
    const field = (/** @type {string} */ id) => /** @type {HTMLInputElement} */ ($(id, form)).value;
    const current = field("#pw-current"), next = field("#pw-new");
    if (next.length < least) return say(`The new password needs at least ${least} characters.`);
    if (next !== field("#pw-again")) return say("The two new passwords do not match.");
    if (next === current) return say("The new password is the same as the current one.");
    say("");
    const save = /** @type {HTMLButtonElement} */ ($("[data-password-save]", form));
    save.disabled = true;
    try {
      const r = await Server.account.setPassword(current, next);
      form.reset();
      toast(r.signed_out ? `Your password is changed. ${r.signed_out === 1 ? "One other device was" : `${r.signed_out} other devices were`} signed out.` : "Your password is changed.");
    } catch (err) { say(err.message || String(err)); }
    save.disabled = false;
  };
}

/**
 * The "Forgot password?" step of signing in: asks for the email address and
 * asks the server to send a recovery link. The answer is the same whether or
 * not the address belongs to anyone, so nobody can use it to find accounts.
 * @param {HTMLElement} box where the sign-in screen draws
 * @param {{ url?: string, email?: string }} data
 * @param {() => void} back returns to the password step
 */
function forgotPasswordStep(box, data, back) {
  paint(box, `<form id="forgot"><p class="small muted">${esc(data.url || location.origin)}</p>
    <h3>Forgot your password?</h3>
    <p class="small">Enter the email address you use with Magnus. We will email you a link to choose a new password.</p>
    <label for="forgot-email">Email address</label><input id="forgot-email" type="email" style="width:100%" autocomplete="email" value="${esc(data.email || "")}" required autofocus>
    <p class="small" id="forgot-note" role="alert" hidden></p>
    <div class="row" style="margin-top:16px"><button class="primary" id="forgot-send">Email me a link</button><button type="button" class="link" id="forgot-back">Back to sign in</button></div></form>`);
  $("#forgot-back").onclick = back;
  $("#forgot").onsubmit = async e => {
    e.preventDefault();
    const email = /** @type {HTMLInputElement} */ ($("#forgot-email")).value.trim();
    const send = /** @type {HTMLButtonElement} */ ($("#forgot-send"));
    send.disabled = true;
    try {
      await Magnus.forgotPassword(email);
      paint(box, `<p class="small muted">${esc(data.url || location.origin)}</p>
        <h3>Check your email</h3>
        <p class="small" role="status">If ${esc(email)} belongs to someone here, a link to choose a new password is on its way. It works for one hour.</p>
        <p class="small muted">Choosing a new password signs you out everywhere. No email after a few minutes? Check your spam folder, or ask the owner of this server to set a new password for you.</p>
        <div class="row" style="margin-top:16px"><button class="primary" id="forgot-back">Back to sign in</button></div>`);
      $("#forgot-back").onclick = back;
    } catch (err) {
      const note = /** @type {HTMLElement} */ ($("#forgot-note"));
      note.textContent = err.message || String(err); note.hidden = false; note.style.color = "var(--bad)";
      send.disabled = false;
    }
  };
}

/**
 * What the server says about its outgoing mail (owner only).
 * @typedef {{ configured: boolean, host?: string, port?: number, from?: string, username?: string|null, credential?: string|null, password_set?: boolean, address: string|null }} MailStatus
 */

/**
 * The owner's Email card in Settings → Account: where recovery emails come
 * from, the mailbox password (it goes straight into the server's vault and
 * is never shown again) and a test email. Hidden for everyone else.
 * @param {HTMLElement|null} el
 */
async function mailCard(el) {
  if (!el) return;
  /** @type {MailStatus} */
  let m;
  try { m = await Server.account.mail(); } catch { el.innerHTML = ""; return; }
  if (!m.configured) {
    el.innerHTML = `<h3>Email</h3><p class="small muted">This server sends no email, so "Forgot password?" cannot reach anyone. Outgoing mail is set up on the server itself.</p>`;
    return;
  }
  el.innerHTML = `<h3>Email</h3>
    <form class="card" data-mail-form>
      <p class="small">Recovery emails come from <b>${esc(m.from || "")}</b> through ${esc(m.host || "")}:${esc(String(m.port || ""))}.
        ${m.address ? `Yours go to <b>${esc(m.address)}</b>.` : `<span style="color:var(--bad)">Your account has no recovery address yet.</span>`}</p>
      <p class="small" data-mail-note role="alert" hidden></p>
      ${m.credential ? `<label for="mail-pw">Password for ${esc(m.username || m.from || "the mailbox")}</label>
      <input id="mail-pw" type="password" style="width:100%" autocomplete="off" placeholder="${m.password_set ? "Saved. Enter a new one to replace it." : "Not saved yet"}">` : ""}
      <div class="row">${m.credential ? `<button class="primary" data-mail-save>Save password</button>` : ""}<button type="button" data-mail-test>Send a test email</button></div>
    </form>`;
  const form = /** @type {HTMLFormElement} */ ($("[data-mail-form]", el));
  const note = /** @type {HTMLElement} */ ($("[data-mail-note]", el));
  const say = (/** @type {string} */ msg, bad = true) => { note.textContent = msg; note.hidden = !msg; note.style.color = bad ? "var(--bad)" : ""; };
  form.onsubmit = async e => {
    e.preventDefault();
    const input = /** @type {HTMLInputElement|null} */ ($("#mail-pw", form));
    if (!input || !m.credential) return;
    if (!input.value) return say("Enter the mailbox password.");
    try {
      await Server.account.setSecret(m.credential, input.value);
      input.value = ""; input.placeholder = "Saved. Enter a new one to replace it.";
      say("Saved. Send a test email to check it.", false);
    } catch (err) { say(err.message || String(err)); }
  };
  /** @type {HTMLButtonElement} */ ($("[data-mail-test]", form)).onclick = async () => {
    say("Sending…", false);
    try { const r = await Server.account.testMail(); say(`Sent to ${r.to}. Check that inbox (and its spam folder).`, false); }
    catch (err) { say(err.message || String(err)); }
  };
}

const COMMON_TIMEZONES = ["UTC", "America/Los_Angeles", "America/Denver", "America/Chicago", "America/New_York", "America/Sao_Paulo", "Europe/London", "Europe/Paris", "Europe/Berlin", "Asia/Dubai", "Asia/Kolkata", "Asia/Singapore", "Asia/Tokyo", "Australia/Sydney"];

function deviceTimezone() {
  try { return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC"; } catch { return "UTC"; }
}

/**
 * When the person has not explicitly chosen a time zone yet, sync their
 * device's IANA time zone so schedules and quiet hours run in local time.
 */
async function autoDetectTimezone() {
  try {
    const st = await Server.account.totp();
    if (st && !st.timezone_explicit) {
      const tz = deviceTimezone();
      if (tz && tz !== "UTC") {
        const updated = await Server.preferences.set({ timezone: tz });
        if (typeof App !== "undefined" && App) App.prefs = updated;
      }
    }
  } catch { /* non-fatal on boot */ }
}

/**
 * Self-service Authenticator app (TOTP) card in Settings → Account.
 * @param {HTMLElement|null} el
 */
async function totpCard(el) {
  if (!el) return;
  let st;
  try { st = await Server.account.totp(); } catch { el.innerHTML = ""; return; }
  if (!st.here) { el.innerHTML = ""; return; }
  const draw = (pending = null) => {
    el.innerHTML = `<h3>Two-factor authentication</h3>
      <div class="card" data-totp-card>
        <p class="small">${st.enrolled ? `<span class="chip ok">Enrolled</span> Your account asks for a 6-digit code from your authenticator app at sign-in.` : `Add an authenticator app (1Password, Google Authenticator, Apple Passwords) for a 6-digit second factor at sign-in.`}</p>
        ${pending ? `<div class="stack" data-totp-setup>
          <p class="small">Enter this key in your authenticator app or open <a href="${esc(pending.uri)}">the setup link</a>:</p>
          <input readonly class="mono" value="${esc(pending.secret)}" aria-label="Authenticator secret key" data-totp-secret>
          <label for="totp-code">6-digit code from your app</label>
          ${st.enrolled ? `<label for="totp-password">Current password</label>
          <input id="totp-password" type="password" autocomplete="current-password" data-totp-password>` : ""}
          <div class="row"><input id="totp-code" data-totp-code inputmode="numeric" pattern="[0-9]{6}" maxlength="6" placeholder="123456"><button type="button" class="primary" data-totp-confirm>Verify and turn on</button></div>
        </div>` : `${st.enrolled && !st.code_required ? `<label for="totp-password">Current password, to remove it</label>
        <input id="totp-password" type="password" autocomplete="current-password" data-totp-password>` : ""}<div class="row">
          <button type="button" class="${st.enrolled ? "" : "primary"}" data-totp-begin>${st.enrolled ? "Replace authenticator app" : "Set up authenticator app"}</button>
          ${st.enrolled && !st.code_required ? `<button type="button" class="danger" data-totp-remove>Remove authenticator app</button>` : ""}
        </div>`}
      </div>`;
    const beginBtn = $("[data-totp-begin]", el);
    if (beginBtn) beginBtn.onclick = async () => {
      try { draw(await Server.account.beginTotp()); }
      catch (err) { toast(err.message || String(err)); }
    };
    const confirmBtn = $("[data-totp-confirm]", el);
    if (confirmBtn) confirmBtn.onclick = async () => {
      const code = /** @type {HTMLInputElement} */ ($("[data-totp-code]", el))?.value.trim() || "";
      const password = /** @type {HTMLInputElement} */ ($("[data-totp-password]", el))?.value || "";
      try {
        await Server.account.confirmTotp(code, password);
        st.enrolled = true;
        toast("Authenticator app enrolled.");
        draw(null);
      } catch (err) { toast(err.message || String(err)); }
    };
    const removeBtn = $("[data-totp-remove]", el);
    if (removeBtn) removeBtn.onclick = async () => {
      if (typeof ask === "function" && !await ask("Remove your authenticator app? Your account will sign in with your password alone.", "Remove", true)) return;
      const password = /** @type {HTMLInputElement} */ ($("[data-totp-password]", el))?.value || "";
      if (!password) { toast("Enter your current password to remove it."); return; }
      try {
        await Server.account.removeTotp(password);
        st.enrolled = false;
        toast("Authenticator app removed.");
        draw(null);
      } catch (err) { toast(err.message || String(err)); }
    };
  };
  draw(null);
}

/**
 * Time zone and working hours card in Settings → Account.
 * @param {HTMLElement|null} el
 */
async function timezoneCard(el) {
  if (!el) return;
  let prefs;
  try { prefs = (typeof App !== "undefined" && App?.prefs) || await Server.preferences.get(); } catch { el.innerHTML = ""; return; }
  const current = prefs?.timezone || "UTC";
  const detected = deviceTimezone();
  const zones = Array.from(new Set([current, detected, ...COMMON_TIMEZONES]));
  el.innerHTML = `<h3>Time zone</h3>
    <div class="card" data-timezone-card>
      <p class="small muted">Schedules, morning briefings and quiet hours follow this time zone.</p>
      <div class="row">
        <select data-tz-select aria-label="Time zone">${zones.map(z => `<option value="${esc(z)}"${z === current ? " selected" : ""}>${esc(z)}</option>`).join("")}</select>
        ${detected && detected !== current ? `<button type="button" data-tz-detect>Use device time zone (${esc(detected)})</button>` : ""}
      </div>
    </div>`;
  const sel = /** @type {HTMLSelectElement} */ ($("[data-tz-select]", el));
  if (sel) sel.onchange = async () => {
    try {
      const updated = await Server.preferences.set({ timezone: sel.value });
      if (typeof App !== "undefined") App.prefs = updated;
      toast(`Time zone set to ${sel.value}.`);
    } catch (err) { toast(err.message || String(err)); }
  };
  const det = $("[data-tz-detect]", el);
  if (det) det.onclick = async () => {
    try {
      const updated = await Server.preferences.set({ timezone: detected });
      if (typeof App !== "undefined") App.prefs = updated;
      toast(`Time zone set to ${detected}.`);
      await timezoneCard(el);
    } catch (err) { toast(err.message || String(err)); }
  };
}

/**
 * Self-service data export and immediate account erasure card in Settings → Account.
 * @param {HTMLElement|null} el
 */
function privacyAccountCard(el) {
  if (!el) return;
  el.innerHTML = `<h3>Your data and account</h3>
    <div class="card" data-privacy-account-card>
      <p class="small muted">Download a copy of your chats, tasks, files metadata, preferences and memory, or permanently erase your account and data from this server.</p>
      <div class="row">
        <button type="button" data-account-export>Download my data (JSON)</button>
        <button type="button" class="danger" data-account-erase>Delete my account and data</button>
      </div>
    </div>`;
  const exp = $("[data-account-export]", el);
  if (exp) exp.onclick = async () => {
    try {
      const bundle = await Server.account.exportData();
      const blob = new Blob([JSON.stringify(bundle, null, 2)], { type: "application/json" });
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = `magnus-data-${bundle.person || "export"}.json`;
      a.click();
      URL.revokeObjectURL(a.href);
      toast("Downloaded your data export.");
    } catch (err) { toast(err.message || String(err)); }
  };
  const erase = $("[data-account-erase]", el);
  if (erase) erase.onclick = async () => {
    const typed = typeof askText === "function"
      ? await askText({ title: "Delete your account and data?", body: "Your chats, tasks, files, schedules, computers and memory will be permanently erased and your devices signed out. Type DELETE to confirm.", placeholder: "DELETE", okLabel: "Erase my account", danger: true })
      : null;
    if (typed !== "DELETE") return;
    try {
      await Server.account.deleteAccount("DELETE");
      toast("Your account and data have been erased.");
      $("nav.side [data-act=sign-out]")?.click();
    } catch (err) { toast(err.message || String(err)); }
  };
}

/**
 * Populate the Authenticator, Time zone, and Data & Account cards in Settings → Account.
 * @param {HTMLElement} root
 */
async function accountExtras(root) {
  await Promise.all([
    totpCard($("[data-totp]", root)),
    timezoneCard($("[data-timezone]", root)),
  ]);
  privacyAccountCard($("[data-account-privacy]", root));
}

