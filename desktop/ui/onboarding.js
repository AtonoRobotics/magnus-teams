// Onboarding Setup Wizard & Automation Discovery Wizard
// Setup Wizard: 3-step dismissible modal (Connect Apps -> How Magnus Works -> Pick a Task).
// Discovery Wizard: separate 3-step agent-led modal to discover & safely activate automations.

UI.start = '<path d="M5 21V4"/><path d="M5 4h12l-2.5 4L17 12H5"/>';

const GUIDE_PLATFORMS = [
  { key: "google", title: "Google", match: (/** @type {string} */ n) => n.startsWith("google-"), unlocks: ["Triage inbox & draft replies", "Resolve calendar conflicts", "Pull Drive docs"] },
  { key: "microsoft", title: "Microsoft", match: (/** @type {string} */ n) => n.startsWith("microsoft-"), unlocks: ["Morning inbox summary", "Rebook overlapping meetings", "Vendor thread tracking"] },
  { key: "slack", title: "Slack", match: (/** @type {string} */ n) => n === "slack", unlocks: ["Catch up on quiet threads", "Extract decisions & owners", "Flag blockers"] },
  { key: "github", title: "GitHub", match: (/** @type {string} */ n) => n === "github", unlocks: ["Daily PR review queue", "Summarize failing CI", "Track release blockers"] },
];

const WIZARD_TIERS = {
  simple: {
    tag: "Simple · Daily Hygiene", cls: "ok", title: "Check emails & resolve schedule conflicts",
    desc: "Scans overnight email for meeting requests, spots double-bookings, proposes 2 open slots, and drafts replies.",
    schedule: "Every weekday at 7:45 AM",
    prompt: "Every weekday at 7:45 AM, check my overnight emails for meeting requests, spot any calendar conflicts, propose two conflict-free time slots, and draft replies for my approval.",
    steps: [
      { k: "1 · Trigger", v: "Runs automatically at 7:45 AM on weekdays." },
      { k: "2 · Read & Check", v: "Scans Gmail/Outlook for requests & checks Calendar." },
      { k: "3 · Approval Gate", v: "Asks your 1-click OK before moving invites or replying.", gate: true },
      { k: "4 · Outcome", v: "Updates calendar & sends confirmations cleanly." },
    ],
  },
  complex: {
    tag: "Multi-Step · End-to-End", cls: "warn", title: "Research product, contact suppliers & order",
    desc: "Finds top options, compares prices, emails suppliers for quotes/lead times, and pauses for your OK before ordering.",
    schedule: "On-demand goal · Multi-step workflow",
    prompt: "Help me research a product, compare prices across suppliers, draft quote request emails to the top 3 vendors, and pause for my approval before placing any order.",
    steps: [
      { k: "1 · Research", v: "Compares specs, reviews, and verified suppliers." },
      { k: "2 · Outreach", v: "Drafts & sends RFQ emails for pricing and lead times." },
      { k: "3 · Approval Gate", v: "Shows price comparison & pauses before ordering.", gate: true },
      { k: "4 · Outcome", v: "Places approved order, saves receipt & tracks delivery." },
    ],
  },
  long: {
    tag: "Long-Running · Continuous", cls: "sch", title: "Multi-week vendor & project follow-up",
    desc: "Monitors open threads for days or weeks, nudges silent contacts after 48h, updates a live brief, and escalates blockers.",
    schedule: "Continuous · Checks every 4 hours",
    prompt: "Monitor my active vendor and project threads for 14 days, draft polite follow-ups whenever a contact is silent for 48 hours, keep a live status brief updated, and ask me before escalating.",
    steps: [
      { k: "1 · Monitor", v: "Watches email & Slack threads tied to your project." },
      { k: "2 · Follow Up", v: "Drafts polite nudges when replies stall > 48 hours." },
      { k: "3 · Approval Gate", v: "Asks before escalating overdue items or scope changes.", gate: true },
      { k: "4 · Outcome", v: "Keeps a live brief updated and only pings when needed." },
    ],
  },
};

const DISCOVERY_PROPOSALS = [
  { id: "d-inbox", focus: "inbox", requires: ["google-gmail", "microsoft-outlook", "google-calendar", "microsoft-calendar"], needsLabel: "Gmail/Outlook", tier: "simple", badge: "Simple · Weekdays 7:45 AM", cls: "ok", title: "Inbox Triage & Schedule Conflict Resolver", desc: "Checks overnight email for meeting requests, resolves calendar overlaps with 2 open options, and prepares reply drafts.", mode: "ask", schedule: { daily_at: "07:45" }, on: true },
  { id: "d-procure", focus: "vendors", requires: [], needsLabel: "", tier: "complex", badge: "Multi-Step · On Demand", cls: "warn", title: "Product Research, Supplier Quotes & Order Placement", desc: "Researches specs & pricing, drafts supplier quote requests, builds a comparison table, and pauses for approval before ordering.", mode: "ask", on: true },
  { id: "d-followup", focus: "followup", requires: ["google-gmail", "microsoft-outlook", "slack"], needsLabel: "Email or Slack", tier: "long", badge: "Long-Running · Every 4h (14 days)", cls: "sch", title: "Continuous Vendor & Deliverable Follow-Up", desc: "Watches open threads for stalled replies (>48h), drafts polite follow-ups, updates your live status brief, and escalates blockers.", mode: "draft", schedule: { every_minutes: 240, max_runs: 84 }, on: true },
  { id: "d-eng-pr", focus: "eng", requires: ["github"], needsLabel: "GitHub", tier: "simple", badge: "Simple · Weekdays 9:00 AM", cls: "ok", title: "Daily PR Review Queue & Blocker Triage", desc: "Ranks pull requests waiting on your review by age, CI status, and release impact without touching code.", mode: "draft", schedule: { daily_at: "09:00" }, on: true },
  { id: "d-eng-ci", focus: "eng", requires: ["github"], needsLabel: "GitHub", tier: "long", badge: "Long-Running · Daily", cls: "sch", title: "CI Failure & Flaky Test Watcher", desc: "Tracks recurring CI failures across recent runs, groups root causes, and prepares draft issues for your 1-click confirmation.", mode: "ask", schedule: { daily_at: "08:30" }, on: false },
];

const Guide = {
  /** @type {?any} */ facts: null,
  /** @type {?number} */ done: null,
  counted: 0,
  setup: { step: 1, tier: "simple", sim: "" },
  disc: { step: 1, filter: "all", focus: new Set(["inbox", "vendors", "followup"]), note: "", items: DISCOVERY_PROPOSALS.map(p => ({ ...p })) },
};

/** @param {any} f */
function guideSteps(f) {
  const mine = (f.connectors || []).filter((/** @type {any} */ c) => c.for_you !== false);
  const platforms = GUIDE_PLATFORMS.map(p => {
    const apps = mine.filter((/** @type {any} */ c) => p.match(c.name));
    return { ...p, apps, connected: apps.filter((/** @type {any} */ c) => c.state === "connected").length };
  });
  const apps = { done: platforms.some(p => p.connected > 0), platforms };
  const team = { done: (f.workspaces || []).length > 0 };
  const workflow = { done: (f.schedules || []).length > 0 };
  return { apps, team, workflow, done: [apps, team, workflow].filter(s => s.done).length };
}

async function guideFacts() {
  const [connectors, workspaces] = await Promise.all([
    Resources.read("Apps", "/v1/connectors", "connectors", true).then(c => c || []).catch(() => []),
    Server.workspaces.list().catch(() => []),
  ]);
  return { connectors, workspaces, schedules: App.schedules || [], threads: App.threads || [] };
}

/** Not a sidebar menu item anymore; wizards open as dismissible modals. */
function guideNavItem() { return ""; }

const guideNewcomer = () => !(App.tasks || []).length && (App.threads || []).every((/** @type {any} */ t) => t.opener);

async function guideFirstRun() {
  if (load("guide-opened", false) || load("guide", {}).hidden || !guideNewcomer()) return;
  save("guide-opened", true);
  await openSetupWizard().catch(() => {});
}

/** @param {string} text */
function guideAsk(text) {
  save("guide-asked", text);
  App.appSearch = text;
  location.hash = "#/chat/new";
}

function wizardDialog(id, label) {
  let d = /** @type {HTMLDialogElement | null} */ (document.getElementById(id));
  if (!d) {
    d = document.createElement("dialog");
    d.id = id;
    d.className = "wizard-modal";
    d.setAttribute("aria-label", label);
    document.body.appendChild(d);
    d.addEventListener("click", e => { if (e.target === d) d.close(); });
  }
  return d;
}

function setupStepHtml(f) {
  const s = guideSteps(f), step = Guide.setup.step;
  if (step === 1) {
    const connectedCount = s.apps.platforms.reduce((n, p) => n + p.connected, 0);
    const cards = s.apps.platforms.map(p => {
      const app = p.apps[0];
      const st = app ? stateOf(app) : "unavailable";
      const isConn = p.connected > 0;
      let btn = `<span class="small muted">Not configured</span>`;
      if (app) {
        if (isConn) btn = `<span class="chip ok">Connected</span>`;
        else if (st === "connecting") btn = `<button class="link" data-act="connect-page" data-url="${esc(Connecting[app.name] || "")}">Finish sign-in</button>`;
        else if (st === "needs_attention") btn = `<button data-act="connect" data-app="${esc(app.name)}">Sign in again</button>`;
        else if (app.ready !== false) btn = `<button class="primary" data-act="connect" data-app="${esc(app.name)}">Connect</button>`;
      }
      return `<div class="wiz-card${isConn ? " connected" : ""}">
        <div class="wiz-card-top">
          <div class="wiz-card-id">${app ? tile(app) : ""}<div style="min-width:0"><strong>${esc(p.title)}</strong><div class="small muted">${p.apps.length ? `${p.apps.length} app${p.apps.length > 1 ? "s" : ""}` : "Unavailable"}</div></div></div>
          ${btn}
        </div>
        <div class="wiz-unlocks">${p.unlocks.map(u => `<span class="wiz-chip">${esc(u)}</span>`).join("")}</div>
      </div>`;
    }).join("");
    return `<h2>Connect the apps Magnus works in</h2>
      <p class="wiz-sub">Magnus isn’t just a search box—it reads context across your tools, resolves conflicts, and prepares drafts. Connect an app or skip ahead to test.</p>
      <div class="wiz-grid-2">${cards}</div>
      <div class="wiz-trust"><span><strong>Least-privilege &amp; vault-backed:</strong> OAuth tokens stay in your vault. Magnus never sends emails or spends money without your rule or approval.</span><strong>${connectedCount} connected</strong></div>`;
  }
  if (step === 2) {
    const sim = Guide.setup.sim;
    const simMsg = sim === "once" ? `<p class="wiz-note ok">Approved once. Magnus placed the order and saved the receipt.</p>`
      : sim === "always" ? `<p class="wiz-note ok">Rule saved: Auto-approve verified supplier orders under $500.</p>`
      : sim === "deny" ? `<p class="wiz-note bad">Declined. Magnus paused the order and asked what to change.</p>` : "";
    return `<h2>How Magnus works safely on your behalf</h2>
      <p class="wiz-sub">Instead of waiting for you to type every prompt, Magnus runs tasks in the background and pauses at sensitive moments so you stay in control.</p>
      <div class="wiz-grid-3">
        <div class="wiz-card"><span class="small" style="color:var(--accent-text);font-weight:600;">1 · Trigger or Schedule</span><strong>Runs without you</strong><span class="small muted">Starts on a schedule, when a message arrives, or from a goal you hand off.</span></div>
        <div class="wiz-card"><span class="small" style="color:var(--accent-text);font-weight:600;">2 · Cross-App Execution</span><strong>Does the legwork</strong><span class="small muted">Reads email &amp; calendar, compares quotes, drafts replies, and builds briefs.</span></div>
        <div class="wiz-card"><span class="small" style="color:var(--accent-text);font-weight:600;">3 · Approval Gate</span><strong>Asks before acting</strong><span class="small muted">External actions—sending email, moving invites, or ordering—pause for your OK.</span></div>
      </div>
      <div class="wiz-sim">
        <div><div class="small" style="color:var(--warn);font-weight:600;text-transform:uppercase;">Interactive Approval Gate</div>
          <strong style="display:block;margin:4px 0;">You decide what needs permission</strong>
          <p class="small muted" style="margin:0;">Test how Magnus pauses before sensitive actions. Every action is logged in Activity.</p></div>
        <div class="wiz-sim-box">
          <strong>Ready to place order &amp; confirm delivery</strong>
          <div class="small muted" style="margin-top:4px;">Supplier: <strong>Apex Industrial</strong> · Best quote: <strong>$418.00</strong> (saved $94)</div>
          <div class="wiz-sim-btns">
            <button class="primary" data-wiz-sim="once">Allow once</button>
            <button data-wiz-sim="always">Allow always (&lt; $500)</button>
            <button data-wiz-sim="deny">Decline</button>
          </div>${simMsg}
        </div>
      </div>`;
  }
  const t = WIZARD_TIERS[/** @type {"simple"|"complex"|"long"} */ (Guide.setup.tier)];
  const tierCards = Object.entries(WIZARD_TIERS).map(([k, v]) => `
    <button type="button" class="wiz-card wiz-tier${Guide.setup.tier === k ? " selected" : ""}" data-wiz-tier="${k}">
      <span class="chip ${v.cls}" style="align-self:flex-start;">${esc(v.tag)}</span>
      <strong>${esc(v.title)}</strong>
      <span class="small muted">${esc(v.desc)}</span>
    </button>`).join("");
  return `<h2>Pick a task to see what Magnus can take over</h2>
    <p class="wiz-sub">From daily hygiene to multi-step procurement and long-running operations—select a tier to inspect its plan and launch it.</p>
    <div class="wiz-grid-3">${tierCards}</div>
    <div class="wiz-blueprint">
      <div class="wiz-bp-head">
        <div><div class="small" style="color:var(--accent-text);font-weight:600;">${esc(t.schedule)}</div><strong>${esc(t.title)}</strong></div>
        <div class="wiz-actions"><button type="button" data-wiz-open-disc>Explore more automations</button><button type="button" class="primary" data-wiz-launch="${esc(Guide.setup.tier)}">Launch in Magnus</button></div>
      </div>
      <div class="wiz-bp-steps">${t.steps.map(st => `<div class="wiz-bp-step${st.gate ? " gate" : ""}"><div class="wiz-bp-k">${esc(st.k)}</div><div class="wiz-bp-v">${esc(st.v)}</div></div>`).join("")}</div>
    </div>`;
}

async function openSetupWizard() {
  const d = wizardDialog("setup-wizard-modal", "Get started with Magnus");
  const draw = async () => {
    const f = await guideFacts();
    Guide.facts = f;
    const step = Guide.setup.step;
    const pill = (n, label) => `<button type="button" class="wiz-pill${step === n ? " on" : ""}${step > n ? " done" : ""}" data-wiz-step="${n}"><span class="num">${n}</span>${label}</button>`;
    paint(d, `<div class="wiz-head"><div class="wiz-stepper">${pill(1, "Connect Apps")}${pill(2, "How Magnus Works")}${pill(3, "Pick a Task")}</div><button type="button" class="link" data-wiz-close>Skip for now</button></div>
      <div class="wiz-body">${setupStepHtml(f)}</div>
      <div class="wiz-foot"><span class="small muted">Step ${step} of 3 · Reopen anytime from Settings, Account</span>
        <div class="wiz-actions">${step > 1 ? `<button type="button" data-wiz-step="${step - 1}">Back</button>` : ""}${step < 3 ? `<button type="button" class="primary" data-wiz-step="${step + 1}">Continue</button>` : `<button type="button" class="primary" data-wiz-close>Finish Setup</button>`}</div>
      </div>`);
    bindConnectorActs(d);
  };
  d.onclick = e => {
    const t = /** @type {HTMLElement} */ (e.target);
    if (t.closest("[data-wiz-close]")) { save("guide", { hidden: true }); d.close(); return; }
    const st = t.closest("[data-wiz-step]");
    if (st) { Guide.setup.step = Number(st.dataset.wizStep); draw(); return; }
    const sim = t.closest("[data-wiz-sim]");
    if (sim) { Guide.setup.sim = sim.dataset.wizSim; draw(); return; }
    const tier = t.closest("[data-wiz-tier]");
    if (tier) { Guide.setup.tier = tier.dataset.wizTier; draw(); return; }
    const launch = t.closest("[data-wiz-launch]");
    if (launch) { d.close(); guideStartTurn(WIZARD_TIERS[/** @type {"simple"|"complex"|"long"} */ (launch.dataset.wizLaunch)].prompt); return; }
    if (t.closest("[data-wiz-open-disc]")) { d.close(); openDiscoveryWizard(); }
  };
  await draw();
  if (!d.open) d.showModal();
}

/** Filter discovery templates by selected focus areas and check connected app readiness. @param {any} f */
function discMatchedProposals(f) {
  const conn = (f.connectors || []).filter((/** @type {any} */ c) => c.state === "connected").map((/** @type {any} */ c) => c.name);
  const focus = Guide.disc.focus;
  return Guide.disc.items
    .filter(p => !focus.size || focus.has(p.focus))
    .map(p => {
      const ready = !p.requires.length || p.requires.some(r => conn.some(cn => cn.startsWith(r)));
      return { ...p, ready, active: p.on && ready };
    });
}

/** Start an agent discovery or schedule creation turn immediately (falls back to draft if offline). @param {string} text @param {string[]} [files] */
async function guideStartTurn(text, files = []) {
  save("guide-asked", text);
  if (typeof Server !== "undefined" && Server.chat?.send) {
    try {
      const res = await Server.chat.send({ text, ...(files.length ? { files } : {}) });
      if (res?.thread) { location.hash = `#/chat/${enc(res.thread)}`; return; }
    } catch { /* fall through to pre-filled new chat */ }
  }
  guideAsk(text);
}

function discStepHtml(f) {
  const st = Guide.disc.step, connected = (f.connectors || []).filter((/** @type {any} */ c) => c.state === "connected");
  const matched = discMatchedProposals(f);
  if (st === 1) {
    const focusBtn = (k, title, sub) => `<button type="button" class="wiz-focus${Guide.disc.focus.has(k) ? " on" : ""}" data-disc-focus="${k}"><strong>${title}</strong><span>${sub}</span></button>`;
    return `<h2>Where should Magnus look for work to take off your plate?</h2>
      <p class="wiz-sub">Pick your focus areas and connected apps. Magnus will tailor suggested automations—or run a live scan in a new conversation.</p>
      <div class="wiz-grid-2">
        <div class="wiz-card"><strong>Connected Apps (Read-Only)</strong><span class="small muted">Nothing is modified or sent during discovery.</span>
          ${connected.length ? connected.map((/** @type {any} */ c) => `<div class="spread" style="padding:6px 0;border-bottom:1px solid var(--hair);"><span>${esc(c.title)}</span><span class="chip ok">Connected</span></div>`).join("") : `<p class="small muted">No apps connected yet. Connect one in Apps, or pick a web/research workflow.</p>`}
        </div>
        <div class="wiz-card"><strong>Focus Areas</strong><span class="small muted">Pick where you want to reclaim time, or add a note below.</span>
          <div class="wiz-grid-2" style="margin-bottom:4px;">${focusBtn("inbox", "Inbox & Calendar", "Triage, conflicts, prep")}${focusBtn("vendors", "Purchasing & Vendors", "Research, RFQs, best price")}${focusBtn("followup", "Ongoing Follow-Ups", "Nudge silent threads")}${focusBtn("eng", "Engineering", "PR queues, CI blockers")}</div>
          <textarea data-disc-note placeholder="Anything specific you are tired of doing manually? (Optional)" rows="2">${esc(Guide.disc.note)}</textarea>
        </div>
      </div>`;
  }
  if (st === 2) {
    const readyCount = matched.filter(p => p.ready).length;
    const statusText = connected.length
      ? `${matched.length} suggested template${matched.length === 1 ? "" : "s"} for your focus (${readyCount} ready with your connected apps)`
      : `No apps connected yet · ${matched.length} template${matched.length === 1 ? "" : "s"} matched to your focus (${readyCount} ready without app sign-in)`;
    return `<h2>Suggested automations for your focus</h2>
      <p class="wiz-sub">Matched to your selected areas (${[...Guide.disc.focus].join(", ") || "all"}) and currently connected apps.</p>
      <div class="wiz-trust" style="margin-bottom:14px;"><span>${esc(statusText)}</span><button type="button" data-disc-custom>Ask Magnus to scan live</button></div>
      <div class="wiz-grid-3">
        ${matched.map(p => `<div class="wiz-card"><span class="chip ${p.cls}" style="align-self:flex-start;">${esc(p.badge)}</span><strong>${esc(p.title)}</strong><span class="small muted">${esc(p.desc)}</span>${p.ready ? `<span class="small" style="color:var(--ok);">Ready</span>` : `<span class="small" style="color:var(--warn);">Requires ${esc(p.needsLabel)} connected</span>`}</div>`).join("")}
      </div>`;
  }
  const items = matched.filter(p => Guide.disc.filter === "all" || p.tier === Guide.disc.filter);
  const fBtn = (k, label) => `<button type="button" class="wiz-pill${Guide.disc.filter === k ? " on" : ""}" data-disc-filter="${k}">${label}</button>`;
  return `<h2>Review safety rules &amp; start automations</h2>
    <p class="wiz-sub">Every automation starts with clear guardrails. Starting sends these directly to Magnus to schedule and confirm.</p>
    <div class="wiz-stepper" style="margin-bottom:12px;">${fBtn("all", `Matched (${matched.length})`)}${fBtn("simple", "Simple")}${fBtn("complex", "Multi-Step")}${fBtn("long", "Long-Running")}</div>
    ${items.length ? items.map(p => `<div class="wiz-prop${p.active ? " on" : ""}">
      <div><div style="display:flex;align-items:center;gap:8px;flex-wrap:wrap;margin-bottom:4px;"><span class="chip ${p.cls}">${esc(p.badge)}</span><strong>${esc(p.title)}</strong>${!p.ready ? `<span class="chip warn">Needs ${esc(p.needsLabel)}</span>` : ""}</div>
        <p class="small muted" style="margin:0 0 8px;">${esc(p.desc)}</p>
        <label class="small">Safety mode: <select data-disc-mode="${esc(p.id)}">
          <option value="ask"${p.mode === "ask" ? " selected" : ""}>Ask before external actions (Recommended)</option>
          <option value="draft"${p.mode === "draft" ? " selected" : ""}>Prepare drafts &amp; briefs only</option>
          <option value="auto"${p.mode === "auto" ? " selected" : ""}>Auto-run within threshold</option>
        </select></label></div>
      <button type="button" class="${p.active ? "primary" : ""}" data-disc-toggle="${esc(p.id)}" ${p.ready ? "" : "disabled title=\"Connect required app first\""}>${p.active ? "Selected" : p.ready ? "Select" : "Needs app"}</button>
    </div>`).join("") : `<p class="empty">No templates match this filter. Switch filters or click "Ask Magnus to scan live".</p>`}`;
}

async function openDiscoveryWizard() {
  const d = wizardDialog("discovery-wizard-modal", "Automation Discovery Wizard");
  const draw = async () => {
    const f = Guide.facts || await guideFacts();
    Guide.facts = f;
    const st = Guide.disc.step, count = discMatchedProposals(f).filter(p => p.active).length;
    const pill = (n, label) => `<button type="button" class="wiz-pill${st === n ? " on" : ""}${st > n ? " done" : ""}" data-disc-step="${n}"><span class="num">${n}</span>${label}</button>`;
    paint(d, `<div class="wiz-head"><div class="wiz-stepper">${pill(1, "Scope & Focus")}${pill(2, "Suggested Plans")}${pill(3, "Safety & Start")}</div><button type="button" class="link" data-disc-close>Close</button></div>
      <div class="wiz-body">${discStepHtml(f)}</div>
      <div class="wiz-foot"><span class="small muted">Step ${st} of 3 · All actions logged in Activity</span>
        <div class="wiz-actions">${st > 1 ? `<button type="button" data-disc-step="${st - 1}">Back</button>` : ""}${st < 3 ? `<button type="button" class="primary" data-disc-step="${st + 1}">${st === 1 ? "See Matching Plans" : "Review Safety Rules"}</button>` : `<button type="button" class="primary" data-disc-activate ${count ? "" : "disabled"}>Start Selected (${count})</button>`}</div>
      </div>`);
  };
  d.oninput = e => { const ta = /** @type {HTMLElement} */ (e.target).closest("[data-disc-note]"); if (ta) Guide.disc.note = /** @type {HTMLTextAreaElement} */ (ta).value; };
  d.onchange = e => {
    const sel = /** @type {HTMLSelectElement} */ (e.target).closest("[data-disc-mode]");
    if (!sel) return;
    const item = Guide.disc.items.find(p => p.id === sel.dataset.discMode);
    if (item) item.mode = sel.value;
  };
  d.onclick = async e => {
    const t = /** @type {HTMLElement} */ (e.target);
    if (t.closest("[data-disc-close]")) { d.close(); return; }
    const st = t.closest("[data-disc-step]");
    if (st) { Guide.disc.step = Number(st.dataset.discStep); draw(); return; }
    const foc = t.closest("[data-disc-focus]");
    if (foc) { const k = foc.dataset.discFocus; if (Guide.disc.focus.has(k)) Guide.disc.focus.delete(k); else Guide.disc.focus.add(k); draw(); return; }
    const fil = t.closest("[data-disc-filter]");
    if (fil) { Guide.disc.filter = fil.dataset.discFilter; draw(); return; }
    const tog = t.closest("[data-disc-toggle]");
    if (tog) { const it = Guide.disc.items.find(p => p.id === tog.dataset.discToggle); if (it) it.on = !it.on; draw(); return; }
    if (t.closest("[data-disc-custom]")) { d.close(); await guideStartTurn(`Inspect my connected apps read-only and propose specific automations I can hand off safely. Focus: ${[...Guide.disc.focus].join(", ")}.${Guide.disc.note ? ` Context: ${Guide.disc.note}` : ""}`); return; }
    if (t.closest("[data-disc-activate]")) {
      const f = Guide.facts || await guideFacts();
      const chosen = discMatchedProposals(f).filter(p => p.active);
      d.close();
      const scheds = chosen.filter(p => p.schedule && typeof Server !== "undefined" && Server.schedules?.create);
      const onDemand = chosen.filter(p => !p.schedule || typeof Server === "undefined" || !Server.schedules?.create);
      let created = 0;
      for (const p of scheds) {
        try {
          await Server.schedules.create({ text: `${p.title}: ${p.desc} (Safety: ${p.mode})${Guide.disc.note ? ` Context: ${Guide.disc.note}` : ""}`, ...p.schedule });
          created++;
        } catch { onDemand.push(p); }
      }
      if (created && typeof refresh === "function") refresh().catch(() => {});
      if (created && typeof toast === "function") toast(`Created ${created} schedule${created === 1 ? "" : "s"}.`);
      if (onDemand.length) {
        await guideStartTurn(`Set up and run:\n${onDemand.map(p => `- ${p.title} (${p.badge}) — Safety: ${p.mode}`).join("\n")}${Guide.disc.note ? `\nExtra context: ${Guide.disc.note}` : ""}`);
      } else if (created && typeof openPanel === "function") {
        openPanel("schedules", { history: true, focus: true });
      }
    }
  };
  await draw();
  if (!d.open) d.showModal();
}

route(/^#\/start$/, async () => {
  if (load("guide", {}).hidden) save("guide", { hidden: false });
  await openSetupWizard();
  if (location.hash === "#/start") location.hash = "#/home";
});
