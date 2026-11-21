// Aurora's navigation is a rail (decisions/2026-11-21-aurora-rail.md): the
// avatar, New conversation, Search, Conversations, Explore and Settings, as
// icons. The full sidebar slides in over the stage when the person asks for
// it, by the Conversations button or Ctrl+B (⌘B), never on hover. It closes
// when a conversation is picked, on Esc, on a click outside it or its Close,
// and its pin keeps it open beside the stage, as a sidebar, until unpinned.
//
// The slide-out is app.js's navigation drawer, the modal dialog the narrow
// window uses, so it keeps that dialog's focus, Esc and outside click. This
// script only opens it from the rail, draws the full sidebar in it rather
// than the rail's icons, and gives the focus back when it closes. Whether
// the rail or the pinned sidebar shows is the `sidebar` preference, which
// Theme.navigation turns into the layout's navigation (theme.js).
"use strict";

const AuroraRail = (() => {
  const on = () => typeof Theme !== "undefined" && typeof Theme.stage === "function" && Theme.stage(document.documentElement.dataset.style);
  const $ = (/** @type {string} */ q) => /** @type {?HTMLElement} */ (document.querySelector(q));
  // Its own icons: the app's set has no sidebar or pin.
  const SIDEBAR = '<svg class="ic" viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/></svg>';
  const PIN = '<svg class="ic" viewBox="0 0 24 24" aria-hidden="true"><path d="M9 4h6l-1 6 4 4H6l4-4z"/><path d="M12 14v6"/></svg>';

  /** Whether the full sidebar is open over the stage. */
  let open = false;
  /** Whether it closed because a place in it was picked. */
  let picked = false;

  const rail = () => on() && document.body.dataset.navigation === "compact";
  const pinned = () => typeof Theme !== "undefined" && Theme.prefs().sidebar === "pinned";

  /** Its two controls, put once in the part of the sidebar app.js draws once. */
  function controls() {
    const nav = $("nav.side"), brand = nav?.querySelector(".brand");
    if (!nav || !brand || nav.querySelector(".aurora-rail-open")) return;
    const show = document.createElement("button");
    show.type = "button";
    show.className = "icon-btn aurora-rail-open";
    show.setAttribute("aria-label", "Conversations");
    show.title = "Conversations (Ctrl+B)";
    show.setAttribute("aria-keyshortcuts", "Control+B Meta+B");
    show.setAttribute("aria-haspopup", "dialog");
    show.innerHTML = SIDEBAR;
    nav.querySelector(".rail-search")?.after(show);
    const pin = document.createElement("button");
    pin.type = "button";
    pin.className = "icon-btn aurora-rail-pin";
    pin.innerHTML = PIN;
    brand.querySelector("[data-nav-toggle]")?.after(pin);
    label();
  }

  /** The pin says what it does, and whether the sidebar is pinned. */
  function label() {
    const pin = $(".aurora-rail-pin");
    if (!pin) return;
    const kept = pinned();
    pin.setAttribute("aria-pressed", String(kept));
    pin.setAttribute("aria-label", kept ? "Unpin the sidebar" : "Pin the sidebar");
    pin.title = kept ? "Unpin the sidebar: show it only when asked for" : "Pin the sidebar: keep it open beside the conversation";
  }

  function show() {
    if (open || !rail()) return;
    // app.js opens its drawer from this button, which the rail hides.
    const opener = $("[data-open-drawer]");
    if (!opener) return;
    opener.click();
    if (!/** @type {HTMLDialogElement} */ ($("#navigation-drawer"))?.open) return;
    open = true;
    picked = false;
    // The sidebar in the drawer is the full one; the layout is still the
    // rail's, and app.js sets this back when it next fits the window.
    document.body.dataset.navigation = "expanded";
    label();
  }

  function hide() {
    /** @type {?HTMLDialogElement} */ ($("#navigation-drawer"))?.close();
  }

  function closed() {
    if (!open) return;
    open = false;
    if (typeof Workspace !== "undefined") Workspace.update();
    label();
    // A place picked takes the focus where it goes; Esc, Close or a click
    // outside give it back to the button that opened the sidebar.
    if (!picked && (!document.activeElement || document.activeElement === document.body)) back();
  }

  /** The focus to the control that shows the sidebar as it is now. */
  function back() {
    (rail() ? $(".aurora-rail-open") : $(".aurora-rail-pin"))?.focus({ preventScroll: true });
  }

  document.addEventListener("click", e => {
    const t = /** @type {HTMLElement} */ (e.target);
    if (t.closest?.(".aurora-rail-open")) { if (open) hide(); else show(); return; }
    if (t.closest?.(".aurora-rail-pin")) {
      Theme.set({ sidebar: pinned() ? "rail" : "pinned" });
      label();
      if (!document.activeElement || document.activeElement === document.body || !(/** @type {HTMLElement} */ (document.activeElement)).offsetParent) back();
      return;
    }
    if (open && t.closest?.("#navigation-drawer a[href]")) picked = true;
  }, true);

  document.addEventListener("keydown", e => {
    if (!(e.ctrlKey || e.metaKey) || e.shiftKey || e.altKey || e.key.toLowerCase() !== "b") return;
    if (!on() || (!rail() && !open) || document.querySelector("dialog[open]:not(#navigation-drawer)")) return;
    e.preventDefault();
    if (open) hide(); else show();
  });

  window.addEventListener("magnus-theme", label);

  // The drawer and the sidebar are drawn when the app starts, and drawn
  // again whole when it starts again: then the body's children change.
  function attach() {
    const d = $("#navigation-drawer");
    if (!d || !$("nav.side .brand")) return false;
    if (!d.dataset.auroraRail) { d.dataset.auroraRail = "1"; d.addEventListener("close", closed); }
    controls();
    return true;
  }
  const first = new MutationObserver(() => {
    if (!attach() || !document.body) return;
    first.disconnect();
    first.observe(document.body, { childList: true });
  });
  if (document.documentElement) first.observe(document.documentElement, { childList: true, subtree: true });

  return { show, hide, get open() { return open; } };
})();
