// Nova UX & autonomous feature enhancements:
// 1. Dissolving midnight cobalt halo & prismatic editorial showcase on empty
//    new conversations (vanishes smoothly once work begins).
// 2. "While Magnus works" calm tip carousel during active background runs.
// 3. Featured Spotlight hero banner on the Apps & Connectors catalog.
// 4. Keyboard-first approval triage (A = Allow, D = open decline note drawer)
//    whenever an open ask card is visible and focus is not in a text field.
"use strict";

const NovaUx = (() => {
  const SYNC_DEBOUNCE_MS = 80;
  const WORKING_TIPS = [
    {
      title: "Edit before approving",
      body: "Open “Edit before allowing” on any approval card to adjust recipients, subjects, or wording—Magnus executes your exact edit with a verified receipt.",
    },
    {
      title: "Trust Graduation",
      body: "Approve the same safe pattern three times and Magnus offers to turn it into a scoped Cedar rule automatically.",
    },
    {
      title: "Quiet routines",
      body: "Scheduled checks run silently in the background and only surface a card when something genuinely needs your attention.",
    },
  ];

  let syncTimer = 0;
  let tipIndex = 0;
  let tipDismissedFor = "";

  const isNova = () => document.documentElement.dataset.style === "nova";

  function syncIntroState() {
    if (!isNova()) {
      delete document.body.dataset.novaIntro;
      document.querySelectorAll(".nova-showcase").forEach(el => el.remove());
      return;
    }
    const thread = document.getElementById("thread");
    const hasHello = Boolean(thread?.querySelector(".hello"));
    const hasMsgs = Boolean(thread?.querySelector(".msg"));
    const onIntro = hasHello && !hasMsgs;
    document.body.dataset.novaIntro = onIntro ? "on" : "off";

    if (onIntro && thread) {
      const hello = thread.querySelector(".hello");
      if (hello && !hello.querySelector(".nova-showcase")) {
        const wrap = document.createElement("div");
        wrap.className = "nova-showcase";
        wrap.innerHTML = `<div class="nova-prism-card" role="region" aria-label="Featured capabilities">
          <button type="button" class="nova-prism-tile nova-prism-wide" data-nova-prompt="Build an interactive summary dashboard from my recent project files and connected apps.">
            <span class="nova-prism-badge">Live Workspace</span>
            <strong>Interactive views built to remix &amp; share</strong>
            <span class="small muted">Magnus composes live bento tiles, parallel sub-agents, and verified deliverables.</span>
          </button>
          <a class="nova-prism-tile nova-prism-accent-a" href="#/explore">
            <strong>Editorial Playbooks</strong>
            <span class="small">Browse visual workflows →</span>
          </a>
          <a class="nova-prism-tile nova-prism-accent-b" href="#/library">
            <strong>Apps &amp; Skills</strong>
            <span class="small">Connect Notion, Linear, Mail →</span>
          </a>
        </div>`;
        hello.append(wrap);
      }
    }
  }

  function syncWorkingTips() {
    const existing = document.querySelector(".nova-wait-tips");
    if (!isNova()) { existing?.remove(); return; }
    const state = document.documentElement.dataset.auroraState || "idle";
    const working = state === "executing" || state === "thinking";
    const hasPendingAsk = Boolean(document.querySelector(".ask[data-ask]"));
    const threadKey = location.hash || "#/home";
    if (!working || hasPendingAsk || tipDismissedFor === threadKey) {
      existing?.remove();
      return;
    }
    const host = document.querySelector("section.conv > .stage-bento");
    if (!host) return;
    if (existing && existing.parentElement === host) return;
    existing?.remove();
    const tip = WORKING_TIPS[tipIndex];
    const card = document.createElement("div");
    card.className = "nova-wait-tips";
    card.setAttribute("role", "note");
    card.innerHTML = `<div class="nova-wait-head">
      <span class="small muted">While Magnus works</span>
      <button type="button" class="icon-btn nova-wait-close" data-nova-tip-close aria-label="Dismiss tips">×</button>
    </div>
    <div class="nova-wait-row">
      <button type="button" class="icon-btn" data-nova-tip-step="-1" aria-label="Previous tip">←</button>
      <div class="nova-wait-body">
        <strong class="nova-wait-title">${tip.title}</strong>
        <p class="small muted nova-wait-copy">${tip.body}</p>
      </div>
      <button type="button" class="icon-btn" data-nova-tip-step="1" aria-label="Next tip">→</button>
    </div>`;
    host.append(card);
  }

  function syncLibraryFeatured() {
    if (!isNova()) {
      document.querySelectorAll(".nova-featured").forEach(el => el.remove());
      return;
    }
    const sections = document.getElementById("lib-sections");
    if (!sections || sections.previousElementSibling?.classList.contains("nova-featured")) return;
    const hero = document.createElement("section");
    hero.className = "nova-featured";
    hero.setAttribute("aria-label", "Featured integrations");
    hero.innerHTML = `<div class="nova-featured-art" aria-hidden="true">
      <span class="nova-app-tile">N</span>
      <span class="nova-app-tile">L</span>
      <span class="nova-app-tile">M</span>
      <span class="nova-app-tile">C</span>
    </div>
    <div class="nova-featured-copy">
      <span class="small muted">Featured</span>
      <h2>Connect your daily tools</h2>
      <p class="small muted">Add Notion, Linear, Mail, and Calendar so Magnus can research, draft, and automate with Cedar guardrails.</p>
    </div>`;
    sections.parentElement?.insertBefore(hero, sections);
  }

  function sync() {
    syncIntroState();
    syncWorkingTips();
    syncLibraryFeatured();
  }

  function queueSync() {
    if (syncTimer) return;
    syncTimer = window.setTimeout(() => { syncTimer = 0; sync(); }, SYNC_DEBOUNCE_MS);
  }

  document.addEventListener("click", e => {
    const t = /** @type {HTMLElement} */ (e.target);
    const promptBtn = t.closest?.("[data-nova-prompt]");
    if (promptBtn) {
      const box = /** @type {?HTMLTextAreaElement} */ (document.getElementById("msg"));
      if (box) {
        box.value = promptBtn.getAttribute("data-nova-prompt") || "";
        box.dispatchEvent(new Event("input", { bubbles: true }));
        box.focus();
      }
      return;
    }
    const stepBtn = t.closest?.("[data-nova-tip-step]");
    if (stepBtn) {
      const d = Number(stepBtn.getAttribute("data-nova-tip-step") || "1");
      tipIndex = (tipIndex + d + WORKING_TIPS.length) % WORKING_TIPS.length;
      const tip = WORKING_TIPS[tipIndex];
      const titleEl = document.querySelector(".nova-wait-title");
      const copyEl = document.querySelector(".nova-wait-copy");
      if (titleEl) titleEl.textContent = tip.title;
      if (copyEl) copyEl.textContent = tip.body;
      return;
    }
    if (t.closest?.("[data-nova-tip-close]")) {
      tipDismissedFor = location.hash || "#/home";
      document.querySelector(".nova-wait-tips")?.remove();
    }
  });

  // Keyboard-first approval triage: A = Allow, D = open decline note drawer.
  document.addEventListener("keydown", e => {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const active = document.activeElement;
    if (active && /^(INPUT|TEXTAREA|SELECT)$/.test(active.tagName)) return;
    if (active && /** @type {HTMLElement} */ (active).isContentEditable) return;
    const ask = document.querySelector(".ask[data-ask]");
    if (!ask) return;
    const key = e.key.toLowerCase();
    if (key === "a") {
      const allowBtn = /** @type {?HTMLButtonElement} */ (ask.querySelector('button[data-offer="allow"]'));
      if (allowBtn) { e.preventDefault(); allowBtn.click(); }
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
  } else {
    new MutationObserver(queueSync).observe(document.documentElement, { childList: true, subtree: true });
  }
  window.addEventListener("magnus-theme", queueSync);
  window.addEventListener("hashchange", queueSync);
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", queueSync);
  else queueSync();

  return { sync, WORKING_TIPS };
})();
