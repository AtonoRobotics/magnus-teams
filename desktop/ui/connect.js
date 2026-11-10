// Connecting an app grants its use (spec rev 140, "Permissions and tasks";
// decisions/2026-11-10-connect-consent.md). Connecting shows one screen:
// what Magnus can read in the app, what it can change and send, and the
// risks, in plain words. Accepting it lets Magnus use every tool the app
// offers; no tool is reviewed on its own, and no description is shown. A
// third-party server that later adds a tool that can change or send asks
// once, for the whole app, on one card: keep it, or disconnect.

/**
 * The connect screen as the server words it (GET /v1/connectors/{name}/terms).
 * @typedef {{ app: string, title: string, reads: string[], changes: string[], risks: string[], cannot: string|null, terms: string }} ConnectTerms
 */
/**
 * The per-app card's details: the app and its new tools that change or send.
 * @typedef {{ app: string, title: string, tools: { tool: string, risk: string, does: string }[] }} AppToolsDetails
 */

/** One part of the screen: a heading and its lines, or what it says when there are none. */
function connectPart(head, lines, none) {
  return `<h3>${esc(head)}</h3>${lines.length ? `<ul class="cp-list">${lines.map(l => `<li>${esc(l)}</li>`).join("")}</ul>` : `<p class="small muted">${esc(none)}</p>`}`;
}

/**
 * The screen itself: three parts and two buttons. Nothing per tool to tick.
 * @param {ConnectTerms} t
 */
function connectScreenHtml(t) {
  return `<div class="connect-screen" data-connect="${esc(t.app)}"><h2>Connect ${esc(t.title)}</h2>
    ${connectPart("What Magnus can read", t.reads || [], "Nothing to read.")}
    ${connectPart("What it can change and send", t.changes || [], "Nothing: it only reads.")}
    ${t.cannot ? `<p class="small">It cannot: ${esc(t.cannot)}.</p>` : ""}
    ${connectPart("The risks", t.risks || [], "None to speak of.")}
    <p class="small muted">Connecting lets Magnus use all of this when your requests need it. You can disconnect at any time.</p>
    <div class="row"><button data-connect-answer="no">Back</button><button class="primary" data-connect-answer="yes">Connect</button></div></div>`;
}

/**
 * Show the screen; true when the person accepts it.
 * @param {ConnectTerms} t
 * @returns {Promise<boolean>}
 */
function connectScreen(t) {
  return new Promise(done => {
    const d = document.createElement("dialog");
    d.className = "ask connect";
    d.innerHTML = connectScreenHtml(t);
    d.addEventListener("click", e => { const b = e.target.closest("[data-connect-answer]"); if (b) d.close(b.dataset.connectAnswer); });
    d.addEventListener("close", () => { d.remove(); done(d.returnValue === "yes"); });
    document.body.append(d);
    d.showModal();
    d.querySelector("[data-connect-answer=no]").focus();
  });
}

/**
 * Read the screen, show it, and record the consent. False: they went back.
 * @param {string} name
 */
async function consentToConnect(name) {
  /** @type {ConnectTerms} */
  const t = await api("GET", `/v1/connectors/${enc(name)}/terms`);
  if (!await connectScreen(t)) return false;
  await api("POST", `/v1/connectors/${enc(name)}/consent`, { terms: t.terms });
  return true;
}

/**
 * The one card for an app's new tools that change or send: what they do in
 * plain words, never their descriptions; keep them, or disconnect the app.
 * @param {{ id: string, can_respond: boolean, details: AppToolsDetails }} i
 */
function appToolsCard(i) {
  const d = i.details || /** @type {AppToolsDetails} */ ({ app: "", title: "", tools: [] });
  return `<ul class="cp-list" data-app-tools="${esc(d.app)}">${(d.tools || []).map(t => `<li>${esc(t.does || t.tool)}</li>`).join("")}</ul>
    <p class="small muted">Keeping lets Magnus use ${d.tools?.length === 1 ? "it" : "them"} as it uses the rest of ${esc(d.title || d.app)}. Disconnecting removes ${esc(d.title || d.app)} for everyone.</p>
    ${buttons(i, [["keep", "Keep it", "primary"], ["disconnect", `Disconnect ${d.title || d.app}`]])}`;
}
