// Aurora, compact (decisions/2026-11-21-aurora-compact.md): the composer's
// model and reasoning level are one chip, "claude-sonnet · Auto", not two
// menus side by side. The chip opens a small menu above it with both, as the
// app draws them (modelPicker in app.js), so what is sent is read from the
// same two selects as before, still in the composer's form. Other styles
// show the two menus as they were.
"use strict";

const AuroraCompact = (() => {
  const on = () => typeof Theme !== "undefined" && typeof Theme.stage === "function" && Theme.stage(document.documentElement.dataset.style);

  /** What the chip says: the model, and the reasoning level. @param {HTMLElement} chip */
  function words(chip) {
    const model = /** @type {?HTMLSelectElement} */ (chip.querySelector("select[data-model]"));
    const reasoning = /** @type {?HTMLSelectElement} */ (chip.querySelector("select[data-reasoning]"));
    const m = (model?.selectedOptions[0]?.textContent || "").replace(/\s*\(default\)$/, "").trim();
    const r = (reasoning?.selectedOptions[0]?.textContent || "").replace(/^Reasoning:\s*/, "").trim();
    const shown = /** @type {?HTMLElement} */ (chip.querySelector(".model-chip-words"));
    if (shown) shown.textContent = [m, r].filter(Boolean).join(" · ");
    const summary = chip.querySelector("summary");
    summary?.setAttribute("aria-label", `Model ${m || "not chosen"}${r ? `, reasoning ${r}` : ""}. Change`);
  }

  /** Puts a composer's model menus into one chip. @param {Element} bar */
  function fold(bar) {
    const labels = [...bar.querySelectorAll(":scope > label.model")];
    if (!labels.length || bar.querySelector(":scope > .model-chip")) return;
    const chip = document.createElement("details");
    chip.className = "model-chip";
    const summary = document.createElement("summary");
    const shown = document.createElement("span");
    shown.className = "model-chip-words";
    summary.append(shown);
    const menu = document.createElement("div");
    menu.className = "model-chip-menu";
    menu.setAttribute("role", "group");
    menu.setAttribute("aria-label", "Model and reasoning");
    labels[0].before(chip);
    menu.append(...labels);
    chip.append(summary, menu);
    chip.addEventListener("change", () => words(chip));
    chip.addEventListener("toggle", () => words(chip));
    words(chip);
  }

  /** Gives a composer back its two menus. @param {Element} chip */
  function unfold(chip) {
    const labels = [...chip.querySelectorAll(":scope > .model-chip-menu > label.model")];
    chip.before(...labels);
    chip.remove();
  }

  function place() {
    if (on()) document.querySelectorAll(".say-bar").forEach(fold);
    else document.querySelectorAll(".say-bar > .model-chip").forEach(unfold);
  }

  /** Shuts any chip open, but `keep`. @param {?Element} [keep] */
  function shut(keep) {
    document.querySelectorAll(".model-chip[open]").forEach(c => { if (c !== keep) /** @type {HTMLDetailsElement} */ (c).open = false; });
  }

  // A click elsewhere, or Esc, shuts the menu; Esc gives the focus back to
  // the chip.
  document.addEventListener("click", e => shut(/** @type {Element} */ (e.target).closest?.(".model-chip")), true);
  document.addEventListener("keydown", e => {
    if (e.key !== "Escape") return;
    const open = /** @type {?HTMLDetailsElement} */ (document.querySelector(".model-chip[open]"));
    if (!open) return;
    e.preventDefault();
    e.stopPropagation();
    open.open = false;
    /** @type {?HTMLElement} */ (open.querySelector("summary"))?.focus();
  }, true);
  // A select changed by the app (a new default, a draft brought back) is
  // said on the chip too.
  document.addEventListener("input", e => {
    const chip = /** @type {Element} */ (e.target).closest?.(".model-chip");
    if (chip) words(/** @type {HTMLElement} */ (chip));
  }, true);

  window.addEventListener("magnus-theme", place);
  const checkBar = () => { if (on() && document.querySelector(".say-bar > label.model")) place(); };
  const start = () => {
    place();
    if (typeof AuroraCore !== "undefined") AuroraCore.onDom(records => { if (records.some(r => r.addedNodes.length)) checkBar(); });
    else new MutationObserver(checkBar).observe(document.body, { childList: true, subtree: true });
  };
  if (document.body) start(); else document.addEventListener("DOMContentLoaded", start);

  return { place, words };
})();
