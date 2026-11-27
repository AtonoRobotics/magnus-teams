// Nova's top bar (docs/nova-greenfield-mockup.html, spec §4.2): one bar
// across the top of every screen, beside the rail. On the left, where the
// person is and this month's spend; on the right, the companion pill and
// Light / Dark. A conversation keeps its own head under it (breadcrumb,
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

  /** The companion pill and Light / Dark (spec §4.2). */
  function controlsHtml() {
    const avatar = document.documentElement.dataset.avatar || "pip";
    const mode = document.documentElement.dataset.mode || "dark";
    const avatars = [["pip", "Pip"], ["mochi", "Mochi"], ["orb", "Orb"], ["prism", "Prism"]];
    return `<div class="nova-topbar-ctrls"><div class="admin-kill" data-admin-kill-top role="group" aria-label="Kill switch" hidden></div><div class="nova-top-seg" role="group" aria-label="Switch companion">${avatars.map(([id, label]) => `<button type="button" class="${avatar === id ? "on" : ""}" data-nova-set-avatar="${id}" aria-pressed="${avatar === id}">${label}</button>`).join("")}</div><button type="button" class="nova-top-pill" data-nova-toggle-mode>${mode === "dark" ? "Light" : "Dark"}</button></div>`;
  }

  /** Paints the bar when what it shows has changed. */
  function sync() {
    const shell = document.querySelector(".shell");
    let bar = shell?.querySelector(":scope > .nova-frame-bar");
    if (!isNova() || !shell) { bar?.remove(); return; }
    readSpend();
    const title = where();
    const money = spendText();
    const key = [title, money, document.documentElement.dataset.avatar || "pip", document.documentElement.dataset.mode || "dark"].join("|");
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

  document.addEventListener("click", e => {
    const t = /** @type {HTMLElement} */ (e.target);
    const avBtn = t.closest?.("[data-nova-set-avatar]");
    if (avBtn && typeof Theme !== "undefined") {
      Theme.set({ avatar: avBtn.getAttribute("data-nova-set-avatar") || "pip" });
      soon();
      return;
    }
    if (t.closest?.("[data-nova-toggle-mode]") && typeof Theme !== "undefined") {
      const cur = Theme.effective(Theme.prefs(), Theme.os()).mode;
      Theme.set({ mode: cur === "dark" ? "light" : "dark" });
      soon();
    }
  });
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
