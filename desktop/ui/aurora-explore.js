// Aurora's Explore is the mockup's bento (decisions/2026-11-21-aurora-explore.md):
// the first playbook a wide image card, the rest image cards in pairs
// (aurora-stage.css). This script makes a filter change move: a playbook
// still shown slides from where it was to where it is, one leaving fades
// out where it was, one arriving rises in. discover.js is unchanged in how
// it draws: it repaints the page at once, and the change is played here.
"use strict";

/** How long a playbook takes to slide to its new place. */
const EXPLORE_MOVE_MS = 520;
/** How long a playbook leaving takes to fade out. */
const EXPLORE_LEAVE_MS = 260;
/** How far apart playbooks arriving start, one after another. */
const EXPLORE_STAGGER_MS = 40;

const AuroraExplore = (() => {
  const EASE = "cubic-bezier(0.16, 1, 0.3, 1)";
  const CARDS = ":scope > .explore-card[data-playbook]";
  const on = () => typeof Theme !== "undefined" && typeof Theme.stage === "function" && Theme.stage(document.documentElement.dataset.style);
  const moving = () => document.documentElement.dataset.motion === "full";

  /** Where each playbook was just before the filter changed. @type {?Map<string, { rect: DOMRect, node: HTMLElement }>} */
  let before = null;

  function grab() {
    const grid = document.querySelector(".explore-grid");
    if (!grid || !on() || !moving()) return;
    before = new Map([...grid.querySelectorAll(CARDS)].map(c => [/** @type {HTMLElement} */ (c).dataset.playbook || "", { rect: c.getBoundingClientRect(), node: /** @type {HTMLElement} */ (c) }]));
  }
  // Taken before discover.js draws the change (capture runs first).
  document.addEventListener("click", e => { if (/** @type {HTMLElement} */ (e.target).closest?.("[data-exp-cat]")) grab(); }, true);
  document.addEventListener("input", e => { if (/** @type {HTMLElement} */ (e.target).id === "explore-search") grab(); }, true);

  const EXPLORE_LEAVE_FALLBACK_MS = EXPLORE_LEAVE_MS + 140;

  /** The change, played from where things were. @param {Element} grid */
  function play(grid) {
    const was = before;
    before = null;
    if (!was) return;
    grid.querySelectorAll(":scope > .explore-leaving").forEach(n => n.remove());
    const box = grid.getBoundingClientRect();
    let arriving = 0;
    for (const c of /** @type {NodeListOf<HTMLElement>} */ (grid.querySelectorAll(CARDS))) {
      const id = c.dataset.playbook || "";
      const old = was.get(id);
      was.delete(id);
      const now = c.getBoundingClientRect();
      if (!old) {
        c.animate([{ opacity: 0, transform: "translateY(14px) scale(.97)" }, { opacity: 1, transform: "none" }], { duration: EXPLORE_MOVE_MS, delay: arriving++ * EXPLORE_STAGGER_MS, easing: EASE, fill: "backwards" });
        continue;
      }
      const dx = old.rect.left - now.left, dy = old.rect.top - now.top;
      const sx = old.rect.width / (now.width || 1), sy = old.rect.height / (now.height || 1);
      if (Math.abs(dx) < 1 && Math.abs(dy) < 1 && Math.abs(sx - 1) < 0.01 && Math.abs(sy - 1) < 0.01) continue;
      c.animate([{ transformOrigin: "top left", transform: `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})` }, { transformOrigin: "top left", transform: "none" }], { duration: EXPLORE_MOVE_MS, easing: EASE });
    }
    // What the filter took away fades out where it was; nobody can reach it.
    for (const { rect, node } of was.values()) {
      node.classList.add("explore-leaving");
      node.setAttribute("aria-hidden", "true");
      node.inert = true;
      Object.assign(node.style, { position: "absolute", left: `${rect.left - box.left}px`, top: `${rect.top - box.top}px`, width: `${rect.width}px`, height: `${rect.height}px`, margin: "0" });
      grid.append(node);
      const done = () => node.remove();
      const a = node.animate([{ opacity: 1, transform: "none" }, { opacity: 0, transform: "scale(.96)" }], { duration: EXPLORE_LEAVE_MS, easing: EASE, fill: "forwards" });
      a.onfinish = done;
      a.oncancel = done;
      setTimeout(done, EXPLORE_LEAVE_FALLBACK_MS);
    }
  }

  window.addEventListener("hashchange", () => {
    before = null;
    document.querySelectorAll(".explore-leaving").forEach(n => n.remove());
  });

  // The page drawn again with the change: played before it shows.
  const onMutate = records => {
    if (!before) return;
    for (const r of records) for (const n of r.addedNodes) {
      const grid = n instanceof Element ? (n.matches(".explore-grid") ? n : n.querySelector(".explore-grid")) : null;
      if (grid) return play(grid);
    }
  };
  if (typeof AuroraCore !== "undefined") AuroraCore.onDom(onMutate);
  else new MutationObserver(onMutate).observe(document.documentElement, { childList: true, subtree: true });

  return { on, grab, play };
})();
