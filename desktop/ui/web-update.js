// A browser tab follows the server's updates
// (decisions/2026-11-21-web-tab-follows-updates.md). The server serves this
// page, stamped with its version (index.html's `?v=`), but a tab opened
// before an update kept the old page until someone reloaded it. Now the tab
// looks at the server's version now and then, and when it has changed it
// loads the new page: at once while nothing is being done in it, else as
// soon as the person leaves the tab or stops. What they were typing is kept
// (drafts.js), and the address they were at is the one that loads. The
// desktop app updates itself and is left alone.
"use strict";

const WebUpdate = (() => {
  /** How often the tab asks the server which version it runs. */
  const LOOK_EVERY_MS = 60_000;
  /** How long nothing is typed or clicked before the tab counts as left alone. */
  const IDLE_FOR_MS = 20_000;
  const KEY = "magnus-loaded-for";

  /** The version this page was served as, from its own stamped scripts. */
  function loaded() {
    const s = /** @type {?HTMLScriptElement} */ (document.querySelector('script[src*="?v="]'));
    return s ? new URL(s.src, location.href).searchParams.get("v") : null;
  }

  let last = Date.now();
  /** @type {?string} */
  let waiting = null;
  for (const e of ["keydown", "pointerdown", "input"]) document.addEventListener(e, () => { last = Date.now(); }, { capture: true, passive: true });

  /** Whether loading the page now would cut into something the person is doing. */
  function busy() {
    if (document.visibilityState === "hidden") return false;
    if (document.querySelector("dialog[open]")) return true;
    const a = document.activeElement;
    if (a && a.matches("textarea, input:not([type=button]):not([type=submit]):not([type=checkbox]):not([type=radio]), select, [contenteditable=true]")) return true;
    return Date.now() - last < IDLE_FOR_MS;
  }

  const api = {
    /** Loads the new page (the tests stand in for it). */
    reload: () => location.reload(),
    /** Loads the new page now if nothing would be cut into. */
    settle() {
      if (!waiting || busy()) return false;
      // One load per version: a page still stamped with the old one after
      // loading (a cache between) does not load over and over.
      try { if (sessionStorage.getItem(KEY) === waiting) return false; sessionStorage.setItem(KEY, waiting); } catch { /* loads once anyway */ }
      waiting = null;
      api.reload();
      return true;
    },
    /** Asks the server which version it runs. */
    async look() {
      const mine = loaded();
      if (!mine || typeof Server === "undefined") return;
      const v = await Server.version().catch(() => null);
      if (v?.version && v.version !== mine) { waiting = v.version; api.settle(); }
    },
    loaded,
  };

  if (typeof window.__TAURI__ === "undefined") {
    setInterval(() => { api.look(); }, LOOK_EVERY_MS);
    setInterval(() => { api.settle(); }, IDLE_FOR_MS / 4);
    document.addEventListener("visibilitychange", () => { if (document.visibilityState === "hidden") api.settle(); else api.look(); });
  }
  return api;
})();
