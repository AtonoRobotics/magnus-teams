// Every page has a way out (decisions/2026-11-21-aurora-pages.md). In
// Aurora, a page that takes the conversation's place (Explore, Teams,
// Trash, a project, the Admin pages) has a close at its top right that goes
// back to the conversation the person came from, or to a new one. A page
// with its own close (Apps) keeps it, and a conversation has none.
"use strict";

const AuroraPages = (() => {
  const on = () => typeof Theme !== "undefined" && typeof Theme.stage === "function" && Theme.stage(document.documentElement.dataset.style);
  /** The conversation last shown, to go back to. */
  let conversation = "";

  const isConversationHash = (/** @type {string} */ h) => !h || /^#\/(?:chat(?:\/|$)|home$|$)/.test(h);
  /** Where the close goes: the last conversation, else a new one. */
  const back = () => (conversation && conversation !== location.hash && isConversationHash(conversation)) ? conversation : "#/chat/new";

  /** The page's own way out, if it has one, near its top. @param {HTMLElement} main */
  function ownWayOut(main) {
    const top = main.getBoundingClientRect().top;
    return [...main.querySelectorAll("a, button")].some(el => {
      if (el.classList.contains("aurora-page-close")) return false;
      const r = el.getBoundingClientRect();
      if (!r.width || r.top - top > 140) return false;
      const words = `${el.getAttribute("aria-label") || ""} ${el.textContent || ""}`.trim();
      return /^(close|back|←)/i.test(words);
    });
  }

  function place() {
    const main = /** @type {?HTMLElement} */ (document.getElementById("main"));
    if (!main) return;
    const mine = main.querySelector(":scope > .aurora-page-bar");
    const talk = !!main.querySelector(":scope > section.conv, .hello-head, #thread");
    if (talk && isConversationHash(location.hash)) conversation = location.hash || "#/home";
    const want = on() && !talk && !!main.firstElementChild && !ownWayOut(main);
    if (!want) { mine?.remove(); return; }
    const title = main.querySelector("h1")?.textContent?.trim() || "this page";
    if (mine) {
      mine.querySelector("button")?.setAttribute("aria-label", `Close ${title}`);
      if (main.firstElementChild !== mine) main.prepend(mine);
      return;
    }
    const bar = document.createElement("div");
    bar.className = "aurora-page-bar";
    const close = document.createElement("button");
    close.type = "button";
    close.className = "icon-btn aurora-page-close";
    close.setAttribute("aria-label", `Close ${title}`);
    close.title = "Close";
    close.innerHTML = typeof icon === "function" ? icon("close") : "×";
    bar.append(close);
    main.prepend(bar);
  }

  document.addEventListener("click", e => {
    if (!(/** @type {Element} */ (e.target).closest?.(".aurora-page-close"))) return;
    e.preventDefault();
    location.hash = back();
  });

  let queued = false;
  const soon = () => { if (queued) return; queued = true; requestAnimationFrame(() => { queued = false; place(); }); };
  window.addEventListener("hashchange", soon);
  window.addEventListener("magnus-theme", soon);
  // The app puts a new #main in place for each page (app.js), and draws
  // some pages into it after their data comes: what is drawn is watched.
  const start = () => {
    if (typeof AuroraCore !== "undefined") AuroraCore.onMain(soon);
    else new MutationObserver(soon).observe(document.body, { childList: true, subtree: true });
    place();
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start); else start();

  return { place, get back() { return back(); } };
})();
