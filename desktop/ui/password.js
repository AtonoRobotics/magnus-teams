// A person's own password: changing it in Settings → Account, and the
// "Forgot password?" step of signing in. Uses app.js's helpers ($, esc,
// paint, api, toast) and the bridge's forgotPassword.
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
  try { rules = await api("GET", "/v1/account/password"); }
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
      const r = await api("POST", "/v1/account/password", { current_password: current, new_password: next });
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
