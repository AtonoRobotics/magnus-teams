// Nova's top bar (docs/nova-greenfield-mockup.html, spec §4.2): one bar
// across the top of every screen, beside the rail. On the left, where the
// person is and this month's spend; on the right, an admin's kill switch.
// The companion and Light / Dark are chosen in Settings, not offered on
// every screen (decisions/2026-11-28-ui-without-ceremony.md). A conversation keeps its own head under it (breadcrumb,
// •••, bell, sidebar). The bar replaces the lone close Aurora puts on a page:
// in Nova the rail is the way out (aurora-pages.js).
"use strict";

const NovaFrame = (() => {
  /** How often the month's spend is read again while the app is open. */
  const SPEND_REFRESH_MS = 5 * 60 * 1000;
  const isNova = () => document.documentElement.dataset.style === "nova";
  /** @type {{ spent_usd?: number, limit_usd?: number } | null} */
  let spend = null;
  let spendAt = 0;
  let spendAsking = false;

  const usd = (/** @type {number} */ n) => `$${n.toFixed(2)}`;

  /** Where the person is, in the words the mockup uses. */
  function where() {
    const h = location.hash || "#/home";
    const main = document.getElementById("main");
    const heading = main?.querySelector("h1")?.textContent?.trim() || "";
    if (/^#\/chat\/new/.test(h)) return "New conversation";
    if (/^#\/(?:home|chat|inbox|activity|tasks|schedules|files|new)?(?:\/|$)/.test(h) || /^#\/(?:file|task)\//.test(h)) return "Conversation";
    if (h === "#/projects") return "Project Hub";
    if (/^#\/project\//.test(h)) return heading ? `Project Hub · ${heading}` : "Project Hub";
    if (h === "#/briefing") return "Morning Briefing & Routines";
    if (h === "#/explore") return "Explore & Artifacts";
    if (/^#\/library/.test(h)) return "Apps & Skills";
    if (h === "#/teams") return "Teams";
    if (h === "#/trash") return "Trash";
    return heading || "Magnus";
  }

  function spendText() {
    if (!spend || !Number.isFinite(spend.spent_usd)) return "";
    const spent = /** @type {number} */ (spend.spent_usd);
    const limit = Number.isFinite(spend.limit_usd) ? ` / ${usd(/** @type {number} */ (spend.limit_usd))}` : "";
    return `Spend this month: ${usd(spent)}${limit}`;
  }

  function readSpend() {
    if (spendAsking || Date.now() - spendAt < SPEND_REFRESH_MS) return;
    if (typeof Server === "undefined" || !Server.spend?.get) return;
    spendAsking = true;
    Server.spend.get().then(/** @param {any} s */ s => { spend = s; }).catch(() => {}).finally(() => {
      spendAsking = false;
      spendAt = Date.now();
      sync();
    });
  }

  /** Where an admin's kill switch goes. */
  function controlsHtml() {
    return `<div class="nova-topbar-ctrls"><div class="admin-kill" data-admin-kill-top role="group" aria-label="Kill switch" hidden></div></div>`;
  }

  /** Paints the bar when what it shows has changed. */
  function sync() {
    const shell = document.querySelector(".shell");
    let bar = shell?.querySelector(":scope > .nova-frame-bar");
    if (!isNova() || !shell) { bar?.remove(); return; }
    readSpend();
    const title = where();
    const money = spendText();
    const key = [title, money].join("|");
    if (bar?.getAttribute("data-key") === key) return;
    if (!bar) {
      bar = document.createElement("header");
      bar.className = "nova-frame-bar";
      bar.setAttribute("aria-label", "Where you are");
      shell.querySelector(":scope > .workspace-bar")?.after(bar) ?? shell.prepend(bar);
    }
    bar.setAttribute("data-key", key);
    const esc_ = (/** @type {string} */ s) => s.replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
    bar.innerHTML = `<div class="nova-frame-where"><span class="nova-page-title">${esc_(title)}</span>${money ? `<span class="nova-frame-sep" aria-hidden="true">·</span><span class="nova-frame-spend">${esc_(money)}</span>` : ""}</div>${controlsHtml()}`;
    if (typeof AdminState !== "undefined" && AdminState.standing?.admin && typeof showKillControl === "function") showKillControl();
  }

  let queued = false;
  const soon = () => { if (queued) return; queued = true; requestAnimationFrame(() => { queued = false; sync(); }); };

  window.addEventListener("hashchange", soon);
  window.addEventListener("magnus-theme", soon);
  window.addEventListener("magnus-render", soon);
  if (typeof UIStore !== "undefined") UIStore.subscribe("*", soon);
  const start = () => {
    if (typeof AuroraCore !== "undefined") AuroraCore.onMain(soon);
    sync();
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start); else start();

  return { sync, where };
})();
