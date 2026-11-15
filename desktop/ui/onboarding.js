// Getting started (decisions/2026-11-15-getting-started.md): one page that
// walks a new person through what makes Magnus useful, in three steps they
// can take in any order. (1) Connect their apps: Google, Microsoft, Slack and
// GitHub, each app with the same one-screen consent and sign-in as the Apps
// page. (2) Set up team chat: start or join a workspace, invite people, and
// choose how the team agent takes part. (3) Ask for a first automated
// workflow in their own words; Magnus proposes the schedule in the
// conversation, and the person confirms it there.
//
// Nothing here is a form of its own and nothing is stored here: every step
// uses the action the rest of the app uses, and whether it is done is read
// from the server each time (connections, workspaces, schedules). The page
// opens by itself once, on a person's first sign-in on this device; after
// that it is in the navigation until every step is done or the person hides
// it, and Settings, Account opens it again.

UI.start = '<path d="M5 21V4"/><path d="M5 4h12l-2.5 4L17 12H5"/>';

/** The four platforms the guide offers, and which connectors are each one's. */
const GUIDE_PLATFORMS = [
  { key: "google", title: "Google", match: (/** @type {string} */ n) => n.startsWith("google-") },
  { key: "microsoft", title: "Microsoft", match: (/** @type {string} */ n) => n.startsWith("microsoft-") },
  { key: "slack", title: "Slack", match: (/** @type {string} */ n) => n === "slack" },
  { key: "github", title: "GitHub", match: (/** @type {string} */ n) => n === "github" },
];
/** How often the navigation's count is read again while the guide is shown. */
const GUIDE_RECOUNT_MS = 15000;

/**
 * @typedef {{ name: string, title: string, state: string, ready?: boolean, for_you?: boolean, account?: ?string, listing?: any }} GuideConnector
 * @typedef {{ id: string, name: string, role: string }} GuideWorkspace
 * @typedef {{ id: string, title: string, next?: string, ended?: string, every_minutes?: number, daily_at?: string }} GuideSchedule
 * @typedef {{ thread: string, title: string, opener?: { app?: string, app_title?: string } }} GuideThread
 * @typedef {{ id: string, name: string, direct?: any, mode: string }} GuideChannel
 * @typedef {{ connectors: GuideConnector[], workspaces: GuideWorkspace[], schedules: GuideSchedule[], threads: GuideThread[], mayStart: boolean, home: ?{ id: string, role: string, agent: { name: string }, channels: GuideChannel[] }, asked: ?string }} GuideFacts
 * @typedef {{ key: string, title: string, apps: GuideConnector[], connected: number }} GuidePlatform
 * @typedef {{ apps: { done: boolean, platforms: GuidePlatform[] }, team: { done: boolean }, workflow: { done: boolean }, done: number }} GuideSteps
 * @typedef {{ busy: ?string, notes: Object<string, { kind: "ok"|"bad", text: string }> }} GuideUi
 */

/** What the guide knows between draws: the last facts read, and what each step last said. */
const Guide = { /** @type {?GuideFacts} */ facts: null, /** @type {?number} */ done: null, counted: 0, /** @type {GuideUi} */ ui: { busy: null, notes: {} } };

/**
 * Which steps are done, from what the server says now.
 * @param {GuideFacts} f
 * @returns {GuideSteps}
 */
function guideSteps(f) {
  const mine = f.connectors.filter(c => c.for_you !== false);
  const platforms = GUIDE_PLATFORMS.map(p => {
    const apps = mine.filter(c => p.match(c.name));
    return { key: p.key, title: p.title, apps, connected: apps.filter(c => c.state === "connected").length };
  });
  const apps = { done: platforms.some(p => p.connected > 0), platforms };
  const team = { done: f.workspaces.length > 0 };
  const workflow = { done: f.schedules.length > 0 };
  return { apps, team, workflow, done: [apps, team, workflow].filter(s => s.done).length };
}

/**
 * First workflows worth asking for, from the apps the person connected:
 * plain sentences they can change before sending.
 * @param {string[]} connected connector names
 */
function guideSuggestions(connected) {
  const has = (/** @type {string} */ n) => connected.includes(n);
  const out = [];
  if (has("google-gmail") || has("microsoft-outlook")) out.push("Every weekday at 8am, catch me up on new email and draft replies to anything that needs one.");
  if (has("google-calendar") || has("microsoft-calendar")) out.push("Every weekday at 7:30am, tell me what is on my calendar today and what I should prepare.");
  if (has("slack")) out.push("Every Friday at 4pm, summarise what happened in my Slack channels this week.");
  if (has("github")) out.push("Every weekday at 9am, list the pull requests waiting for my review.");
  if (has("google-drive") || has("microsoft-onedrive")) out.push("Every Monday at 9am, list the documents people shared with me last week.");
  out.push("Every Monday at 9am, send me a short plan for my week.");
  return out.slice(0, 4);
}

/** A step's last result, said where the step is. @param {GuideUi} ui @param {string} key */
function guideNote(ui, key) {
  const n = ui.notes[key];
  if (!n) return "";
  return `<p class="guide-note ${n.kind}" ${n.kind === "bad" ? 'role="alert"' : 'role="status"'} data-guide-note="${esc(key)}">${esc(n.text)}</p>`;
}

/** One app in a platform: where it stands, and the one thing to do next. @param {GuideConnector} c @param {GuideFacts} f */
function guideAppRow(c, f) {
  const who = c.listing?.publisher || c.title;
  const opened = f.threads.find(t => t.opener?.app === c.name);
  let line, act = "";
  switch (stateOf(c)) {
    case "connected":
      line = `<span class="chip ok">Connected</span> ${c.account ? `as ${esc(c.account)}` : ""}${opened ? ` · <a href="#/chat/${enc(opened.thread)}">See what Magnus could take over</a>` : ""}`;
      break;
    case "connecting":
      line = `<span class="spin" aria-hidden="true"></span> Finish signing in on ${esc(who)}'s page, then come back here.`;
      act = `<button class="link" data-act="connect-page" data-url="${esc(Connecting[c.name] || "")}">Open it again</button>`;
      break;
    case "needs_attention":
      line = `<span class="chip warn">Sign in again</span> Your sign-in stopped working.`;
      act = `<button data-act="connect" data-app="${esc(c.name)}">Sign in again</button>`;
      break;
    default:
      if (c.ready === false) { line = "Not set up on this server yet. Ask your organisation's owner."; break; }
      line = `Sign in on ${esc(who)}'s own page. Magnus never sees your password.`;
      act = `<button class="primary" data-act="connect" data-app="${esc(c.name)}" aria-label="Connect ${esc(c.title)}">Connect</button>`;
  }
  return `<li class="guide-app" data-app="${esc(c.name)}" data-state="${esc(stateOf(c))}">${tile(c)}<div class="guide-app-text"><strong>${esc(c.title)}</strong><span class="small muted">${line}</span></div>${act}</li>`;
}

/** Step 1: the four platforms, each with its apps. @param {GuideSteps} s @param {GuideFacts} f @param {GuideUi} ui */
function guideAppsStep(s, f, ui) {
  const platforms = s.apps.platforms.map(p => `<section class="guide-platform" data-platform="${p.key}" aria-label="${esc(p.title)}">${p.apps.length === 1 && p.apps[0].title === p.title ? "" : `<h3>${esc(p.title)}${p.connected ? ` <span class="small muted">· ${p.connected} connected</span>` : ""}</h3>`}
    ${p.apps.length ? `<ul class="guide-apps">${p.apps.map(c => guideAppRow(c, f)).join("")}</ul>` : `<p class="small muted">${esc(p.title)} is not offered on this server.</p>`}</section>`).join("");
  return `<p class="small muted">Connect the apps you work in, and Magnus can read, draft and act in them when your requests need it. Each one shows what Magnus can do there before you sign in. One is enough to start.</p>
    <div class="guide-platforms">${platforms}</div>${guideNote(ui, "apps")}<p class="small"><a href="#/library">Every app Magnus can work in</a></p>`;
}

/** Step 2: start or join a workspace, then invite people and set how the team agent joins in. @param {GuideFacts} f @param {GuideUi} ui */
function guideTeamStep(f, ui) {
  const busy = ui.busy === "team";
  if (!f.workspaces.length) {
    const form = f.mayStart
      ? `<p class="small muted">A workspace is where your team chats, shares files and works with Magnus. Magnus is in every channel from the start.</p>
        <form class="guide-form" data-guide-form="team"><label class="sr" for="guide-team">Workspace name</label><input id="guide-team" data-draft="guide-team" placeholder="Your team's name, like Acme Design" autocomplete="off" ${busy ? "disabled" : ""}><button class="primary" ${busy ? "disabled" : ""}>${busy ? "Creating…" : "Create workspace"}</button></form>`
      : `<p class="small muted">A Team Admin starts your team's workspace and sends you a link. Paste it here to join.</p>
        <form class="guide-form" data-guide-form="join"><label class="sr" for="guide-join">Invite link</label><input id="guide-join" data-draft="guide-join" placeholder="Paste your invite link" autocomplete="off" ${busy ? "disabled" : ""}><button class="primary" ${busy ? "disabled" : ""}>${busy ? "Joining…" : "Join"}</button></form>`;
    return `${form}${guideNote(ui, "team")}`;
  }
  const w = f.workspaces[0], h = f.home && f.home.id === w.id ? f.home : null;
  const more = f.workspaces.length > 1 ? ` and ${f.workspaces.length - 1} more` : "";
  let extra = "";
  if (h && ["admin", "owner"].includes(h.role)) {
    const general = h.channels.find(c => !c.direct);
    const helpful = general?.mode === "when_helpful";
    extra = `<h3>Invite your team</h3><p class="small muted">By email, or copy a link to send them.</p>${inviteForm()}
      ${general ? `<h3>How ${esc(h.agent.name)} takes part in #${esc(general.name)}</h3><label class="sr" for="guide-mode">How ${esc(h.agent.name)} takes part</label><select id="guide-mode" data-guide-mode data-ws="${esc(h.id)}" data-channel="${esc(general.id)}"><option value="mentions"${helpful ? "" : " selected"}>Replies when someone mentions it</option><option value="when_helpful"${helpful ? " selected" : ""}>Joins in when it can help</option></select>
      <p class="small faint">Each channel has its own setting. You can change it at the top of any channel.</p>` : ""}`;
  }
  return `<p class="guide-done-line">You are in <a href="#/team/${enc(w.id)}">${esc(w.name)}</a>${more}. Mention ${esc(h?.agent.name || "Magnus")} in any channel to ask it for something.</p>${extra}${guideNote(ui, "team")}`;
}

/** Step 3: a first workflow, asked for in the person's own words. @param {GuideSteps} s @param {GuideFacts} f @param {GuideUi} ui */
function guideWorkflowStep(s, f, ui) {
  if (f.schedules.length) {
    const sc = f.schedules[0];
    return `<p class="guide-done-line">Scheduled: <a href="#/schedules" data-open="schedule:${esc(sc.id)}">${esc(sc.title)}</a>${rule(sc) ? ` · ${esc(rule(sc))}` : ""}${sc.next && !sc.ended ? ` · next run ${esc(until(sc.next))}` : ""}.</p>
      <p class="small muted">Magnus runs it on its own and posts the results in its conversation. Ask for more any time.</p>`;
  }
  const connected = s.apps.platforms.flatMap(p => p.apps.filter(c => c.state === "connected").map(c => c.name));
  const asked = f.asked ? `<p class="guide-note ok" role="status">You asked Magnus: “${esc(f.asked)}”. When it proposes the schedule in your conversation, confirm it there and this step is done.</p>` : "";
  return `<p class="small muted">Say what you want done and when, in your own words. Magnus proposes the schedule, and you confirm it. Pick one to start from, or write your own.</p>
    ${asked}<ul class="guide-suggestions">${guideSuggestions(connected).map(t => `<li><button data-guide-suggest="${esc(t)}">${esc(t)}</button></li>`).join("")}</ul>
    <form class="guide-form" data-guide-form="ask"><label class="sr" for="guide-ask">What should Magnus do, and when?</label><input id="guide-ask" data-draft="guide-ask" placeholder="Every … , please …" autocomplete="off"><button>Ask Magnus</button></form>${guideNote(ui, "workflow")}`;
}

/**
 * The whole page.
 * @param {GuideFacts} f @param {GuideUi} ui
 */
function guideHtml(f, ui) {
  const s = guideSteps(f), total = 3;
  const next = !s.apps.done ? "apps" : !s.team.done ? "team" : !s.workflow.done ? "workflow" : "";
  const step = (/** @type {string} */ key, /** @type {number} */ n, /** @type {string} */ title, /** @type {boolean} */ done, /** @type {string} */ body) =>
    `<li class="guide-step${done ? " done" : ""}${key === next ? " next" : ""}" data-step="${key}"><span class="guide-mark" aria-hidden="true">${done ? icon("done") : n}</span>
      <div class="guide-body"><h2>${esc(title)} <span class="sr">${done ? "(done)" : "(to do)"}</span></h2>${body}</div></li>`;
  const all = s.done === total;
  return `<div class="guide" data-guide>
    <header class="guide-head"><h1>${all ? "You are set up" : "Get started with Magnus"}</h1>
      <p class="muted">${all ? "Magnus can work in your apps, your team can reach it in chat, and your first workflow runs on its own." : "Three steps, in any order. Each takes a minute, and you can come back to any of them."}</p>
      <div class="guide-progress" role="progressbar" aria-label="Setup progress" aria-valuemin="0" aria-valuemax="${total}" aria-valuenow="${s.done}"><span style="width:${Math.round(s.done / total * 100)}%"></span></div>
      <p class="small" data-guide-count aria-live="polite">${s.done} of ${total} done</p></header>
    <ol class="guide-steps">
      ${step("apps", 1, "Connect your apps", s.apps.done, guideAppsStep(s, f, ui))}
      ${step("team", 2, "Set up team chat", s.team.done, guideTeamStep(f, ui))}
      ${step("workflow", 3, "Schedule your first workflow", s.workflow.done, guideWorkflowStep(s, f, ui))}
    </ol>
    <footer class="guide-foot">${all ? `<button class="primary" data-guide="finish">Start working</button>` : `<button class="link" data-guide="hide">Hide this guide</button><span class="small faint">You can open it again from Settings, Account.</span>`}</footer></div>`;
}

/** The code in an invite link, a #/join link, or the code itself. @param {string} s */
function guideInviteCode(s) {
  const t = s.trim();
  try { const u = new URL(t); const c = u.searchParams.get("code"); if (c) return c; const m = u.hash.match(/^#\/join\/([^/?]+)/); if (m) return decodeURIComponent(m[1]); } catch { /* not a link */ }
  const m = t.match(/(?:^|[?&]code=|#\/join\/)([A-Za-z0-9_-]+)$/);
  return m ? m[1] : "";
}

/** Read what the guide shows: connections, workspaces, schedules and the person's first workspace. */
async function guideFacts() {
  const [connectors, workspaces, mayStart] = await Promise.all([
    Resources.read("Apps", "/v1/connectors", "connectors", true).then(c => c || []),
    Server.workspaces.list().catch(() => []),
    mayStartWorkspace().catch(() => false),
  ]);
  const home = workspaces.length && ["admin", "owner"].includes(workspaces[0].role) ? await Server.workspaces.home(workspaces[0].id).catch(() => null) : null;
  return { connectors, workspaces, schedules: App.schedules || [], threads: App.threads || [], mayStart, home, asked: load("guide-asked", null) };
}

/** Whether the guide is still offered in the navigation. */
const guideShown = () => !load("guide", {}).hidden && Guide.done !== 3;

/** Count the steps done, for the navigation, now and then while the guide is shown. */
async function guideRecount(force = false) {
  if (!App.me || !guideShown() || (!force && Date.now() - Guide.counted < GUIDE_RECOUNT_MS)) return;
  Guide.counted = Date.now();
  const f = await guideFacts().catch(() => null);
  if (!f) return;
  const before = Guide.done;
  Guide.facts = f; Guide.done = guideSteps(f).done;
  if (before !== Guide.done) sidebar();
}

/** The navigation's entry, while there is something left to do. @param {string} route */
function guideNavItem(route) {
  if (!guideShown()) return "";
  guideRecount().catch(() => {});
  const on = route === "#/start", n = Guide.done;
  return `<a class="primary-nav${on ? " on" : ""}" href="#/start" data-guide-nav ${on ? 'aria-current="page"' : ""} aria-label="Get started${n === null ? "" : `, ${n} of 3 done`}" title="Get started">${icon("start")}<span class="nav-label">Get started</span>${n === null ? "" : `<span class="pill quiet" aria-hidden="true">${n}/3</span>`}</a>`;
}

/** Whether the person is new here: no task, and no conversation but the ones Magnus opened. */
const guideNewcomer = () => !(App.tasks || []).length && (App.threads || []).every(t => t.opener);

/** At sign-in: a newcomer's first time on this device opens the guide instead of home. */
async function guideFirstRun() {
  await guideRecount(true);
  if (load("guide-opened", false) || !guideShown() || !guideNewcomer()) return;
  save("guide-opened", true);
  if (!location.hash || location.hash === "#/home") location.hash = "#/start";
}

/** Say a step's result where the step is, and draw again. @param {string} key @param {"ok"|"bad"} kind @param {string} text */
function guideSay(key, kind, text) { Guide.ui.notes[key] = { kind, text }; }

/** What each control on the page does. @param {HTMLElement} main @param {() => Promise<void>} draw */
function guideWire(main, draw) {
  bindConnectorActs(main);
  main.addEventListener("submit", async e => {
    const form = /** @type {HTMLElement} */ (e.target).closest("[data-guide-form]");
    if (!form) return;
    e.preventDefault();
    const input = /** @type {HTMLInputElement} */ ($("input", form)), text = input.value.trim(), which = form.dataset.guideForm;
    if (!text) { input.focus(); return; }
    if (which === "ask") return guideAsk(text);
    Guide.ui.busy = "team"; await draw();
    try {
      if (which === "team") {
        const w = await Server.workspaces.create(text);
        guideSay("team", "ok", `Created ${w.name || text}. Magnus is already in every channel. Invite your team next.`);
      } else {
        const code = guideInviteCode(text);
        if (!code) throw new Error("That does not look like an invite link. Copy the whole link from the invitation and paste it here.");
        await Server.workspaces.join(code);
        guideSay("team", "ok", "You joined. Your team's channels are under Teams.");
      }
      $$("[data-guide-form] input", main).forEach(i => { /** @type {HTMLInputElement} */ (i).value = ""; }); delete drafts[which === "team" ? "guide-team" : "guide-join"];
    } catch (err) { guideSay("team", "bad", err.message); }
    Guide.ui.busy = null; await draw(); guideRecount(true).catch(() => {});
  });
  main.addEventListener("change", async e => {
    const sel = /** @type {HTMLSelectElement} */ (e.target).closest("[data-guide-mode]");
    if (!sel) return;
    const name = Guide.facts?.home?.agent.name || "Magnus";
    try {
      await Server.channels.act(sel.dataset.ws, sel.dataset.channel, "agent", { mode: sel.value });
      guideSay("team", "ok", sel.value === "when_helpful" ? `Saved. ${name} joins in when it can help: catching people up, following up on quiet threads.` : `Saved. ${name} replies when someone mentions it.`);
    } catch (err) { guideSay("team", "bad", err.message); }
    await draw();
  });
  main.addEventListener("click", e => {
    const t = /** @type {HTMLElement} */ (e.target);
    const pick = t.closest("[data-guide-suggest]");
    if (pick) { e.preventDefault(); return guideAsk(pick.dataset.guideSuggest); }
    const g = t.closest("[data-guide]");
    if (g?.dataset.guide === "hide") { e.preventDefault(); save("guide", { hidden: true }); toast("Guide hidden. Settings, Account opens it again."); location.hash = "#/home"; }
    if (g?.dataset.guide === "finish") { e.preventDefault(); save("guide", { hidden: true }); location.hash = "#/home"; }
  });
}

/** A first workflow: the sentence goes to a new conversation for the person to send. @param {string} text */
function guideAsk(text) {
  save("guide-asked", text);
  delete drafts["guide-ask"];
  App.appSearch = text;
  location.hash = "#/chat/new";
}

route(/^#\/start$/, async main => {
  // Opening it asks for it again, after it was hidden.
  if (load("guide", {}).hidden) save("guide", { hidden: false });
  Guide.ui.busy = null;
  const draw = async () => {
    const f = await guideFacts();
    if (!main.isConnected) return;
    Guide.facts = f; Guide.done = guideSteps(f).done; Guide.counted = Date.now();
    paint(main, guideHtml(f, Guide.ui));
    if (f.workspaces.length) bindInvite(main, f.workspaces[0].id);
  };
  guideWire(main, draw);
  await draw();
  App.redraw = draw;
  onLeave(() => { if (App.redraw === draw) App.redraw = null; Guide.ui.notes = {}; });
});
