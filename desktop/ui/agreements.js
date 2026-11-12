// The agreements between a person and whoever runs the server (owner,
// 2026-11-11): the Terms of Use and Privacy notice, accepted before the app
// opens and again whenever their version changes; the question whether to
// share anonymous usage data, asked once, with two answers of equal weight
// and none chosen beforehand; and the tester program, joined only by
// accepting the Tester Terms. Settings › Agreements changes any of it.
"use strict";

/**
 * @typedef {{ heading: string, paragraphs: string[] }} AgreementSection
 * @typedef {{ id: string, title: string, version: string, path: string, sections: AgreementSection[], attribution: string }} AgreementDoc
 * @typedef {{ version: string, title: string, text: string[], yes: string, no: string }} UsagePrompt
 * @typedef {{
 *   terms_due: boolean, usage_due: boolean,
 *   usage: { share: boolean, current: boolean } | null,
 *   tester: { version: string, current: boolean } | null,
 *   invited_by: string | null,
 *   documents: { terms: AgreementDoc, privacy: AgreementDoc, testers: AgreementDoc },
 *   usage_prompt: UsagePrompt, contact: string
 * }} AgreementStanding
 */

/** A document as the app shows it. @param {AgreementDoc} d */
function agreementDocHtml(d) {
  return `<article class="agreement" data-agreement="${esc(d.id)}"><h2>${esc(d.title)}</h2><p class="small faint">Version ${esc(d.version)}</p>
    ${d.sections.map(s => `<h3>${esc(s.heading)}</h3>${s.paragraphs.map(p => `<p>${esc(p)}</p>`).join("")}`).join("")}
    <p class="small faint">${esc(d.attribution)}</p></article>`;
}

/** Ask with a dialog whose buttons are `choices` ([value, label]), all
 * alike. Escape answers `escape` when given, and is refused when not.
 * @param {string} label @param {string} body @param {[string, string][]} choices @param {string} [escape] @returns {Promise<string>} */
function agreementAsk(label, body, choices, escape) {
  return new Promise(done => {
    const d = document.createElement("dialog");
    d.className = "ask agreement-ask";
    d.setAttribute("aria-label", label);
    d.innerHTML = `<div class="agreement-body" tabindex="-1">${body}</div><div class="row" data-agreement-choices>${choices.map(([v, l]) => `<button type="button" data-agreement-choice="${esc(v)}">${esc(l)}</button>`).join("")}</div>`;
    d.addEventListener("cancel", e => { if (escape === undefined) e.preventDefault(); });
    d.addEventListener("click", e => { const b = /** @type {HTMLElement} */ (e.target).closest("[data-agreement-choice]"); if (b) d.close(b.getAttribute("data-agreement-choice") || ""); });
    d.addEventListener("close", () => { d.remove(); done(d.returnValue || escape || ""); });
    document.body.append(d);
    d.showModal();
    // Neither answer is focused, so neither is the default.
    /** @type {HTMLElement} */ ($(".agreement-body", d)).focus();
  });
}

/** Send one answer; a changed text is read again. @returns {Promise<AgreementStanding|null>} */
async function agreementSend(path, body) {
  try { return await api("POST", path, body); }
  catch (err) { toast(err.body?.error || err.message); return null; }
}

/** What is due before the app opens. Resolves false when the person
 * would rather sign out than accept the Terms of Use. */
async function agreementsGate() {
  for (;;) {
    /** @type {AgreementStanding} */
    const st = await api("GET", "/v1/legal");
    const docs = st.documents;
    if (st.terms_due) {
      const body = `<p>Before you use Magnus, read and accept the Terms of Use. The Privacy notice below says what is kept and who sees it.</p>${agreementDocHtml(docs.terms)}${agreementDocHtml(docs.privacy)}`;
      const a = await agreementAsk("Terms of Use", body, [["accept", "I accept"], ["decline", "Sign out"]]);
      if (a !== "accept") return false;
      await agreementSend("/v1/legal/terms", { terms: docs.terms.version, privacy: docs.privacy.version });
      continue;
    }
    if (st.usage_due) {
      const p = st.usage_prompt;
      const body = `<h2>${esc(p.title)}</h2>${p.text.map(t => `<p>${esc(t)}</p>`).join("")}`;
      const a = await agreementAsk(p.title, body, [["yes", p.yes], ["no", p.no]], "later");
      if (a === "later") return true;
      await agreementSend("/v1/legal/usage", { share: a === "yes", version: p.version });
      continue;
    }
    if (st.tester && !st.tester.current) {
      const body = `<p>The Tester Terms changed. Accept them to stay in the tester program, or leave it.</p>${agreementDocHtml(docs.testers)}`;
      const a = await agreementAsk("Tester Terms", body, [["join", "Accept and stay"], ["leave", "Leave the program"]], "later");
      if (a === "later") return true;
      await agreementSend(a === "join" ? "/v1/legal/testers/join" : "/v1/legal/testers/leave", { version: docs.testers.version });
      continue;
    }
    if (st.invited_by && !st.tester) {
      const body = `<p>${esc(st.invited_by)} invited you to the tester program. Read the Tester Terms: as a tester, Magnus's developers see your work in full.</p>${agreementDocHtml(docs.testers)}`;
      const a = await agreementAsk("Tester program", body, [["join", "Join the tester program"], ["decline", "Decline"]], "later");
      if (a === "later") return true;
      await agreementSend(a === "join" ? "/v1/legal/testers/join" : "/v1/legal/testers/decline", { version: docs.testers.version });
      continue;
    }
    return true;
  }
}

/** Settings › Agreements. @param {HTMLElement} el */
async function agreementsSection(el) {
  const draw = async () => {
    /** @type {AgreementStanding} */
    const st = await api("GET", "/v1/legal");
    const d = st.documents, u = st.usage && st.usage.current ? st.usage.share : null;
    const tester = st.tester && st.tester.current;
    paint(el, `<h2>Agreements</h2>
      <div class="card" data-agreement-usage><h3>${esc(st.usage_prompt.title)}</h3>${st.usage_prompt.text.map(t => `<p class="small muted">${esc(t)}</p>`).join("")}
        <p class="small">${u === null ? "You have not answered." : u ? "You share anonymous usage data." : "You do not share usage data."}</p>
        <div class="row"><button data-agreement-usage-set="yes"${u === true ? ' aria-pressed="true"' : ""}>${esc(st.usage_prompt.yes)}</button><button data-agreement-usage-set="no"${u === false ? ' aria-pressed="true"' : ""}>${esc(st.usage_prompt.no)}</button></div></div>
      <div class="card" data-agreement-testers><h3>Tester program</h3>
        <p class="small muted">${tester ? "You are a tester: Magnus's developers see your work in full. Leaving applies from now on." : "Testers let Magnus's developers see their work in full to find and fix bugs. Joining means accepting the Tester Terms."}</p>
        <details><summary>Read the Tester Terms</summary>${agreementDocHtml(d.testers)}</details>
        <div class="row">${tester ? `<button data-agreement-tester="leave">Leave the tester program</button>` : `<button data-agreement-tester="join">Accept the Tester Terms and join</button>`}</div></div>
      <div class="card" data-agreement-docs><h3>Terms of Use and Privacy notice</h3>
        <p class="small muted">You accepted version ${esc(d.terms.version)} of the Terms of Use. Both are also at ${esc(d.terms.path)} and ${esc(d.privacy.path)} on this server. Questions: ${esc(st.contact)}.</p>
        <details><summary>Read the Terms of Use</summary>${agreementDocHtml(d.terms)}</details>
        <details><summary>Read the Privacy notice</summary>${agreementDocHtml(d.privacy)}</details></div>`);
    el.onclick = async e => {
      const t = /** @type {HTMLElement} */ (e.target);
      const usage = t.closest("[data-agreement-usage-set]"), program = t.closest("[data-agreement-tester]");
      if (usage) {
        const share = usage.getAttribute("data-agreement-usage-set") === "yes";
        if (await agreementSend("/v1/legal/usage", { share, version: st.usage_prompt.version })) toast(share ? "You share anonymous usage data from now on." : "Nothing of yours reaches the developers from now on.");
      } else if (program) {
        const join = program.getAttribute("data-agreement-tester") === "join";
        if (join && !(await ask("Join the tester program? Magnus's developers will see your work in full, as the Tester Terms say.", "Join"))) return;
        if (await agreementSend(join ? "/v1/legal/testers/join" : "/v1/legal/testers/leave", { version: d.testers.version })) toast(join ? "You are in the tester program." : "You left the tester program.");
      } else return;
      await draw();
    };
  };
  await draw();
}
