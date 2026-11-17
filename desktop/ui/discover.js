// Content-rich discovery: starter playbooks for the new-conversation hero
// and the full Explore gallery (#/explore) so newcomers see what Magnus can
// do, which apps it connects to, and how approval gates keep them in control.

const Discover = (() => {
  /** @typedef {{ id: string, cat: string, mode: string, cls: string, title: string, desc: string, tools: string[], steps: string[], prompt: string }} Playbook */

  const CATEGORIES = [
    { id: "all", label: "All" },
    { id: "ops", label: "Daily Ops" },
    { id: "research", label: "Research" },
    { id: "monitor", label: "Monitor" },
    { id: "eng", label: "Engineering" },
    { id: "team", label: "Teams & Files" },
  ];

  /** @type {Playbook[]} */
  const PLAYBOOKS = [
    {
      id: "inbox-triage",
      cat: "ops",
      mode: "Scheduled · Weekdays 7:45 AM",
      cls: "ok",
      title: "Morning Inbox & Calendar Triage",
      desc: "Scans overnight emails for meeting requests, spots calendar conflicts, proposes 2 open slots, and drafts replies for your 1-click OK.",
      tools: ["Gmail / Outlook", "Calendar", "Approval Gate"],
      steps: [
        "Trigger: Runs automatically every weekday at 7:45 AM.",
        "Read & Check: Scans overnight email & checks calendar overlaps.",
        "Approval Gate: Pauses for your 1-click OK before sending or moving invites.",
        "Outcome: Updates calendar & sends confirmed replies.",
      ],
      prompt: "Every weekday at 7:45 AM, check my overnight emails for meeting requests, spot any calendar conflicts, propose two conflict-free time slots, and draft replies for my approval.",
    },
    {
      id: "supplier-rfq",
      cat: "research",
      mode: "Multi-step · Cloud Browser",
      cls: "warn",
      title: "Research Suppliers & Compare Quotes",
      desc: "Searches vendors in the Cloud Computer, builds a price & lead-time table, drafts quote request emails, and pauses before ordering.",
      tools: ["Cloud Computer", "Gmail", "Approval Gate"],
      steps: [
        "Research: Compares specs, reviews, and verified suppliers in the Cloud Browser.",
        "Outreach: Drafts RFQ emails to top vendors for pricing & lead times.",
        "Approval Gate: Shows comparison table & pauses before sending or ordering.",
        "Outcome: Places approved order, saves receipt & tracks delivery.",
      ],
      prompt: "Help me research a product, compare prices across suppliers, draft quote request emails to the top 3 vendors, and pause for my approval before placing any order.",
    },
    {
      id: "stalled-threads",
      cat: "monitor",
      mode: "Continuous · Every 4h (14d)",
      cls: "sch",
      title: "Track Stalled Threads (48h+)",
      desc: "Watches open vendor and project threads for 14 days, drafts polite follow-ups after 48 hours of silence, and asks before escalating.",
      tools: ["Email", "Slack", "Approval Gate"],
      steps: [
        "Monitor: Checks active project & vendor threads every 4 hours.",
        "Draft: Prepares polite follow-up when a contact is silent > 48 hours.",
        "Approval Gate: Asks before sending nudges or escalating blockers.",
        "Outcome: Keeps a live status brief updated in Files.",
      ],
      prompt: "Monitor my active vendor and project threads for 14 days, draft polite follow-ups whenever a contact is silent for 48 hours, keep a live status brief updated, and ask me before escalating.",
    },
    {
      id: "pr-ci-triage",
      cat: "eng",
      mode: "Scheduled · Weekdays 9:00 AM",
      cls: "ok",
      title: "Daily PR Review Queue & CI Blockers",
      desc: "Ranks pull requests waiting on your review by age, CI status, and release impact, and summarizes failing checks without touching code.",
      tools: ["GitHub"],
      steps: [
        "Trigger: Runs every weekday at 9:00 AM.",
        "Read: Checks open PRs requesting your review & recent CI logs.",
        "Deliver: Sends a prioritized queue with blocker summaries.",
      ],
      prompt: "Every weekday at 9:00 AM, triage pull requests waiting for my review on GitHub, rank them by age and release impact, and summarize any CI blockers.",
    },
    {
      id: "exec-brief",
      cat: "ops",
      mode: "One-time · Multi-doc",
      cls: "ok",
      title: "Turn Docs into an Executive Brief",
      desc: "Reads uploaded files or Drive docs, extracts decisions, risks, and owners, and formats a crisp 1-page brief you can save or share.",
      tools: ["Files", "Google Drive"],
      steps: [
        "Input: Reads attached docs or a Drive folder you name.",
        "Synthesize: Extracts decisions, open risks, owners & deadlines.",
        "Outcome: Saves a formatted 1-page brief to Files.",
      ],
      prompt: "Read the attached documents (or ask me which Drive files to use), extract key decisions, risks, and owners, and format a 1-page executive brief I can save to Files.",
    },
    {
      id: "market-matrix",
      cat: "research",
      mode: "Multi-step · Cloud Browser",
      cls: "warn",
      title: "Competitive & Pricing Matrix",
      desc: "Uses the Cloud Computer to research competitors, pricing tiers, and recent launches, then delivers a structured comparison table.",
      tools: ["Cloud Computer", "Files"],
      steps: [
        "Browse: Visits public product & pricing pages in the Cloud Browser.",
        "Compare: Structures features, pricing tiers & differentiators.",
        "Outcome: Delivers a comparison table and saves a brief to Files.",
      ],
      prompt: "Research the top 4 competitors in [my space], compare their pricing tiers, key features, and recent launches in a structured table, and save the brief to Files.",
    },
    {
      id: "weekly-digest",
      cat: "monitor",
      mode: "Scheduled · Fridays 4:00 PM",
      cls: "sch",
      title: "Weekly Project Risk & Status Digest",
      desc: "Pulls updates across project channels and email threads every Friday, highlights overdue items, and drafts a status summary.",
      tools: ["Slack", "Email", "Files"],
      steps: [
        "Trigger: Runs every Friday at 4:00 PM.",
        "Scan: Reads project threads for shipped work & stalled items.",
        "Approval Gate: Drafts the weekly summary for your review before sharing.",
      ],
      prompt: "Every Friday at 4:00 PM, gather updates from my active project threads, highlight overdue items and risks, and draft a concise weekly status brief for my approval.",
    },
    {
      id: "ci-repro",
      cat: "eng",
      mode: "Multi-step · Cloud Computer",
      cls: "warn",
      title: "Investigate a Failing Build or Bug",
      desc: "Reads CI logs and repository context, reproduces the failure in the Cloud Computer, and proposes a targeted fix for your review.",
      tools: ["GitHub", "Cloud Computer"],
      steps: [
        "Inspect: Reads failing CI run logs and recent commits.",
        "Reproduce: Runs tests in the Cloud Computer sandbox.",
        "Approval Gate: Presents root cause & proposed fix for your review.",
      ],
      prompt: "Investigate the latest failing CI run in [owner/repo], reproduce the failure in the Cloud Computer, explain the root cause, and propose a targeted fix for my review.",
    },
    {
      id: "channel-catchup",
      cat: "team",
      mode: "One-time · Team",
      cls: "ok",
      title: "Catch Up on Quiet Team Threads",
      desc: "Summarizes busy Slack or team channels, pulls out decisions made, open questions, and who owns each next step.",
      tools: ["Slack", "Teams"],
      steps: [
        "Read: Reviews recent messages in the channel you name.",
        "Extract: Separates decisions, blockers & action owners.",
        "Deliver: Gives you a scannable 60-second catch-up.",
      ],
      prompt: "Catch me up on [channel or topic]: summarize the key decisions made this week, open questions, and who owns each next step.",
    },
    {
      id: "meeting-actions",
      cat: "team",
      mode: "One-time · Files",
      cls: "ok",
      title: "Extract Action Items & Draft Follow-ups",
      desc: "Drop a meeting transcript or notes file here—Magnus pulls action items with owners and due dates and drafts follow-up emails.",
      tools: ["Files", "Calendar", "Approval Gate"],
      steps: [
        "Input: Drop a transcript or paste meeting notes.",
        "Extract: Lists action items with owners and due dates.",
        "Approval Gate: Drafts follow-up emails for your 1-click OK.",
      ],
      prompt: "Extract all action items, owners, and deadlines from the attached meeting notes, and draft follow-up emails for my approval.",
    },
  ];

  const state = { heroCat: "all", exploreCat: "all", query: "" };

  function firstName() {
    const n = (typeof App !== "undefined" && App.me?.name ? App.me.name : "").trim().split(/\s+/)[0];
    return n ? `Hello, ${n}` : "Hello";
  }

  const TOOL_META = {
    "Gmail / Outlook": { tone: "tone-mail", ic: "mail" },
    "Email": { tone: "tone-mail", ic: "mail" },
    "Gmail": { tone: "tone-mail", ic: "mail" },
    "Calendar": { tone: "tone-cal", ic: "calendar" },
    "GitHub": { tone: "tone-code", ic: "tasks" },
    "Slack": { tone: "tone-chat", ic: "chat" },
    "Teams": { tone: "tone-chat", ic: "chat" },
    "Cloud Computer": { tone: "tone-browser", ic: "globe" },
    "Files": { tone: "tone-doc", ic: "file" },
    "Google Drive": { tone: "tone-doc", ic: "folder" },
    "Approval Gate": { tone: "tone-gate", ic: "shield" },
  };

  /** @param {string} name */
  function toolBadge(name, compact = false) {
    const m = TOOL_META[name] || { tone: "tone-doc", ic: "file" };
    const svg = icon(m.ic);
    if (compact) return `<span class="flow-node ${m.tone}" title="${esc(name)}">${svg}</span>`;
    return `<span class="hero-tool ${m.tone}">${svg}<span>${esc(name)}</span></span>`;
  }

  const CAT_IMAGE = {
    ops: "card-inbox.jpg",
    research: "card-research.jpg",
    monitor: "card-monitor.jpg",
    eng: "card-eng.jpg",
    team: "card-monitor.jpg",
  };

  /** @param {Playbook} p */
  function cardHtml(p, expanded = false) {
    const tools = p.tools.map(t => toolBadge(t, false)).join("");
    const flow = p.tools.map(t => toolBadge(t, true)).join('<span class="flow-arrow" aria-hidden="true">→</span>');
    const img = CAT_IMAGE[p.cat] || "card-inbox.jpg";
    if (!expanded) {
      return `<button type="button" class="hello-card" data-starter-prompt="${esc(p.prompt)}">
        <div class="card-media"><img src="${esc(img)}" alt="" aria-hidden="true" loading="lazy"></div>
        <div class="hello-card-top"><span class="chip ${esc(p.cls)}">${esc(p.mode)}</span><div class="card-flow" aria-hidden="true">${flow}</div></div>
        <strong class="hello-card-title">${esc(p.title)}</strong>
        <p class="hello-card-desc">${esc(p.desc)}</p>
        <div class="hello-card-foot"><div class="hello-card-tools">${tools}</div><span class="hello-card-go">Try prompt →</span></div>
      </button>`;
    }
    return `<article class="explore-card">
      <div class="card-media"><img src="${esc(img)}" alt="" aria-hidden="true" loading="lazy"></div>
      <div class="explore-card-head"><span class="chip ${esc(p.cls)}">${esc(p.mode)}</span><div class="card-flow" aria-hidden="true">${flow}</div></div>
      <h3>${esc(p.title)}</h3>
      <p class="small muted">${esc(p.desc)}</p>
      <div class="hello-card-tools">${tools}</div>
      <details class="explore-steps"><summary class="small">See workflow steps &amp; prompt</summary>
        <ol class="small">${p.steps.map(s => `<li>${esc(s)}</li>`).join("")}</ol>
        <p class="explore-prompt small mono">${esc(p.prompt)}</p>
      </details>
      <div class="explore-card-foot"><button type="button" class="primary" data-explore-try="${esc(p.id)}">Use in new chat</button></div>
    </article>`;
  }

  function bannerHtml() {
    return `<div class="hello-banner" aria-hidden="true">
      <img class="hero-img-ag" src="hero-antigravity.jpg" alt="">
      <img class="hero-img-gem" src="hero-gemini.jpg" alt="">
      <div class="hello-banner-overlay"></div>
    </div>`;
  }

  function heroHtml() {
    const list = state.heroCat === "all" ? PLAYBOOKS.slice(0, 4) : PLAYBOOKS.filter(p => p.cat === state.heroCat).slice(0, 4);
    const tabs = CATEGORIES.map(c => `<button type="button" class="hero-tab${state.heroCat === c.id ? " on" : ""}" data-hero-cat="${esc(c.id)}" aria-pressed="${state.heroCat === c.id}">${esc(c.label)}</button>`).join("");
    return `<div class="hello-hero">
      <div class="hello-head">
        ${bannerHtml()}
        <h1 class="hello-title">${esc(firstName())}</h1>
        <p class="hello-sub">Your autonomous digital worker — connected to your apps, equipped with a cloud computer, and always asking before acting.</p>
        <div class="hello-caps">
          <a href="#/library" class="hello-cap">${icon("grid")}<span>Connect apps</span></a>
          <button type="button" class="hello-cap" data-open-discovery>${icon("calendar")}<span>Find automations</span></button>
          <button type="button" class="hello-cap" data-open-setup>${icon("shield")}<span>How Magnus works</span></button>
          <a href="#/explore" class="hello-cap">${icon("sparkles")}<span>All playbooks</span></a>
        </div>
      </div>
      <div class="hero-tabs" role="tablist" aria-label="Playbook categories">${tabs}</div>
      <div class="hello-cards">${list.map(p => cardHtml(p, false)).join("")}</div>
      <div class="hello-pillars">
        <div class="hello-pillar"><strong>1. Give a goal</strong><span class="small muted">Magnus plans multi-step work across your apps &amp; Cloud Computer.</span></div>
        <div class="hello-pillar"><strong>2. Runs on schedule</strong><span class="small muted">Turn any workflow into a daily check or multi-day follow-up.</span></div>
        <div class="hello-pillar"><strong>3. Pauses for your OK</strong><span class="small muted">Sending emails, moving invites, or ordering always asks first.</span></div>
      </div>
    </div>`;
  }

  /** @param {Event} ev @param {any} view */
  function handleHeroClick(ev, view) {
    const t = /** @type {HTMLElement} */ (ev.target);
    const card = t.closest(".hello-card");
    if (card) {
      const prompt = /** @type {HTMLElement} */ (card).dataset.starterPrompt || "";
      const box = view.section?.querySelector("textarea");
      if (box && prompt) { box.value = prompt; box.dispatchEvent(new Event("input", { bubbles: true })); box.focus(); }
      return true;
    }
    const tab = t.closest("[data-hero-cat]");
    if (tab) {
      state.heroCat = /** @type {HTMLElement} */ (tab).dataset.heroCat || "all";
      const hero = view.list?.querySelector(".hello-hero");
      if (hero) hero.outerHTML = heroHtml();
      return true;
    }
    if (t.closest("[data-open-discovery]")) { ev.preventDefault(); if (typeof openDiscoveryWizard === "function") openDiscoveryWizard().catch(() => {}); return true; }
    if (t.closest("[data-open-setup]")) { ev.preventDefault(); if (typeof openSetupWizard === "function") openSetupWizard().catch(() => {}); return true; }
    return false;
  }

  /** @param {string} id */
  function usePlaybook(id) {
    const p = PLAYBOOKS.find(x => x.id === id);
    if (!p) return;
    if (typeof App !== "undefined") App.appSearch = p.prompt;
    location.hash = "#/chat/new";
  }

  /** @param {HTMLElement} main */
  async function show(main) {
    const seq = typeof App !== "undefined" ? App.seq : 0;
    const connectors = await Resources.read("Apps", "/v1/connectors", "connectors", true).catch(() => []);
    if (typeof App !== "undefined" && seq !== App.seq) return;
    const mine = (connectors || []).filter((/** @type {any} */ c) => c.for_you !== false);
    const connected = mine.filter((/** @type {any} */ c) => c.state === "connected");
    const draw = () => {
      const q = state.query.trim().toLowerCase();
      const filtered = PLAYBOOKS.filter(p => (state.exploreCat === "all" || p.cat === state.exploreCat)
        && (!q || `${p.title} ${p.desc} ${p.tools.join(" ")}`.toLowerCase().includes(q)));
      const tabs = CATEGORIES.map(c => `<button type="button" class="hero-tab${state.exploreCat === c.id ? " on" : ""}" data-exp-cat="${esc(c.id)}" aria-pressed="${state.exploreCat === c.id}">${esc(c.label)}</button>`).join("");
      const appStrip = mine.slice(0, 6).map((/** @type {any} */ c) => {
        const on = c.state === "connected";
        return `<a href="#/library/${enc(c.name)}" class="explore-app${on ? " on" : ""}"><span>${esc(c.title || c.name)}</span><span class="chip ${on ? "ok" : ""}">${on ? "Connected" : "Connect"}</span></a>`;
      }).join("");
      paint(main, `<div class="explore-page">
        <div class="explore-hero">
          <div>
            <h1>Explore what Magnus can do</h1>
            <p class="muted">Magnus isn’t just a chat box—it works across your connected apps, runs a Cloud Computer to research and build, and pauses at approval gates so you stay in control.</p>
          </div>
          <div class="row">
            <button type="button" class="primary" data-open-discovery>Find automations</button>
            <button type="button" data-open-setup>Setup guide</button>
          </div>
        </div>
        <section class="explore-apps" aria-label="Connected apps">
          <div class="spread"><strong>Your connected apps <span class="muted small">(${connected.length} connected)</span></strong><a href="#/library" class="small">Manage all apps →</a></div>
          <div class="explore-app-row">${appStrip || '<span class="small muted">No apps configured on this server yet.</span>'}</div>
        </section>
        <div class="explore-controls">
          <div class="hero-tabs" role="tablist" aria-label="Filter playbooks">${tabs}</div>
          <input type="search" id="explore-search" placeholder="Filter playbooks…" value="${esc(state.query)}" aria-label="Filter playbooks">
        </div>
        <div class="explore-grid">${filtered.length ? filtered.map(p => cardHtml(p, true)).join("") : '<p class="muted">No playbooks match that filter.</p>'}</div>
      </div>`);
    };
    draw();
    main.onclick = e => {
      const t = /** @type {HTMLElement} */ (e.target);
      const cat = t.closest("[data-exp-cat]");
      if (cat) { state.exploreCat = /** @type {HTMLElement} */ (cat).dataset.expCat || "all"; draw(); return; }
      const tryBtn = t.closest("[data-explore-try]");
      if (tryBtn) { usePlaybook(/** @type {HTMLElement} */ (tryBtn).dataset.exploreTry || ""); return; }
      if (t.closest("[data-open-discovery]")) { if (typeof openDiscoveryWizard === "function") openDiscoveryWizard().catch(() => {}); return; }
      if (t.closest("[data-open-setup]")) { if (typeof openSetupWizard === "function") openSetupWizard().catch(() => {}); }
    };
    main.oninput = e => {
      const t = /** @type {HTMLInputElement} */ (e.target);
      if (t.id === "explore-search") { state.query = t.value; draw(); const inp = document.getElementById("explore-search"); if (inp) { inp.focus(); /** @type {HTMLInputElement} */ (inp).setSelectionRange(state.query.length, state.query.length); } }
    };
  }

  return { PLAYBOOKS, CATEGORIES, heroHtml, handleHeroClick, usePlaybook, show };
})();
