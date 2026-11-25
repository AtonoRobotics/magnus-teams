// Aurora's decisions and alerts are part of the stage, not over it
// (decisions/2026-11-21-aurora-alerts.md):
// - a decision in a conversation is made beside it, in the canvas, or in its
//   box under it (aurora-stage.js); the thread keeps one quiet line for it,
//   "Needs your decision", then "✓ Allowed" once answered;
// - answered in the canvas, the card says the answer where it was, as the
//   mockup's approve button turns to "✓ Sent & verified", and the canvas
//   then slides away;
// - in a list of decisions, one arriving rises in, and one answered says
//   the answer, then folds away;
// - a toast, where there is no stage to say it (aurora-reasoning.js `tell`),
//   slides out when it goes.
// app.js and asks.js remove and redraw at once, and what they removed is
// played out here. An answer shows once the server took it: app.js tells
// "magnus:answered" then (decisions/2026-11-23-answer-shown-once-taken.md).
"use strict";

/** How long an answered decision shows its answer before it goes. */
const ALERT_CONFIRM_MS = 900;
/** How long a toast or an answered decision takes to go. */
const ALERT_LEAVE_MS = 420;
/** How long an answer is waited on to take its card away. */
const ALERT_ANSWER_WAIT_MS = 15000;
/** How many decisions met are remembered, so each rises in once. */
const ALERT_SEEN_MAX = 2000;

const AuroraAlerts = (() => {
  const EASE = "cubic-bezier(.22,1,.36,1)";
  const ANSWERED = { approve: "Allowed", reject: "Declined", answer: "Sent", accept: "Accepted", dismiss: "Dismissed", resume: "Continuing", extend: "Budget doubled", keep: "Kept", disconnect: "Disconnected", endorse: "Saved as a company fact" };
  const DECISION = ".card.item:has(.ask-answers)";
  /** Where a conversation's decision cards are: the stage shows them instead. */
  const IN_THREAD = "section.conv .card.item[data-id]";

  const on = () => typeof Theme !== "undefined" && typeof Theme.stage === "function" && Theme.stage(document.documentElement.dataset.style);
  const moving = () => document.documentElement.dataset.motion === "full";
  /** What an answer is called once given. @param {string} decision */
  const said = decision => ANSWERED[decision] || "Done";

  /**
   * A node played out after its owner removed it: a copy where it was,
   * that nobody can reach, folding to nothing.
   * @param {HTMLElement} node @param {Node} parent @param {?Node} before
   * @param {Keyframe} gone how it leaves @param {number} [delay]
   */
  function playOut(node, parent, before, gone, delay = 0) {
    const ghost = /** @type {HTMLElement} */ (node.cloneNode(true));
    ghost.classList.add("aurora-leaving");
    ghost.removeAttribute("id");
    ghost.querySelectorAll("[id]").forEach(el => el.removeAttribute("id"));
    ghost.setAttribute("aria-hidden", "true");
    ghost.inert = true;
    parent.insertBefore(ghost, before && before.parentNode === parent ? before : null);
    const s = getComputedStyle(ghost), h = ghost.offsetHeight;
    const gap = parseFloat(getComputedStyle(/** @type {Element} */ (parent)).rowGap) || 0;
    const anim = ghost.animate([
      { height: `${h}px`, paddingTop: s.paddingTop, paddingBottom: s.paddingBottom, marginBottom: s.marginBottom, opacity: 1, transform: "none" },
      { height: "0px", paddingTop: "0px", paddingBottom: "0px", marginBottom: `${-gap}px`, borderWidth: "0px", ...gone },
    ], { duration: ALERT_LEAVE_MS, delay, easing: EASE, fill: "both" });
    anim.onfinish = () => ghost.remove();
    anim.oncancel = () => ghost.remove();
    return ghost;
  }

  /** A toast that went slides out where it was. @param {MutationRecord} r */
  function leftToast(r) {
    const box = /** @type {HTMLElement} */ (r.target);
    if (box.id !== "toasts" || !box.isConnected) return;
    for (const n of r.removedNodes) {
      if (n instanceof HTMLElement && n.matches(".toast:not(.aurora-leaving)")) playOut(n, box, r.nextSibling, { opacity: 0, transform: "translateY(-8px) scale(.98)" });
    }
  }

  // ---------- Answers ----------
  /** Each decision answered in this session, by its item, as what was said. @type {Map<string, string>} */
  const given = new Map();
  /**
   * A card just answered, until it goes.
   * @type {Map<string, { card: HTMLElement, said: string, canvas: boolean, head: string, entry: string, after: string, before: string, at: number }>}
   */
  const answering = new Map();

  // An answer is what the server took, not the tap: app.js's respond() tells
  // "magnus:answered" once the server accepted it. One refused (answered by
  // someone else first, expired, the network) is never said here.
  document.addEventListener("magnus:answered", e => {
    const { item, decision, button } = /** @type {CustomEvent<{ item: string, decision: string, button?: HTMLElement }>} */ (e).detail || {};
    if (!on() || !item || !decision || decision === "ask-more") return;
    // The thread says it once the card goes, at the app's redraw.
    given.set(item, said(decision));
    const card = button?.closest?.(DECISION);
    if (!(card instanceof HTMLElement) || !card.dataset.id) return;
    answering.set(card.dataset.id, {
      card,
      said: said(decision),
      canvas: !!card.closest(".stage-artifact"),
      // The canvas built around the decision keeps its question as its
      // title while it says the answer (2026-11-21-aurora-canvas.md).
      head: card.closest(".stage-decision")?.querySelector(":scope > .artifact-top")?.outerHTML || "",
      entry: card.closest("[data-entry]")?.getAttribute("data-entry") || "",
      after: card.previousElementSibling?.getAttribute("data-id") || "",
      before: card.nextElementSibling?.getAttribute("data-id") || "",
      at: Date.now(),
    });
  });

  /** The card as answered: its answers become what was said. @param {HTMLElement} card @param {string} words */
  function confirmed(card, words) {
    card.classList.add("aurora-confirmed");
    const answers = card.querySelector(".ask-answers");
    if (!answers) return;
    const done = document.createElement("div");
    done.className = "aurora-confirmed-row";
    done.innerHTML = '<span class="aurora-confirmed-pill"></span><span class="aurora-confirmed-note">Magnus has your answer.</span>';
    const pill = done.firstElementChild;
    if (pill) pill.textContent = `✓ ${words}`;
    answers.replaceWith(done);
  }

  /**
   * Answered in the canvas: the card stays there saying the answer, then the
   * canvas slides away and the stage shows what is next.
   * @param {string} id @param {{ card: HTMLElement, said: string }} a
   */
  function answeredBeside(id, a) {
    const content = document.querySelector(".stage-artifact > .artifact-content");
    if (!content || typeof Panel === "undefined" || Panel.what !== `item:${id}`) return;
    confirmed(a.card, a.said);
    content.replaceChildren(a.card);
    const stage = content.parentElement;
    const top = stage?.querySelector(":scope > .artifact-top");
    if (a.head && stage && top) {
      stage.classList.add("stage-decision");
      top.outerHTML = a.head;
      const title = stage.querySelector(":scope > .artifact-top h2")?.textContent;
      if (title) stage.setAttribute("aria-label", title);
    }
    setTimeout(() => {
      if (Panel.what !== `item:${id}` || typeof closePanel !== "function") return;
      closePanel(false);
      if (typeof AuroraStage !== "undefined") AuroraStage.refresh();
    }, ALERT_CONFIRM_MS);
  }

  // ---------- Decisions in a list ----------
  /** Each decision met, so one rises in only the first time it shows, and
   * only while the person stays where they are: what a new route or the
   * first drawing shows was there already. */
  const seen = new Set();
  /** @type {?string} */
  let route = null;

  /** Decisions arriving rise in; answered ones gone from the page are played out. */
  function decisions() {
    const cards = /** @type {HTMLElement[]} */ ([...document.querySelectorAll(DECISION)]).filter(c => c.dataset.id && !c.classList.contains("aurora-leaving"));
    for (const c of cards) {
      const id = c.dataset.id || "";
      if (seen.has(id)) continue;
      if (seen.size > ALERT_SEEN_MAX) seen.clear();
      seen.add(id);
      if (route === location.hash && moving() && !c.closest(".stage-artifact")) c.classList.add("aurora-alert-in");
    }
    if (document.querySelector("#main > *")) route = location.hash;
    for (const [id, a] of answering) {
      const late = Date.now() - a.at > ALERT_ANSWER_WAIT_MS;
      if (late || cards.some(c => c.dataset.id === id)) {
        if (late) answering.delete(id);
        continue;
      }
      if (a.card.isConnected) continue;
      answering.delete(id);
      if (a.canvas) { answeredBeside(id, a); continue; }
      if (!moving()) continue;
      // Where the card was: after the decision before it, before the one
      // after it, or at the end of its entry.
      const card = (/** @type {string} */ k) => k ? document.querySelector(`${DECISION}[data-id="${CSS.escape(k)}"]:not(.aurora-leaving)`) : null;
      const prev = card(a.after), next = prev ? null : card(a.before);
      const entry = a.entry ? document.querySelector(`[data-entry="${CSS.escape(a.entry)}"]`) : null;
      const parent = prev?.parentNode || next?.parentNode || entry;
      if (!parent) continue;
      confirmed(a.card, a.said);
      playOut(a.card, parent, prev ? prev.nextSibling : next, { opacity: 0, transform: "scale(.98)" }, ALERT_CONFIRM_MS);
    }
  }

  // ---------- The thread's line for each decision ----------
  /** Each decision's line in its conversation. @type {Map<string, { el: HTMLElement, entry: string }>} */
  const lines = new Map();

  /** @param {string} id */
  function line(id) {
    let l = lines.get(id);
    if (l) return l;
    const el = document.createElement("div");
    el.className = "aurora-decision-line";
    el.dataset.item = id;
    el.innerHTML = '<span class="aurora-decision-dot" aria-hidden="true"></span><span class="aurora-decision-said"></span><span class="aurora-decision-what"></span><span class="aurora-decision-meta"></span><a class="aurora-decision-open" href="#/inbox">Review</a>';
    l = { el, entry: "" };
    lines.set(id, l);
    return l;
  }

  /** @param {HTMLElement} el @param {string} cls @param {string} text */
  function words(el, cls, text) {
    const t = el.querySelector(cls);
    if (t && t.textContent !== text) t.textContent = text;
  }

  /**
   * A conversation's decision cards are shown by the stage; in their place
   * the thread keeps a line: waiting (with tier/destination context), then the answer given.
   */
  function threadLines() {
    for (const card of /** @type {NodeListOf<HTMLElement>} */ (document.querySelectorAll(IN_THREAD))) {
      const id = card.dataset.id || "";
      const l = line(id);
      l.entry = card.closest("[data-entry]")?.getAttribute("data-entry") || l.entry;
      l.el.dataset.state = "waiting";
      words(l.el, ".aurora-decision-said", "Needs your decision");
      words(l.el, ".aurora-decision-what", card.querySelector(".ask-sentence, h4, strong")?.textContent?.trim() || "");
      const metaBits = [
        card.querySelector(".ask-tier")?.textContent?.trim(),
        ...[...card.querySelectorAll(".ask-preview-row")].slice(0, 2).map(r => r.textContent?.replace(/\s+/g, " ").trim()),
        card.querySelector(".consequence")?.textContent?.trim(),
      ].filter(Boolean);
      words(l.el, ".aurora-decision-meta", metaBits.join(" · "));
      const open = l.el.querySelector(".aurora-decision-open");
      open?.setAttribute("data-open", `item:${id}`);
      if (card.previousElementSibling !== l.el) card.before(l.el);
    }
    for (const [id, l] of lines) {
      if (document.querySelector(`${IN_THREAD}[data-id="${CSS.escape(id)}"]`)) continue;
      const answer = given.get(id);
      // Gone without an answer from here: answered elsewhere, or withdrawn.
      if (!answer) { l.el.remove(); lines.delete(id); continue; }
      l.el.dataset.state = "answered";
      words(l.el, ".aurora-decision-said", `✓ ${answer}`);
      words(l.el, ".aurora-decision-meta", "");
      l.el.querySelector(".aurora-decision-open")?.remove();
      const entry = l.entry ? document.querySelector(`[data-entry="${CSS.escape(l.entry)}"]`) : null;
      if (entry && l.el.parentElement !== entry) entry.append(l.el);
      else if (!entry) l.el.remove();
    }
  }

  /** Leaves the page as the other styles have it. */
  function off() {
    document.querySelectorAll(".aurora-decision-line").forEach(el => el.remove());
    lines.clear();
  }

  let queued = 0;
  const soon = () => { if (!queued) queued = requestAnimationFrame(() => { queued = 0; if (on()) decisions(); }); };
  function handleDom(/** @type {MutationRecord[]} */ records) {
    if (!on()) { if (lines.size) off(); return; }
    // Played out and put in place in this callback, before the browser
    // draws the change.
    if (moving()) records.forEach(leftToast);
    if (records.some(r => r.addedNodes.length || r.removedNodes.length)) { threadLines(); soon(); }
  }
  function init() {
    if (typeof AuroraCore !== "undefined") AuroraCore.onDom(handleDom);
    else new MutationObserver(handleDom).observe(document.body, { childList: true, subtree: true });
    new MutationObserver(() => { if (on()) threadLines(); else off(); }).observe(document.documentElement, { attributes: true, attributeFilter: ["data-style"] });
    soon();
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();

  return { on, decisions, said, seen, answering, given, lines };
})();
