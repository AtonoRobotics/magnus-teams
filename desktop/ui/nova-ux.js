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
      body: "Approve the same safe pattern three times and Magnus offers to turn it into a standing rule automatically.",
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

  const LAUNCHPADS = [
    {
      requires: ["google-gmail", "microsoft-outlook"],
      icon: "✨",
      label: "Triage my inbox &amp; prepare morning brief",
      prompt: "Triage my unread email and messages, stage replies for my approval, and give me a morning brief.",
    },
    {
      requires: ["github"],
      icon: "🔎",
      label: "Review open PRs &amp; summarize CI blockers",
      prompt: "Rank pull requests waiting on my review by age and CI status, and summarize any failing checks.",
    },
    {
      requires: ["slack"],
      icon: "💬",
      label: "Catch up on quiet Slack threads &amp; blockers",
      prompt: "Summarize decisions, owners, and open blockers from my unread Slack threads.",
    },
    {
      requires: [],
      icon: "📊",
      label: "Prepare board deck &amp; investor follow-ups",
      prompt: "Prepare a board update deck from my recent project files and stage follow-up emails for approval.",
    },
    {
      requires: [],
      icon: "🧭",
      label: "Run parallel competitor pricing research",
      prompt: "Run parallel sub-agents to benchmark competitor pricing and build a live comparison view.",
    },
    {
      requires: [],
      icon: "⚡",
      label: "Turn recent notes into an executive brief",
      prompt: "Read my recent project files, extract key decisions, risks, and owners, and build a 1-page executive brief.",
    },
  ];

  function adaptiveLaunchpads() {
    const connected = new Set(
      (typeof App !== "undefined" && Array.isArray(App.connectors) ? App.connectors : [])
        .filter(c => c.state === "connected")
        .map(c => c.name),
    );
    const matched = LAUNCHPADS.filter(lp => lp.requires.length && lp.requires.some(r => connected.has(r)));
    const universal = LAUNCHPADS.filter(lp => !lp.requires.length);
    return [...matched, ...universal].slice(0, 3);
  }

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
      const pads = adaptiveLaunchpads();
      const wrap = document.createElement("div");
      wrap.className = "nova-showcase";
      wrap.innerHTML = `<div class="nova-intro-grid">
        <div class="nova-launchpads" role="region" aria-label="Autonomous launchpads">
          <span class="nova-launch-head">Autonomous launchpads</span>
          ${pads.map(lp => `<button type="button" class="nova-launch-btn" data-nova-prompt="${lp.prompt}">
            <span aria-hidden="true">${lp.icon}</span><span>${lp.label}</span>
          </button>`).join("")}
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

  // The app the spotlight shows: one of the catalog's own, not yet connected,
  // the mockup's Notion first.
  const FEATURED_FIRST = ["notion", "linear", "github", "slack", "google-calendar", "gmail"];
  function featuredApp(all) {
    const open = all.filter(c => c.state !== "connected" && c.ready !== false);
    const by = name => open.find(c => c.name === name);
    return FEATURED_FIRST.map(by).find(Boolean) || open[0] || all[0] || null;
  }

  function appTile(c, lead) {
    const logo = typeof AppLogos !== "undefined" ? AppLogos.svg(c.name) : "";
    // An app with no logo shows its initial, as the mockup's "N" for Notion.
    const inner = logo || escapeHtml((c.title || "?").slice(0, 1));
    return `<span class="nova-app-tile${lead ? " lead" : ""}${logo ? "" : " letter"}">${inner}</span>`;
  }

  function escapeHtml(v) {
    return String(v ?? "").replace(/[&<>"']/g, ch => `&#${ch.charCodeAt(0)};`);
  }

  function syncLibraryFeatured() {
    if (!isNova()) {
      document.querySelectorAll(".nova-featured").forEach(el => el.remove());
      return;
    }
    const page = document.querySelector(".libpage:not(.cpage)");
    const head = page?.querySelector(":scope > .lhead");
    if (!head) return;
    // The segmented pill sits on the title's row, as in the mockup.
    const seg = page.querySelector(":scope > .lbar > .lseg");
    if (seg) head.append(seg);
    // The spotlight leads the catalog (spec §4.8), on an app of the catalog.
    const all = typeof Lib !== "undefined" && Array.isArray(Lib.all) ? Lib.all : [];
    const c = featuredApp(all);
    const key = c ? `${c.name}:${c.state}` : "";
    const was = page.querySelector(":scope > .nova-featured");
    if (!c) { was?.remove(); return; }
    if (was?.dataset.key === key) return;
    const others = all.filter(o => o !== c).slice(0, 5);
    const tiles = [...others.slice(0, 3).map(o => appTile(o, false)), appTile(c, true), ...others.slice(3).map(o => appTile(o, false))];
    const name = escapeHtml(c.name), title = escapeHtml(c.title);
    const add = c.state === "connected"
      ? `<a class="nova-pbtn blue" href="#/library/${encodeURIComponent(c.name)}">Open</a>`
      : `<button type="button" class="nova-pbtn blue" data-act="connect" data-app="${name}" aria-label="Add ${title}">+ Add</button>`;
    const hero = document.createElement("section");
    hero.className = "nova-featured";
    hero.dataset.key = key;
    hero.setAttribute("aria-label", "Featured app");
    hero.innerHTML = `<div class="nova-featured-art" aria-hidden="true">${tiles.join("")}</div>
    <div class="nova-featured-copy">
      <span class="nova-featured-kicker">Featured</span>
      <h2>${title}</h2>
      <p>${escapeHtml(c.summary || "")}</p>
      <div class="nova-featured-acts">${add}<a class="nova-pbtn" href="#/library/${encodeURIComponent(c.name)}">Details</a></div>
    </div>`;
    if (was) was.replaceWith(hero); else head.insertAdjacentElement("afterend", hero);
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
    // A launchpad or the Live Workspace tile is one tap: it sends its work
    // and the conversation opens on it, as the mockup's launchStage does.
    const promptBtn = t.closest?.("[data-nova-prompt]");
    if (promptBtn) {
      const box = /** @type {?HTMLTextAreaElement} */ (document.getElementById("msg"));
      if (box) {
        box.value = promptBtn.getAttribute("data-nova-prompt") || "";
        box.dispatchEvent(new Event("input", { bubbles: true }));
        box.form?.requestSubmit();
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

  // Keyboard-first approval triage: A = Allow, E = Edit before allowing, D = open decline note drawer.
  document.addEventListener("keydown", e => {
    if (!isNova() || e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey) return;
    if (document.querySelector("dialog[open]")) return;
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

  return { sync, WORKING_TIPS };
})();
