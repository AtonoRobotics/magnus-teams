// Aurora's alerts move as the mockup's do (decisions/2026-11-21-aurora-alerts.md):
// - a pop-up in the lower corner, and a toast, slide out when they go and the
//   rest glide into the room left, instead of vanishing in one frame; a
//   pop-up shows the time it has left, held while the pointer or focus is on
//   it, and a dot for what it is about;
// - a decision arriving rises in; answered, it shows the answer where it was,
//   as the mockup's approve button turns to "✓ Sent & verified", then folds
//   away, instead of the card disappearing with the next repaint.
// notices.js, app.js and asks.js are unchanged: they remove and redraw at
// once, and what they removed is played out here.
"use strict";

/** How long an answered decision shows its answer before folding. */
const ALERT_CONFIRM_MS = 900;
/** How long a pop-up, a toast or an answered decision takes to go. */
const ALERT_LEAVE_MS = 420;
/** How long an answer is waited on to take its card away. */
const ALERT_ANSWER_WAIT_MS = 15000;
/** How many decisions met are remembered, so each rises in once. */
const ALERT_SEEN_MAX = 2000;

const AuroraAlerts = (() => {
  const EASE = "cubic-bezier(.22,1,.36,1)";
  const ANSWERED = { approve: "Allowed", reject: "Declined", answer: "Sent", accept: "Accepted", dismiss: "Dismissed", resume: "Continuing", extend: "Budget doubled", keep: "Kept", disconnect: "Disconnected", endorse: "Saved as a company fact" };
  const DECISION = ".card.item:has(.ask-answers)";

  const on = () => typeof Theme !== "undefined" && typeof Theme.stage === "function" && Theme.stage(document.documentElement.dataset.style);
  const moving = () => document.documentElement.dataset.motion === "full";

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

  // ---------- Pop-ups and toasts ----------
  /** @param {MutationRecord} r */
  function leftCorner(r) {
    const box = /** @type {HTMLElement} */ (r.target);
    if (box.id !== "notices" && box.id !== "toasts") return;
    for (const n of r.removedNodes) {
      if (!(n instanceof HTMLElement) || n.classList.contains("aurora-leaving") || !n.matches(".notice, .toast") || !box.isConnected) continue;
      playOut(n, box, r.nextSibling, box.id === "notices" ? { opacity: 0, transform: "translateX(28px) scale(.98)" } : { opacity: 0, transform: "translateY(-8px) scale(.98)" });
    }
    for (const n of r.addedNodes) if (n instanceof HTMLElement && n.matches(".notice:not(.aurora-leaving)")) arrived(n);
  }

  /** A pop-up's time line restarts with notices.js's timer when the person lets go. @param {HTMLElement} el */
  function arrived(el) {
    const box = el.parentElement;
    if (box && typeof NOTICE_SHOW_MS === "number") box.style.setProperty("--aurora-notice-life", `${NOTICE_SHOW_MS}ms`);
    const restart = () => document.getAnimations().forEach(a => {
      if (/** @type {any} */ (a).animationName === "auroraNoticeLife" && /** @type {KeyframeEffect} */ (a.effect)?.target === el) a.currentTime = 0;
    });
    el.addEventListener("pointerleave", restart);
    el.addEventListener("focusout", e => { if (!el.contains(/** @type {Node} */ (/** @type {FocusEvent} */ (e).relatedTarget))) restart(); });
  }

  // ---------- Decisions ----------
  /** Each decision met, so one rises in only the first time it shows, and
   * only while the person stays where they are: what a new route or the
   * first drawing shows was there already. */
  const seen = new Set();
  /** @type {?string} */
  let route = null;
  /**
   * A decision just answered, until its card goes.
   * @type {Map<string, { card: HTMLElement, said: string, entry: string, after: string, before: string, at: number }>}
   */
  const answering = new Map();

  document.addEventListener("click", e => {
    const btn = /** @type {HTMLElement} */ (e.target).closest?.(".ask-answers button[data-decision]");
    const card = btn?.closest(DECISION);
    if (!on() || !btn || !(card instanceof HTMLElement) || !card.dataset.id) return;
    const decision = (/** @type {HTMLElement} */ (btn).dataset.decision || "").split(":")[0];
    if (decision === "ask-more") return;
    answering.set(card.dataset.id, {
      card,
      said: ANSWERED[decision] || "Done",
      entry: card.closest("[data-entry]")?.getAttribute("data-entry") || "",
      after: card.previousElementSibling?.getAttribute("data-id") || "",
      before: card.nextElementSibling?.getAttribute("data-id") || "",
      at: Date.now(),
    });
  }, true);

  /** Decisions arriving rise in; answered ones gone from the page are played out. */
  function decisions() {
    const cards = /** @type {HTMLElement[]} */ ([...document.querySelectorAll(DECISION)]).filter(c => c.dataset.id && !c.classList.contains("aurora-leaving"));
    for (const c of cards) {
      const id = c.dataset.id || "";
      if (seen.has(id)) continue;
      if (seen.size > ALERT_SEEN_MAX) seen.clear();
      seen.add(id);
      if (route === location.hash && moving()) c.classList.add("aurora-alert-in");
    }
    if (document.querySelector("#main > *")) route = location.hash;
    for (const [id, a] of answering) {
      if (Date.now() - a.at > ALERT_ANSWER_WAIT_MS || cards.some(c => c.dataset.id === id)) {
        if (Date.now() - a.at > ALERT_ANSWER_WAIT_MS) answering.delete(id);
        continue;
      }
      if (a.card.isConnected) continue;
      answering.delete(id);
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

  /** The card as answered: its answers become what was said. @param {HTMLElement} card @param {string} said */
  function confirmed(card, said) {
    card.classList.add("aurora-confirmed");
    const answers = card.querySelector(".ask-answers");
    if (!answers) return;
    const done = document.createElement("div");
    done.className = "aurora-confirmed-row";
    done.innerHTML = '<span class="aurora-confirmed-pill"></span><span class="aurora-confirmed-note">Magnus has your answer.</span>';
    const pill = done.firstElementChild;
    if (pill) pill.textContent = `✓ ${said}`;
    answers.replaceWith(done);
  }

  let queued = 0;
  const soon = () => { if (!queued) queued = requestAnimationFrame(() => { queued = 0; if (on()) decisions(); }); };
  function init() {
    new MutationObserver(records => {
      if (!on()) return;
      // Played out in this callback, before the browser draws the removal.
      if (moving()) records.forEach(leftCorner);
      if (records.some(r => r.addedNodes.length || r.removedNodes.length)) soon();
    }).observe(document.body, { childList: true, subtree: true });
    soon();
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();

  return { on, decisions, seen, answering };
})();
