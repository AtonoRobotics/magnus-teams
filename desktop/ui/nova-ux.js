// Nova's small helpers that act on what is already on the page:
// 1. Marks an empty new conversation, so the composer's state line hides.
// 2. Verified Task Replay Scrubber on task views with 2+ steps.
// 3. Keyboard-first approval triage (A = Allow, D = open decline note drawer).
// The showcase, the companion demo and the featured-app banner went with the
// owner's "remove ceremony, and theater" (decisions/2026-11-28-ui-without-ceremony.md).
"use strict";

const NovaUx = (() => {
  const SYNC_DEBOUNCE_MS = 60;

  let syncTimer = 0;

  const isNova = () => document.documentElement.dataset.style === "nova";

  function syncIntroState() {
    if (!isNova()) { delete document.body.dataset.novaIntro; return; }
    const thread = document.getElementById("thread");
    const onIntro = Boolean(thread?.querySelector(".hello-hero")) && !thread?.querySelector(".msg");
    document.body.dataset.novaIntro = onIntro ? "on" : "off";
  }

  function syncTaskReplay() {
    if (!isNova()) {
      document.querySelectorAll(".nova-replay").forEach(el => el.remove());
      return;
    }
    const steps = [...document.querySelectorAll("#main .step, #detail .step")];
    if (steps.length < 2) return;
    const first = steps[0];
    const parent = first.parentElement;
    if (!parent || parent.querySelector(".nova-replay")) return;
    const bar = document.createElement("div");
    bar.className = "nova-replay";
    parent.insertBefore(bar, first);
    if (typeof TaskReplay !== "undefined" && typeof TaskReplay.mount === "function") {
      TaskReplay.mount(bar, steps);
      return;
    }
    bar.innerHTML = `<div class="nova-replay-head">
      <strong class="small">Task Replay</strong>
      <span class="mono small muted" data-nova-replay-lbl>Step ${steps.length} / ${steps.length}</span>
    </div>
    <input type="range" class="nova-replay-range" min="1" max="${steps.length}" value="${steps.length}" aria-label="Scrub task steps">`;
    const range = /** @type {HTMLInputElement} */ (bar.querySelector(".nova-replay-range"));
    const lbl = bar.querySelector("[data-nova-replay-lbl]");
    range.addEventListener("input", () => {
      const idx = Number(range.value);
      if (lbl) lbl.textContent = `Step ${idx} / ${steps.length}`;
      steps.forEach((el, i) => {
        el.classList.toggle("nova-step-active", i === idx - 1);
        el.classList.toggle("nova-step-future", i >= idx);
      });
    });
  }

  // Apps' segmented pill sits on the title's row, as in the mockup.
  function syncLibraryHead() {
    if (!isNova()) return;
    const page = document.querySelector(".libpage:not(.cpage)");
    const head = page?.querySelector(":scope > .lhead");
    const seg = page?.querySelector(":scope > .lbar > .lseg");
    if (head && seg) head.append(seg);
  }

  function sync() {
    syncIntroState();
    syncTaskReplay();
    syncLibraryHead();
  }

  function queueSync() {
    if (syncTimer) return;
    syncTimer = window.setTimeout(() => { syncTimer = 0; sync(); }, SYNC_DEBOUNCE_MS);
  }

  // Keyboard-first approval triage: A = Allow, E = Edit before allowing, D = open decline note drawer.
  document.addEventListener("keydown", e => {
    if (!isNova() || e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey) return;
    if (document.querySelector("dialog[open]")) return;
    if (document.querySelector(".card.item.is-triage-focus")) return;
    const active = document.activeElement;
    if (active && /^(INPUT|TEXTAREA|SELECT)$/.test(active.tagName)) return;
    if (active && /** @type {HTMLElement} */ (active).isContentEditable) return;
    const ask = [...document.querySelectorAll(".ask[data-ask]")].find(el => !el.closest("[hidden]"));
    if (!ask) return;
    const key = e.key.toLowerCase();
    if (key === "a") {
      const allowBtn = /** @type {?HTMLButtonElement} */ (ask.querySelector('button[data-offer="allow"]'));
      if (allowBtn) { e.preventDefault(); allowBtn.click(); }
    } else if (key === "e") {
      const edit = /** @type {?HTMLDetailsElement} */ (ask.querySelector("[data-ask-edit]"));
      if (edit) {
        e.preventDefault();
        edit.open = true;
        /** @type {?HTMLElement} */ (edit.querySelector("[data-edit-field]"))?.focus();
      }
    } else if (key === "d") {
      const dec = /** @type {?HTMLDetailsElement} */ (ask.querySelector("[data-ask-decline]"));
      if (dec) {
        e.preventDefault();
        dec.open = true;
        /** @type {?HTMLInputElement} */ (dec.querySelector(".ask-decline-note"))?.focus();
      }
    }
  });

  if (typeof AuroraCore !== "undefined") {
    AuroraCore.onDom(queueSync);
    AuroraCore.onState(queueSync);
  }
  if (typeof UIStore !== "undefined") UIStore.subscribe("*", queueSync);
  window.addEventListener("magnus-render", queueSync);
  window.addEventListener("magnus-theme", queueSync);
  window.addEventListener("hashchange", queueSync);
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", queueSync);
  else queueSync();

  return { sync };
})();
