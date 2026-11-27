// Content-rich discovery: starter playbooks for the new-conversation hero
// and the full Explore gallery (#/explore) with app-aware readiness,
// active-schedule detection, inline 1-click run/schedule configuration,
// and on-demand idea scanning into For you.

const Discover = (() => {
  /**
   * @typedef {{ key: string, label: string, placeholder: string, token: string }} PlaybookField
   * @typedef {{ id: string, cat: string, mode: string, cls: string, title: string, desc: string, guardrail: string, requires: string[], needsLabel?: string, tools: string[], steps: string[], prompt: string, schedule?: Record<string, any>, fields?: PlaybookField[] }} Playbook
   */

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
      guardrail: "Drafts first · never auto-sends",
      requires: ["google-gmail", "microsoft-outlook"],
      needsLabel: "Gmail / Outlook",
      tools: ["Gmail / Outlook", "Calendar", "Approval Gate"],
      steps: [
        "Trigger: Runs automatically every weekday at 7:45 AM.",
        "Read & Check: Scans overnight email & checks calendar overlaps.",
        "Approval Gate: Pauses for your 1-click OK before sending or moving invites.",
        "Outcome: Updates calendar & sends confirmed replies.",
      ],
      schedule: { weekdays_at: "07:45" },
      prompt: "Every weekday at 7:45 AM, check my overnight emails for meeting requests, spot any calendar conflicts, propose two conflict-free time slots, and draft replies for my approval.",
    },
    {
      id: "supplier-rfq",
      cat: "research",
      mode: "Multi-step · Cloud Browser",
      cls: "warn",
      title: "Research Suppliers & Compare Quotes",
      desc: "Searches vendors in the Cloud Computer, builds a price & lead-time table, drafts quote request emails, and pauses before ordering.",
      guardrail: "Isolated public browser · drafts RFQs",
      requires: [],
      tools: ["Cloud Computer", "Gmail", "Approval Gate"],
      steps: [
        "Research: Compares specs, reviews, and verified suppliers in the Cloud Browser.",
        "Outreach: Drafts RFQ emails to top vendors for pricing & lead times.",
        "Approval Gate: Shows comparison table & pauses before sending or ordering.",
        "Outcome: Places approved order, saves receipt & tracks delivery.",
      ],
      fields: [{ key: "product", label: "Product or part to source", placeholder: "e.g. NEMA 23 stepper motors (qty 50)", token: "a product" }],
      prompt: "Help me research a product, compare prices across suppliers, draft quote request emails to the top 3 vendors, and pause for my approval before placing any order.",
    },
    {
      id: "stalled-threads",
      cat: "monitor",
      mode: "Continuous · Every 4h (14d)",
      cls: "sch",
      title: "Track Stalled Threads (48h+)",
      desc: "Watches open vendor and project threads for 14 days, drafts polite follow-ups after 48 hours of silence, and asks before escalating.",
      guardrail: "Drafts nudges · asks before escalating",
      requires: ["google-gmail", "microsoft-outlook", "slack"],
      needsLabel: "Email or Slack",
      tools: ["Email", "Slack", "Approval Gate"],
      steps: [
        "Monitor: Checks active project & vendor threads every 4 hours.",
        "Draft: Prepares polite follow-up when a contact is silent > 48 hours.",
        "Approval Gate: Asks before sending nudges or escalating blockers.",
        "Outcome: Keeps a live status brief updated in Files.",
      ],
      schedule: { every_minutes: 240, max_runs: 84 },
      prompt: "Monitor my active vendor and project threads for 14 days, draft polite follow-ups whenever a contact is silent for 48 hours, keep a live status brief updated, and ask me before escalating.",
    },
    {
      id: "pr-ci-triage",
      cat: "eng",
      mode: "Scheduled · Weekdays 9:00 AM",
      cls: "ok",
      title: "Daily PR Review Queue & CI Blockers",
      desc: "Ranks pull requests waiting on your review by age, CI status, and release impact, and summarizes failing checks without touching code.",
      guardrail: "Read-only · never touches code",
      requires: ["github"],
      needsLabel: "GitHub",
      tools: ["GitHub"],
      steps: [
        "Trigger: Runs every weekday at 9:00 AM.",
        "Read: Checks open PRs requesting your review & recent CI logs.",
        "Deliver: Sends a prioritized queue with blocker summaries.",
      ],
      schedule: { weekdays_at: "09:00" },
      prompt: "Every weekday at 9:00 AM, triage pull requests waiting for my review on GitHub, rank them by age and release impact, and summarize any CI blockers.",
    },
    {
      id: "exec-brief",
      cat: "ops",
      mode: "One-time · Multi-doc",
      cls: "ok",
      title: "Turn Docs into an Executive Brief",
      desc: "Reads uploaded files or Drive docs, extracts decisions, risks, and owners, and formats a crisp 1-page brief you can save or share.",
      guardrail: "Read-only · saves to your Files",
      requires: [],
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
      guardrail: "Isolated public browser · read-only",
      requires: [],
      tools: ["Cloud Computer", "Files"],
      steps: [
        "Browse: Visits public product & pricing pages in the Cloud Browser.",
        "Compare: Structures features, pricing tiers & differentiators.",
        "Outcome: Delivers a comparison table and saves a brief to Files.",
      ],
      fields: [{ key: "space", label: "Market or category", placeholder: "e.g. enterprise AI agent platforms", token: "[my space]" }],
      prompt: "Research the top 4 competitors in [my space], compare their pricing tiers, key features, and recent launches in a structured table, and save the brief to Files.",
    },
    {
      id: "weekly-digest",
      cat: "monitor",
      mode: "Scheduled · Fridays 4:00 PM",
      cls: "sch",
      title: "Weekly Project Risk & Status Digest",
      desc: "Pulls updates across project channels and email threads every Friday, highlights overdue items, and drafts a status summary.",
      guardrail: "Drafts brief · never shares automatically",
      requires: ["slack", "google-gmail", "microsoft-outlook"],
      needsLabel: "Slack or Email",
      tools: ["Slack", "Email", "Files"],
      steps: [
        "Trigger: Runs every Friday at 4:00 PM.",
        "Scan: Reads project threads for shipped work & stalled items.",
        "Approval Gate: Drafts the weekly summary for your review before sharing.",
      ],
      schedule: { weekly_at: { days: ["fri"], at: "16:00" } },
      prompt: "Every Friday at 4:00 PM, gather updates from my active project threads, highlight overdue items and risks, and draft a concise weekly status brief for my approval.",
    },
    {
      id: "ci-repro",
      cat: "eng",
      mode: "Multi-step · Cloud Computer",
      cls: "warn",
      title: "Investigate a Failing Build or Bug",
      desc: "Reads CI logs and repository context, reproduces the failure in the Cloud Computer, and proposes a targeted fix for your review.",
      guardrail: "Isolated sandbox · proposes fix only",
      requires: ["github"],
      needsLabel: "GitHub",
      tools: ["GitHub", "Cloud Computer"],
      steps: [
        "Inspect: Reads failing CI run logs and recent commits.",
        "Reproduce: Runs tests in the Cloud Computer sandbox.",
        "Approval Gate: Presents root cause & proposed fix for your review.",
      ],
      fields: [{ key: "repo", label: "Repository", placeholder: "owner/repo", token: "[owner/repo]" }],
      prompt: "Investigate the latest failing CI run in [owner/repo], reproduce the failure in the Cloud Computer, explain the root cause, and propose a targeted fix for my review.",
    },
    {
      id: "channel-catchup",
      cat: "team",
      mode: "One-time · Team",
      cls: "ok",
      title: "Catch Up on Quiet Team Threads",
      desc: "Summarizes busy Slack or team channels, pulls out decisions made, open questions, and who owns each next step.",
      guardrail: "Read-only summary",
      requires: ["slack"],
      needsLabel: "Slack",
      tools: ["Slack", "Teams"],
      steps: [
        "Read: Reviews recent messages in the channel you name.",
        "Extract: Separates decisions, blockers & action owners.",
        "Deliver: Gives you a scannable 60-second catch-up.",
      ],
      fields: [{ key: "channel", label: "Channel or topic", placeholder: "#engineering or Q4 launch", token: "[channel or topic]" }],
      prompt: "Catch me up on [channel or topic]: summarize the key decisions made this week, open questions, and who owns each next step.",
    },
    {
      id: "meeting-actions",
      cat: "team",
      mode: "One-time · Files",
      cls: "ok",
      title: "Extract Action Items & Draft Follow-ups",
      desc: "Drop a meeting transcript or notes file here—Magnus pulls action items with owners and due dates and drafts follow-up emails.",
      guardrail: "Drafts follow-ups · never auto-sends",
      requires: [],
      tools: ["Files", "Calendar", "Approval Gate"],
      steps: [
        "Input: Drop a transcript or paste meeting notes.",
        "Extract: Lists action items with owners and due dates.",
        "Approval Gate: Drafts follow-up emails for your 1-click OK.",
      ],
      prompt: "Extract all action items, owners, and deadlines from the attached meeting notes, and draft follow-up emails for my approval.",
    },
  ];

  const state = {
    heroCat: "all",
    exploreCat: "all",
    query: "",
    openConfig: "",
    scanMsg: "",
    scanning: false,
    /** @type {Record<string, string>} */
    fieldValues: {},
    /** @type {Record<string, string>} */
    timeValues: {},
  };

  function firstName() {
    const heading = typeof Theme !== "undefined" && Theme.get?.()?.features?.introHeading;
    if (heading) return heading;
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
    ops: "card-inbox.webp",
    research: "card-research.webp",
    monitor: "card-monitor.webp",
    eng: "card-eng.webp",
    team: "card-monitor.webp",
  };

  /** @param {Playbook} p @param {string[]} connectedNames */
  function isReady(p, connectedNames) {
    if (!p.requires || !p.requires.length) return true;
    return p.requires.some(r => connectedNames.some(c => c.startsWith(r)));
  }

  /** @param {Playbook} p @param {any[]} allMine */
  function connectHref(p, allMine) {
    const match = (allMine || []).find(c => (p.requires || []).some(r => String(c.name || "").startsWith(r)));
    return match ? `#/library/${enc(match.name)}` : "#/library";
  }

  /** @param {Playbook} p @param {any[]} schedules */
  function activeSchedule(p, schedules) {
    const needle = p.title.toLowerCase();
    return (schedules || []).find(s => {
      if (s.status === "ended" || s.status === "cancelled") return false;
      const hay = `${s.title || ""} ${s.text || ""}`.toLowerCase();
      return hay.includes(needle);
    }) || null;
  }

  const scheduleTime = s => s?.daily_at || s?.weekdays_at || s?.weekly_at?.at || "";

  /** @param {Playbook} p */
  function buildPrompt(p) {
    let out = p.prompt;
    for (const f of p.fields || []) {
      const val = (state.fieldValues[`${p.id}:${f.key}`] || "").trim();
      if (val) out = out.replace(f.token, val);
    }
    const baseTime = scheduleTime(p.schedule);
    const time = (state.timeValues[p.id] || baseTime).trim();
    if (time && baseTime && time !== baseTime) {
      out = out.replace(/\b\d{1,2}:\d{2}\s*(?:AM|PM)\b/i, time);
    }
    return out;
  }

  /** @param {Playbook} p @param {boolean} expanded @param {{ connectedNames?: string[], allMine?: any[], schedules?: any[] }} [ctx] */
  function cardHtml(p, expanded = false, ctx = {}) {
    const tools = p.tools.map(t => toolBadge(t, false)).join("");
    const flow = p.tools.map(t => toolBadge(t, true)).join('<span class="flow-arrow" aria-hidden="true">→</span>');
    const img = CAT_IMAGE[p.cat] || "card-inbox.webp";
    if (!expanded) {
      const conn = typeof App !== "undefined" && Array.isArray(App.connectors)
        ? App.connectors.filter(c => (typeof stateOf === "function" ? stateOf(c) : c.state) === "connected").map(c => c.name)
        : null;
      const needsApp = conn && p.requires.length > 0 && !isReady(p, conn);
      return `<button type="button" class="hello-card" data-starter-prompt="${esc(p.prompt)}"${needsApp ? ` data-needs-app="${esc(p.needsLabel || "app")}"` : ""}>
        <div class="card-media"><img src="${esc(img)}" alt="" aria-hidden="true" loading="lazy"></div>
        <div class="hello-card-top"><span class="chip ${esc(p.cls)}">${esc(p.mode)}</span>${needsApp ? `<span class="chip warn">Needs ${esc(p.needsLabel || "app")}</span>` : ""}<div class="card-flow" aria-hidden="true">${flow}</div></div>
        <strong class="hello-card-title">${esc(p.title)}</strong>
        <p class="hello-card-desc">${esc(p.desc)}</p>
        <div class="hello-card-foot"><span class="hello-card-meta">${esc(p.tools.join(" · "))}</span><span class="hello-card-go">${needsApp ? `Connect ${esc(p.needsLabel || "app")} →` : "Try →"}</span></div>
      </button>`;
    }
    const connectedNames = ctx.connectedNames || [];
    const ready = isReady(p, connectedNames);
    const active = activeSchedule(p, ctx.schedules || []);
    const hasInline = Boolean((p.fields && p.fields.length) || p.schedule);
    const configOpen = state.openConfig === p.id;
    const statusChip = active
      ? `<span class="chip ok">Active</span>`
      : ready && connectedNames.length > 0 && p.requires.length > 0
        ? `<span class="chip ok">Ready</span>`
        : !ready && connectedNames.length > 0
          ? `<span class="chip warn">Needs ${esc(p.needsLabel || "app")}</span>`
          : "";
    const baseTime = scheduleTime(p.schedule);
    const inlineHtml = configOpen ? `<div class="explore-inline" data-inline-for="${esc(p.id)}">
      ${(p.fields || []).map(f => `<label class="small"><span>${esc(f.label)}</span><input type="text" data-explore-field="${esc(p.id)}:${esc(f.key)}" placeholder="${esc(f.placeholder)}" value="${esc(state.fieldValues[`${p.id}:${f.key}`] || "")}"></label>`).join("")}
      ${baseTime ? `<label class="small"><span>Run time</span><input type="time" data-explore-time="${esc(p.id)}" value="${esc(state.timeValues[p.id] || baseTime)}"></label>` : ""}
      <div class="explore-inline-actions">
        ${p.schedule ? `<button type="button" class="primary" data-explore-schedule="${esc(p.id)}">Create schedule</button>` : ""}
        <button type="button" class="${p.schedule ? "" : "primary"}" data-explore-launch="${esc(p.id)}">${p.schedule ? "Run once in chat" : "Start now"}</button>
        <button type="button" class="link small" data-explore-config="">Cancel</button>
      </div>
    </div>` : "";
    let footerHtml;
    if (active) {
      footerHtml = `<a href="#/schedules" class="small">Manage routine →</a><button type="button" data-explore-try="${esc(p.id)}">Run in chat</button>`;
    } else if (!ready && connectedNames.length > 0) {
      footerHtml = `<button type="button" class="link small" data-explore-try="${esc(p.id)}">Preview in chat</button><a href="${esc(connectHref(p, ctx.allMine || []))}" class="explore-connect">Connect ${esc(p.needsLabel || "app")}</a>`;
    } else if (hasInline && !configOpen) {
      footerHtml = `<button type="button" class="link small" data-explore-try="${esc(p.id)}">Edit in chat</button><button type="button" class="primary" data-explore-config="${esc(p.id)}">${p.schedule ? "Schedule or run" : "Configure & run"}</button>`;
    } else if (!configOpen) {
      footerHtml = `<button type="button" class="primary" data-explore-try="${esc(p.id)}">Use in new chat</button>`;
    } else {
      footerHtml = "";
    }
    return `<article class="explore-card${ready ? "" : " explore-needs-app"}" data-playbook="${esc(p.id)}">
      <div class="card-media"><img src="${esc(img)}" alt="" aria-hidden="true" loading="lazy"></div>
      <div class="explore-card-head"><span class="chip ${esc(p.cls)}">${esc(p.mode)}</span>${statusChip}<div class="card-flow" aria-hidden="true">${flow}</div></div>
      <h3>${esc(p.title)}</h3>
      <p class="small muted">${esc(p.desc)}</p>
      <div class="explore-guardrail small">${icon("shield")}<span>${esc(p.guardrail)}</span></div>
      <div class="hello-card-tools">${tools}</div>
      <details class="explore-steps"><summary class="small">See workflow steps &amp; prompt</summary>
        <ol class="small">${p.steps.map(s => `<li>${esc(s)}</li>`).join("")}</ol>
        <p class="explore-prompt small mono">${esc(p.prompt)}</p>
      </details>
      ${inlineHtml}
      ${footerHtml ? `<div class="explore-card-foot">${footerHtml}</div>` : ""}
    </article>`;
  }

  function bannerHtml() {
    return `<div class="hello-banner" aria-hidden="true">
      <img class="hero-img-ag" src="hero-antigravity.webp" alt="">
      <img class="hero-img-gem" src="hero-gemini.webp" alt="">
      <div class="hello-banner-overlay"></div>
    </div>`;
  }

  function heroHtml() {
    const list = state.heroCat === "all" ? PLAYBOOKS.slice(0, 4) : PLAYBOOKS.filter(p => p.cat === state.heroCat).slice(0, 4);
    const tabs = CATEGORIES.map(c => `<button type="button" class="hero-tab${state.heroCat === c.id ? " on" : ""}" data-hero-cat="${esc(c.id)}" aria-pressed="${state.heroCat === c.id}">${esc(c.label)}</button>`).join("");
    const auroraChips = PLAYBOOKS.slice(0, 4).map(p =>
      `<button type="button" class="hello-chip aurora-hello-chip" data-starter-prompt="${esc(p.prompt)}">${esc(p.title)}</button>`
    ).join("");
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
      <div class="aurora-hello-chips" aria-label="Suggested starters">${auroraChips}</div>
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

  /** @param {string} id @param {string} [overridePrompt] */
  function usePlaybook(id, overridePrompt) {
    const p = PLAYBOOKS.find(x => x.id === id);
    if (!p) return;
    if (typeof App !== "undefined") App.appSearch = overridePrompt || p.prompt;
    location.hash = "#/chat/new";
  }

  /** @param {HTMLElement} main */
  async function show(main) {
    const seq = typeof App !== "undefined" ? App.seq : 0;
    const connectors = await Resources.read("Apps", "/v1/connectors", "connectors", true).catch(() => []);
    if (typeof App !== "undefined" && seq !== App.seq) return;
    const mine = (connectors || []).filter((/** @type {any} */ c) => c.for_you !== false);
    const connected = mine.filter((/** @type {any} */ c) => c.state === "connected");
    const connectedNames = connected.map((/** @type {any} */ c) => String(c.name || ""));

    const filteredPlaybooks = () => {
      const q = state.query.trim().toLowerCase();
      const base = PLAYBOOKS.filter(p => (state.exploreCat === "all" || p.cat === state.exploreCat)
        && (!q || `${p.title} ${p.desc} ${p.guardrail} ${p.tools.join(" ")}`.toLowerCase().includes(q)));
      if (!connectedNames.length) return base;
      return [...base].sort((a, b) => Number(isReady(b, connectedNames)) - Number(isReady(a, connectedNames)));
    };

    const gridHtml = () => {
      const schedules = typeof App !== "undefined" ? (App.schedules || []) : [];
      const ctx = { connectedNames, allMine: mine, schedules };
      const filtered = filteredPlaybooks();
      return `<div class="explore-grid">${filtered.length ? filtered.map(p => cardHtml(p, true, ctx)).join("") : '<p class="muted">No playbooks match that filter.</p>'}</div>`;
    };

    const ideasMarkup = () => typeof ExploreIdeas !== "undefined" && typeof card === "function" ? ExploreIdeas.html(App.inbox, i => card(i)) : "";

    const pageHtml = () => {
      const tabs = CATEGORIES.map(c => `<button type="button" class="hero-tab${state.exploreCat === c.id ? " on" : ""}" data-exp-cat="${esc(c.id)}" aria-pressed="${state.exploreCat === c.id}">${esc(c.label)}</button>`).join("");
      const appStrip = mine.slice(0, 8).map((/** @type {any} */ c) => {
        const on = c.state === "connected";
        return `<a href="#/library/${enc(c.name)}" class="explore-app${on ? " on" : ""}"><span>${esc(c.title || c.name)}</span><span class="chip ${on ? "ok" : ""}">${on ? "Connected" : "Connect"}</span></a>`;
      }).join("");
      const scanNotice = state.scanMsg ? `<p class="small muted explore-scan-msg">${esc(state.scanMsg)}</p>` : "";
      return `<div class="explore-page">
        <div class="explore-hero">
          <div>
            <h1>Explore what Magnus can do</h1>
            <p class="muted">Live ideas from your connected apps, plus ready-to-run playbooks with clear guardrails.</p>
            ${scanNotice}
          </div>
          <div class="row">
            <button type="button" class="primary" data-explore-scan ${state.scanning ? "disabled" : ""}>${state.scanning ? "Scanning…" : "Scan for new ideas"}</button>
            <button type="button" data-open-setup>Setup guide</button>
          </div>
        </div>
        <div data-explore-ideas>${typeof ExploreIdeas !== "undefined" && typeof card === "function" ? ExploreIdeas.html(App.inbox, i => card(i)) : ""}</div>
        <section class="explore-apps" aria-label="Connected apps">
          <div class="spread"><strong>Your connected apps <span class="muted small">(${connected.length} connected)</span></strong><a href="#/library" class="small">Manage all apps →</a></div>
          <div class="explore-app-row">${appStrip || '<span class="small muted">No apps configured on this server yet.</span>'}</div>
        </section>
        <div class="explore-controls">
          <div class="hero-tabs" role="tablist" aria-label="Filter playbooks">${tabs}</div>
          <input type="search" id="explore-search" placeholder="Filter playbooks…" value="${esc(state.query)}" aria-label="Filter playbooks">
        </div>
        ${gridHtml()}
      </div>`;
    };

    let lastGridMarkup = "";
    let lastScanMarkup = "";
    const draw = () => {
      const nextGrid = gridHtml();
      const nextScan = state.scanMsg ? `<p class="small muted explore-scan-msg">${esc(state.scanMsg)}</p>` : "";
      if (typeof main.querySelector === "function" && main.querySelector(".explore-page")) {
        const ideasBox = main.querySelector("[data-explore-ideas]");
        if (ideasBox) paint(ideasBox, ideasMarkup());
        const scanBtn = /** @type {?HTMLButtonElement} */ (main.querySelector("[data-explore-scan]"));
        if (scanBtn) {
          scanBtn.disabled = !!state.scanning;
          scanBtn.textContent = state.scanning ? "Scanning…" : "Scan for new ideas";
        }
        if (nextScan !== lastScanMarkup) {
          const heroText = main.querySelector(".explore-hero > div");
          const existing = main.querySelector(".explore-scan-msg");
          if (existing) existing.remove();
          if (heroText && nextScan) heroText.insertAdjacentHTML("beforeend", nextScan);
          lastScanMarkup = nextScan;
        }
        if (nextGrid !== lastGridMarkup) {
          const grid = main.querySelector(".explore-grid");
          if (grid) { grid.outerHTML = nextGrid; lastGridMarkup = nextGrid; }
        }
        main._html = pageHtml();
        return;
      }
      lastGridMarkup = nextGrid;
      lastScanMarkup = nextScan;
      paint(main, pageHtml());
    };

    const drawGridOnly = () => {
      if (typeof main.querySelector !== "function") { draw(); return; }
      const grid = main.querySelector(".explore-grid");
      if (!grid) { draw(); return; }
      const nextGrid = gridHtml();
      if (nextGrid !== lastGridMarkup) {
        grid.outerHTML = nextGrid;
        lastGridMarkup = nextGrid;
      }
      for (const btn of main.querySelectorAll("[data-exp-cat]")) {
        const on = /** @type {HTMLElement} */ (btn).dataset.expCat === state.exploreCat;
        btn.classList.toggle("on", on);
        btn.setAttribute("aria-pressed", String(on));
      }
      main._html = pageHtml();
    };

    draw();
    // An idea started or dismissed here redraws it, from the inbox as the
    // server has it then.
    if (typeof App !== "undefined" && typeof onLeave === "function") {
      const redraw = async () => { if (typeof pollInbox === "function") await pollInbox(); draw(); };
      App.redraw = redraw;
      onLeave(() => { if (App.redraw === redraw) App.redraw = null; });
    }
    main.onclick = async e => {
      const t = /** @type {HTMLElement} */ (e.target);
      const cat = t.closest("[data-exp-cat]");
      if (cat) { state.exploreCat = /** @type {HTMLElement} */ (cat).dataset.expCat || "all"; drawGridOnly(); return; }
      const cfg = t.closest("[data-explore-config]");
      if (cfg) { state.openConfig = /** @type {HTMLElement} */ (cfg).dataset.exploreConfig || ""; drawGridOnly(); return; }
      const schedBtn = t.closest("[data-explore-schedule]");
      if (schedBtn) {
        const id = /** @type {HTMLElement} */ (schedBtn).dataset.exploreSchedule || "";
        const p = PLAYBOOKS.find(x => x.id === id);
        if (!p) return;
        const prompt = buildPrompt(p);
        const sched = { ...(p.schedule || {}) };
        if (state.timeValues[id]) {
          if (sched.daily_at) sched.daily_at = state.timeValues[id];
          else if (sched.weekdays_at) sched.weekdays_at = state.timeValues[id];
          else if (sched.weekly_at) sched.weekly_at = { ...sched.weekly_at, at: state.timeValues[id] };
        }
        if (typeof Server !== "undefined" && Server.schedules?.create) {
          try {
            await Server.schedules.create({ text: `${p.title}: ${prompt}`, ...sched });
            state.openConfig = "";
            const msg = `Scheduled "${p.title}". Manage it anytime under Routines.`;
            state.scanMsg = msg;
            if (typeof toast === "function") toast(msg);
            if (typeof pollSchedules === "function") await pollSchedules();
            draw();
            const EXPLORE_STATUS_MS = 4000;
            setTimeout(() => { if (state.scanMsg === msg) { state.scanMsg = ""; if (main.isConnected) draw(); } }, EXPLORE_STATUS_MS);
            return;
          } catch { /* fall through to chat */ }
        }
        usePlaybook(id, prompt);
        return;
      }
      const launchBtn = t.closest("[data-explore-launch]");
      if (launchBtn) {
        const id = /** @type {HTMLElement} */ (launchBtn).dataset.exploreLaunch || "";
        const p = PLAYBOOKS.find(x => x.id === id);
        if (!p) return;
        const prompt = buildPrompt(p);
        state.openConfig = "";
        if (typeof Server !== "undefined" && Server.chat?.send) {
          try {
            const res = await Server.chat.send({ text: prompt });
            if (res?.thread) { location.hash = `#/chat/${enc(res.thread)}`; return; }
          } catch { /* fall through to pre-filled new chat */ }
        }
        usePlaybook(id, prompt);
        return;
      }
      const tryBtn = t.closest("[data-explore-try]");
      if (tryBtn) { usePlaybook(/** @type {HTMLElement} */ (tryBtn).dataset.exploreTry || ""); return; }
      if (t.closest("[data-explore-scan]")) {
        if (state.scanning) return;
        state.scanning = true;
        state.scanMsg = "Starting a read-only scan of your connected apps…";
        draw();
        try {
          const res = typeof Server !== "undefined" && Server.explore?.scan ? await Server.explore.scan() : null;
          state.scanMsg = res?.started
            ? "Scanning your apps in the background — new ideas will appear in For you."
            : "A scan is already running or paused by the kill switch.";
        } catch {
          state.scanMsg = "Could not start scan right now. Try again in a moment.";
        } finally {
          state.scanning = false;
          draw();
        }
        return;
      }
      if (t.closest("[data-open-discovery]")) { if (typeof openDiscoveryWizard === "function") openDiscoveryWizard().catch(() => {}); return; }
      if (t.closest("[data-open-setup]")) { if (typeof openSetupWizard === "function") openSetupWizard().catch(() => {}); }
    };
    main.oninput = e => {
      const t = /** @type {HTMLInputElement} */ (e.target);
      if (t.id === "explore-search") {
        state.query = t.value;
        drawGridOnly();
        return;
      }
      const fk = t.dataset?.exploreField;
      if (fk) { state.fieldValues[fk] = t.value; return; }
      const tk = t.dataset?.exploreTime;
      if (tk) { state.timeValues[tk] = t.value; }
    };
  }

  return { PLAYBOOKS, CATEGORIES, heroHtml, handleHeroClick, usePlaybook, show };
})();
