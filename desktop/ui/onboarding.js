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
  { id: "d-inbox", tier: "simple", badge: "Simple · Weekdays 7:45 AM", cls: "ok", title: "Inbox Triage & Schedule Conflict Resolver", desc: "Checks overnight email for meeting requests, resolves calendar overlaps with 2 open options, and prepares reply drafts.", mode: "ask", on: true },
  { id: "d-procure", tier: "complex", badge: "Multi-Step · On Demand", cls: "warn", title: "Product Research, Supplier Quotes & Order Placement", desc: "Researches specs & pricing, emails verified suppliers for quotes, builds a comparison table, and pauses for approval before ordering.", mode: "ask", on: true },
  { id: "d-followup", tier: "long", badge: "Long-Running · Every 4h (14 days)", cls: "sch", title: "Continuous Vendor & Deliverable Follow-Up", desc: "Watches open threads for stalled replies (>48h), drafts polite follow-ups, updates your live status brief, and escalates blockers.", mode: "draft", on: true },
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
    if (launch) { d.close(); guideAsk(WIZARD_TIERS[/** @type {"simple"|"complex"|"long"} */ (launch.dataset.wizLaunch)].prompt); return; }
    if (t.closest("[data-wiz-open-disc]")) { d.close(); openDiscoveryWizard(); }
  };
  await draw();
  if (!d.open) d.showModal();
}

function discStepHtml(f) {
  const st = Guide.disc.step, connected = (f.connectors || []).filter((/** @type {any} */ c) => c.state === "connected");
  if (st === 1) {
    const focusBtn = (k, title, sub) => `<button type="button" class="wiz-focus${Guide.disc.focus.has(k) ? " on" : ""}" data-disc-focus="${k}"><strong>${title}</strong><span>${sub}</span></button>`;
    return `<h2>Where should Magnus look for work to take off your plate?</h2>
      <p class="wiz-sub">Magnus inspects recent patterns read-only to find repetitive tasks, multi-step workflows, and long-running follow-ups you can automate safely.</p>
      <div class="wiz-grid-2">
        <div class="wiz-card"><strong>Connected Apps (Read-Only)</strong><span class="small muted">Nothing is modified or sent during discovery.</span>
          ${connected.length ? connected.map((/** @type {any} */ c) => `<div class="spread" style="padding:6px 0;border-bottom:1px solid var(--hair);"><span>${esc(c.title)}</span><span class="chip ok">Connected</span></div>`).join("") : `<p class="small muted">No apps connected yet—Magnus can still propose general workflows.</p>`}
        </div>
        <div class="wiz-card"><strong>Focus Areas</strong><span class="small muted">Pick where you want to reclaim time, or add a note below.</span>
          <div class="wiz-grid-2" style="margin-bottom:4px;">${focusBtn("inbox", "Inbox & Calendar", "Triage, conflicts, prep")}${focusBtn("vendors", "Purchasing & Vendors", "Research, RFQs, best price")}${focusBtn("followup", "Ongoing Follow-Ups", "Nudge silent threads")}${focusBtn("eng", "Engineering", "PR queues, CI blockers")}</div>
          <textarea data-disc-note placeholder="Anything specific you are tired of doing manually? (Optional)" rows="2">${esc(Guide.disc.note)}</textarea>
        </div>
      </div>`;
  }
  if (st === 2) {
    return `<h2>Here is what Magnus found you can automate</h2>
      <p class="wiz-sub">Opportunities grouped from quick daily hygiene to multi-week autonomous tracking.</p>
      <div class="wiz-trust" style="margin-bottom:14px;"><span><strong>Discovery scan ready</strong> · Read-only pattern check across your workspace</span><span class="chip ok">3 high-leverage workflows</span></div>
      <div class="wiz-grid-3">
        <div class="wiz-card"><span class="chip ok" style="align-self:flex-start;">Simple · Daily</span><strong>Email &amp; calendar conflicts</strong><span class="small muted">Spot scheduling emails &amp; overlapping invites every weekday morning.</span></div>
        <div class="wiz-card"><span class="chip warn" style="align-self:flex-start;">Multi-Step · Execution</span><strong>Research, quotes &amp; ordering</strong><span class="small muted">Compare suppliers, request quotes, and pause for your 1-click order OK.</span></div>
        <div class="wiz-card"><span class="chip sch" style="align-self:flex-start;">Long-Running · Continuous</span><strong>Stalled thread &amp; vendor tracker</strong><span class="small muted">Watch open threads for weeks, draft 48h nudges, and escalate blockers.</span></div>
      </div>
      <div class="wiz-trust"><span>Want Magnus to design a custom automation from scratch?</span><button type="button" data-disc-custom>Explore custom task with Magnus</button></div>`;
  }
  const items = Guide.disc.items.filter(p => Guide.disc.filter === "all" || p.tier === Guide.disc.filter);
  const fBtn = (k, label) => `<button type="button" class="wiz-pill${Guide.disc.filter === k ? " on" : ""}" data-disc-filter="${k}">${label}</button>`;
  return `<h2>Review safety rules &amp; activate automations</h2>
    <p class="wiz-sub">Every automation starts with clear guardrails. Choose which tasks to hand over and how much autonomy each gets.</p>
    <div class="wiz-stepper" style="margin-bottom:12px;">${fBtn("all", "All (3)")}${fBtn("simple", "Simple")}${fBtn("complex", "Multi-Step")}${fBtn("long", "Long-Running")}</div>
    ${items.map(p => `<div class="wiz-prop${p.on ? " on" : ""}">
      <div><div style="display:flex;align-items:center;gap:8px;flex-wrap:wrap;margin-bottom:4px;"><span class="chip ${p.cls}">${esc(p.badge)}</span><strong>${esc(p.title)}</strong></div>
        <p class="small muted" style="margin:0 0 8px;">${esc(p.desc)}</p>
        <label class="small">Safety mode: <select data-disc-mode="${esc(p.id)}">
          <option value="ask"${p.mode === "ask" ? " selected" : ""}>Ask before external actions (Recommended)</option>
          <option value="draft"${p.mode === "draft" ? " selected" : ""}>Prepare drafts &amp; briefs only</option>
          <option value="auto"${p.mode === "auto" ? " selected" : ""}>Auto-run within threshold</option>
        </select></label></div>
      <button type="button" class="${p.on ? "primary" : ""}" data-disc-toggle="${esc(p.id)}">${p.on ? "Selected" : "Select"}</button>
    </div>`).join("")}`;
}

async function openDiscoveryWizard() {
  const d = wizardDialog("discovery-wizard-modal", "Automation Discovery Wizard");
  const draw = async () => {
    const f = Guide.facts || await guideFacts();
    Guide.facts = f;
    const st = Guide.disc.step, count = Guide.disc.items.filter(p => p.on).length;
    const pill = (n, label) => `<button type="button" class="wiz-pill${st === n ? " on" : ""}${st > n ? " done" : ""}" data-disc-step="${n}"><span class="num">${n}</span>${label}</button>`;
    paint(d, `<div class="wiz-head"><div class="wiz-stepper">${pill(1, "Scope & Focus")}${pill(2, "What Magnus Found")}${pill(3, "Safety & Activate")}</div><button type="button" class="link" data-disc-close>Close</button></div>
      <div class="wiz-body">${discStepHtml(f)}</div>
      <div class="wiz-foot"><span class="small muted">Step ${st} of 3 · All actions logged in Activity</span>
        <div class="wiz-actions">${st > 1 ? `<button type="button" data-disc-step="${st - 1}">Back</button>` : ""}${st < 3 ? `<button type="button" class="primary" data-disc-step="${st + 1}">${st === 1 ? "Scan for Automations" : "Review Safety Rules"}</button>` : `<button type="button" class="primary" data-disc-activate ${count ? "" : "disabled"}>Activate Selected (${count})</button>`}</div>
      </div>`);
  };
  d.oninput = e => { const ta = /** @type {HTMLElement} */ (e.target).closest("[data-disc-note]"); if (ta) Guide.disc.note = /** @type {HTMLTextAreaElement} */ (ta).value; };
  d.onchange = e => {
    const sel = /** @type {HTMLSelectElement} */ (e.target).closest("[data-disc-mode]");
    if (!sel) return;
    const item = Guide.disc.items.find(p => p.id === sel.dataset.discMode);
    if (item) item.mode = sel.value;
  };
  d.onclick = e => {
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
    if (t.closest("[data-disc-custom]")) { d.close(); guideAsk(`Help me discover what repetitive work I can automate safely. Focus: ${[...Guide.disc.focus].join(", ")}.${Guide.disc.note ? ` Context: ${Guide.disc.note}` : ""}`); return; }
    if (t.closest("[data-disc-activate]")) {
      const chosen = Guide.disc.items.filter(p => p.on);
      d.close();
      guideAsk(`Set up these automations with their safety modes:\n${chosen.map(p => `- ${p.title} (${p.badge}) — Safety: ${p.mode}`).join("\n")}${Guide.disc.note ? `\nExtra context: ${Guide.disc.note}` : ""}`);
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
