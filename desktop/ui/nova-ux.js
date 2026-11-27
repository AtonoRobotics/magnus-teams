// Nova UX & autonomous feature overhaul:
// 1. Dissolving midnight cobalt halo & prismatic editorial showcase on empty
//    new conversations (.hello-hero) — vanishes smoothly once work begins.
// 2. Live Companion Studio preview card in Settings -> Appearance with
//    interactive state demo pills (Ready, Listening, Thinking, Working, Waiting, Done).
// 3. Verified Task Replay Scrubber on task views with 2+ steps.
// 4. "While Magnus works" calm tip carousel during active background runs.
// 5. Featured Spotlight hero banner on the Apps & Connectors catalog.
// 6. Keyboard-first approval triage (A = Allow, D = open decline note drawer).
"use strict";

const NovaUx = (() => {
  const SYNC_DEBOUNCE_MS = 60;
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
  const DEMO_STATES = [
    ["ready", "Ready"],
    ["listening", "Listening"],
    ["thinking", "Thinking"],
    ["working", "Working"],
    ["waiting", "Waiting"],
    ["done", "Done"],
  ];

  let syncTimer = 0;
  let tipIndex = 0;
  let tipDismissedFor = "";
  let studioState = "ready";
  let studioAvatar = null;

  const isNova = () => document.documentElement.dataset.style === "nova";

  function syncIntroState() {
    if (!isNova()) {
      delete document.body.dataset.novaIntro;
      document.querySelectorAll(".nova-showcase").forEach(el => el.remove());
      return;
    }
    const thread = document.getElementById("thread");
    const hero = thread?.querySelector(".hello-hero");
    const hasMsgs = Boolean(thread?.querySelector(".msg"));
    const onIntro = Boolean(hero) && !hasMsgs;
    document.body.dataset.novaIntro = onIntro ? "on" : "off";

    const conv = thread?.closest("section.conv");
    const slot = conv?.querySelector(":scope > .composer-slot");
    const existing = conv?.querySelector(":scope > .nova-showcase");
    if (!onIntro) {
      existing?.remove();
    } else if (conv && slot && !existing) {
      const wrap = document.createElement("div");
      wrap.className = "nova-showcase";
      wrap.innerHTML = `<div class="nova-intro-grid">
        <div class="nova-launchpads" role="region" aria-label="Autonomous launchpads">
          <span class="nova-launch-head">Autonomous launchpads</span>
          <button type="button" class="nova-launch-btn" data-nova-prompt="Prepare a board update deck from my recent project files and stage follow-up emails for approval.">
            <span aria-hidden="true">📊</span><span>Prepare board deck &amp; investor follow-ups</span>
          </button>
          <button type="button" class="nova-launch-btn" data-nova-prompt="Triage my unread email and messages, stage replies for my approval, and give me a morning brief.">
            <span aria-hidden="true">✨</span><span>Triage my inbox &amp; prepare morning brief</span>
          </button>
          <button type="button" class="nova-launch-btn" data-nova-prompt="Run parallel sub-agents to benchmark competitor pricing and build a live comparison view.">
            <span aria-hidden="true">🧭</span><span>Run parallel competitor pricing research</span>
          </button>
        </div>
        <div class="nova-prism-card" role="region" aria-label="Featured capabilities">
          <button type="button" class="nova-prism-tile nova-prism-wide" data-nova-prompt="Build an interactive summary dashboard from my recent project files and connected apps.">
            <div class="nova-prism-top">
              <div>
                <strong>Interactive views built to remix &amp; share.</strong>
                <span class="nova-prism-badge">Live Workspace</span>
              </div>
              <div class="nova-prism-mosaic" aria-hidden="true">
                <span style="background:#6366f1"></span><span style="background:#38bdf8"></span><span style="background:#f43f5e"></span><span style="background:#fbbf24"></span>
                <span style="background:#34d399"></span><span style="background:#a855f7"></span><span style="background:#818cf8"></span><span style="background:#0ea5e9"></span>
              </div>
            </div>
          </button>
          <a class="nova-prism-tile nova-prism-accent-a" href="#/explore">
            <strong>Editorial Playbooks</strong>
            <span class="small">Visual gallery →</span>
          </a>
          <a class="nova-prism-tile nova-prism-accent-b" href="#/library">
            <strong>Apps &amp; Skills</strong>
            <span class="small">Notion, Linear, Mail →</span>
          </a>
        </div>
      </div>`;
      slot.insertAdjacentElement("afterend", wrap);
    }

    syncTopBar();
  }

  function syncTopBar() {
    if (!isNova() || typeof Theme === "undefined") {
      document.querySelectorAll(".nova-topbar-ctrls").forEach(el => el.remove());
      return;
    }
    const row = document.querySelector(".conv-head > .row");
    if (!row) return;
    const key = topBarKey();
    const ctrls = row.querySelector(".nova-topbar-ctrls");
    if (ctrls?.getAttribute("data-key") === key) return;
    // convHead paints the controls with the bar (topBarHtml), so a repaint
    // never drops them; this only follows a change of companion or mode.
    if (ctrls) ctrls.outerHTML = topBarHtml();
    else row.insertAdjacentHTML("afterbegin", topBarHtml());
  }

  const topBarKey = () => `${document.documentElement.dataset.avatar || "pip"}:${document.documentElement.dataset.mode || "dark"}`;
  // Nova's quick controls in the top bar (spec §4.2): companion segmented
  // pill and the Light / Dark toggle.
  function topBarHtml() {
    if (!isNova()) return "";
    const curAvatar = document.documentElement.dataset.avatar || "pip";
    const curMode = document.documentElement.dataset.mode || "dark";
    const avatars = [["pip", "Pip"], ["mochi", "Mochi"], ["orb", "Orb"], ["prism", "Prism"]];
    return `<div class="nova-topbar-ctrls" data-key="${topBarKey()}"><div class="nova-top-seg" role="group" aria-label="Switch companion">${avatars.map(([id, label]) => `<button type="button" class="${curAvatar === id ? "on" : ""}" data-nova-set-avatar="${id}" aria-pressed="${curAvatar === id}">${label}</button>`).join("")}</div>
      <button type="button" class="nova-top-pill" data-nova-toggle-mode>${curMode === "dark" ? "Light" : "Dark"}</button></div>`;
  }

  function syncCompanionStudio() {
    const heading = document.getElementById("avatar-h");
    if (!heading || !isNova() || typeof AuroraAvatar === "undefined") {
      if (studioAvatar) { studioAvatar.avatar?.destroy?.(); studioAvatar = null; }
      return;
    }
    const existing = heading.parentElement?.querySelector(".nova-companion-studio");
    if (existing) return;
    const card = document.createElement("div");
    card.className = "nova-companion-studio";
    card.innerHTML = `<div class="nova-studio-stage" data-nova-studio-slot></div>
      <div class="nova-studio-meta">
        <strong>Live Companion Preview</strong>
        <span class="small muted">Move your pointer and preview each live agent state.</span>
        <div class="nova-studio-states">${DEMO_STATES.map(([k, l]) => `<button type="button" class="${k === studioState ? "on" : ""}" data-nova-demo-state="${k}">${l}</button>`).join("")}</div>
      </div>`;
    heading.insertAdjacentElement("afterend", card);
    const slot = /** @type {?HTMLElement} */ (card.querySelector("[data-nova-studio-slot]"));
    if (slot) studioAvatar = AuroraAvatar.mount(slot, 86, "studio", studioState);
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
    bar.innerHTML = `<div class="nova-replay-head">
      <strong class="small">Task Replay</strong>
      <span class="mono small muted" data-nova-replay-lbl>Step ${steps.length} / ${steps.length}</span>
    </div>
    <input type="range" class="nova-replay-range" min="1" max="${steps.length}" value="${steps.length}" aria-label="Scrub task steps">`;
    parent.insertBefore(bar, first);
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
    // The spotlight leads the catalog (spec §4.8): straight under the page's
    // header, above search, programs and outside agents.
    const head = document.querySelector(".libpage > .lhead");
    if (!head || head.nextElementSibling?.classList.contains("nova-featured")) return;
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
    head.insertAdjacentElement("afterend", hero);
  }

  function sync() {
    syncIntroState();
    syncCompanionStudio();
    syncTaskReplay();
    syncWorkingTips();
    syncLibraryFeatured();
  }

  function queueSync() {
    if (syncTimer) return;
    syncTimer = window.setTimeout(() => { syncTimer = 0; sync(); }, SYNC_DEBOUNCE_MS);
  }

  document.addEventListener("click", e => {
    const t = /** @type {HTMLElement} */ (e.target);
    const demoBtn = t.closest?.("[data-nova-demo-state]");
    if (demoBtn) {
      studioState = demoBtn.getAttribute("data-nova-demo-state") || "ready";
      demoBtn.parentElement?.querySelectorAll("button").forEach(b => b.classList.toggle("on", b === demoBtn));
      if (studioAvatar) {
        studioAvatar.fixedState = studioState;
        studioAvatar.avatar?.set?.(studioState);
      }
      return;
    }
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
    const avBtn = t.closest?.("[data-nova-set-avatar]");
    if (avBtn && typeof Theme !== "undefined") {
      Theme.set({ avatar: avBtn.getAttribute("data-nova-set-avatar") || "pip" });
      syncTopBar();
      return;
    }
    if (t.closest?.("[data-nova-toggle-mode]") && typeof Theme !== "undefined") {
      const cur = Theme.effective(Theme.prefs(), Theme.os()).mode;
      Theme.set({ mode: cur === "dark" ? "light" : "dark" });
      syncTopBar();
      return;
    }
    if (t.closest?.("[data-nova-tip-close]")) {
      tipDismissedFor = location.hash || "#/home";
      document.querySelector(".nova-wait-tips")?.remove();
    }
  });

  // Keyboard-first approval triage: A = Allow, E = Edit before allowing, D = open decline note drawer.
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
  } else {
    new MutationObserver(queueSync).observe(document.documentElement, { childList: true, subtree: true });
  }
  window.addEventListener("magnus-theme", queueSync);
  window.addEventListener("hashchange", queueSync);
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", queueSync);
  else queueSync();

  return { sync, topBarHtml, WORKING_TIPS };
})();
