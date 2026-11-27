// App boot, lifecycle, background polling schedule, and update checks (extracted from app.js).
"use strict";

// ---------- start ----------

let background = [];
// ---------- the app's own updates ----------

// `look`: ask the appliance now, as when a session begins (the app's hourly
// look may have run before it was connected).
async function refreshUpdate(look = false) {
  try { App.update = await (look ? Magnus.updateLook() : Magnus.updateState()); } catch { App.update = await Magnus.updateState().catch(() => null); }
  if ($("#side")) sidebar();
  await App.redrawUpdates?.();
}

let restartingUpdate = false;
async function restartToUpdate() {
  if (restartingUpdate) return;
  restartingUpdate = true;
  const btns = $$("[data-act=restart-update]");
  // The rail shows the button as its icon: only its words change.
  const say = (/** @type {HTMLElement} */ b, t) => { const l = b.querySelector(".update-label"); if (l) l.textContent = t; else b.textContent = t; };
  for (const b of btns) { b.disabled = true; say(b, "Restarting…"); }
  toast("Installing the update; Magnus restarts in a moment.", 10000);
  try {
    await Magnus.updateInstall();
  } catch (e) {
    restartingUpdate = false;
    for (const b of btns) { b.disabled = false; say(b, "Restart to update"); }
    toast(`The update did not install: ${e.message}`);
    refreshUpdate();
  }
}

// What changed, in the release's own plain words, once after an update.
async function showWhatsNew() {
  let n = null;
  try { n = await Magnus.whatsNew(); } catch { return; }
  if (!n) return;
  const box = document.createElement("div");
  box.className = "whats-new card"; box.setAttribute("role", "dialog"); box.setAttribute("aria-label", "What's new");
  box.innerHTML = `<h3>What's new in Magnus ${esc(n.version)}</h3>
    <div class="notes">${releaseNotesHtml(n.notes)}</div>
    <div class="row"><button class="primary" data-act="close">Got it</button></div>`;
  box.querySelector("[data-act=close]").onclick = () => box.remove();
  document.body.append(box);
}

// The appliance no longer accepts this version of the app: only updating helps.
let updateScreen = null;
async function updateRequiredScreen(info) {
  if (updateScreen) return;
  stop();
  document.body.innerHTML = `<div class="signin"><h1>Magnus</h1><h3>Update required</h3>
    <p class="muted">${esc(info?.error || "The server needs a newer version of this app.")}</p><div class="card" id="ur"></div></div>`;
  const draw = async () => {
    let st = null;
    try { st = await Magnus.updateState(); } catch { /* shown below */ }
    const box = $("#ur");
    if (!box) return;
    if (!st) paint(box, `<p>Open the server's address in a browser to use it until the app is updated.</p>`);
    else if (st.ready) paint(box, `<p>Magnus ${esc(st.ready.version)} is downloaded and checked.</p><div class="row"><button class="primary" data-act="restart-update">Restart to update</button></div>`);
    else if (st.can_update) paint(box, `<p>${esc(st.can_update)}</p>`);
    else paint(box, `<p>${st.error ? `The update could not be fetched: ${esc(st.error)}` : "Downloading the update…"}</p><div class="row"><button data-act="look">Try again</button></div>`);
  };
  document.body.addEventListener("click", async e => {
    if (e.target.closest("[data-act=restart-update]")) restartToUpdate();
    if (e.target.closest("[data-act=look]")) { try { await Magnus.updateLook(); } catch { /* drawn */ } draw(); }
  });
  Magnus.onUpdateReady(draw);
  updateScreen = setInterval(draw, 5000);
  draw();
}
window.addEventListener("magnus-update-required", e => updateRequiredScreen(e.detail));

async function boot() {
  await loadUiCatalog();
  App.sessionGeneration++;
  Resources.reset();
  App.me = await Server.me();
  // The Terms of Use first, then the usage-data question (agreements.js).
  if (!(await agreementsGate())) return endSession();
  Theme.scope(App.appliance, App.me.id);
  AuroraBoxes.scope(App.appliance, App.me.id);
  MessageDrafts.open(App.appliance, App.me.id);
  const host = await Magnus.systemLook().catch(() => ({}));
  const canRead = () => { App.canReadAloud = !!App.me?.read_aloud || !!host.native_speech || ("speechSynthesis" in window && speechSynthesis.getVoices().length > 0); };
  canRead();
  if ("speechSynthesis" in window) speechSynthesis.addEventListener("voiceschanged", () => { canRead(); App.redraw?.(); });
  shell();
  adminStart();
  window.onhashchange = go;
  Magnus.onUpdateReady(() => refreshUpdate());
  refreshUpdate(true);
  showWhatsNew();
  App.prefs = await Server.preferences.get().catch(() => null);
  if (typeof autoDetectTimezone === "function") autoDetectTimezone();
  await Promise.all([refresh(), pollVersions()]);
  // Back from an app's sign-in in this browser: it is finished here.
  ConnectWeb.finish(connectorsChanged).catch(() => {});
  // Opening the app shows what changed since the last session: with
  // news, and landing on home, the needs panel opens beside it (#367).
  App.reported = new Set();
  App.reporting = new Set();
  App.feed = await Server.homeOpened().catch(() => null);
  App.feedDismissed = false;
  // Return-session details stay in Activity; informational arrivals do
  // not switch the selected region or hide the conversation's composer.
  banners();
  await Notices.poll();
  lastNoticesPollMs = Date.now();
  const pollRefreshTick = () => {
    if (document.hidden && Date.now() - lastRefreshMs < REFRESH_HIDDEN_MS) return;
    refresh();
  };
  const pollVersionsTick = () => {
    if (document.hidden) return;
    pollVersions();
  };
  const pollNoticesTick = () => {
    if (document.hidden && Date.now() - lastNoticesPollMs < NOTICES_HIDDEN_MS) return;
    lastNoticesPollMs = Date.now();
    Notices.poll();
  };
  if (visibilityHandler) document.removeEventListener("visibilitychange", visibilityHandler);
  visibilityHandler = () => {
    if (!document.hidden && App.me) {
      refresh();
      lastNoticesPollMs = Date.now();
      Notices.poll();
    }
  };
  document.addEventListener("visibilitychange", visibilityHandler);
  background = [
    setInterval(pollRefreshTick, REFRESH_VISIBLE_MS),
    setInterval(pollVersionsTick, VERSION_POLL_MS),
    setInterval(pollNoticesTick, NOTICES_VISIBLE_MS),
  ];
  await guideFirstRun().catch(() => {});
  if (!document._wizardBound) {
    document._wizardBound = true;
    document.addEventListener("click", async e => {
      const t = /** @type {HTMLElement} */ (e.target);
      if (t.closest("[data-open-discovery]")) { e.preventDefault(); openDiscoveryWizard().catch(() => {}); }
      else if (t.closest("[data-open-setup]")) { e.preventDefault(); openSetupWizard().catch(() => {}); }
      else if (t.closest("[data-toggle-notices]")) {
        e.preventDefault();
        const pop = $("#notice-center");
        const btn = t.closest("[data-toggle-notices]");
        if (pop) {
          const open = pop.hidden;
          if (open && typeof Notices !== "undefined" && Notices.centerHtml) paint(pop, Notices.centerHtml());
          pop.hidden = !open;
          btn?.setAttribute("aria-expanded", String(open));
        }
      }
      else if (t.closest("[data-dnd-toggle]")) {
        e.preventDefault();
        if (typeof Notices !== "undefined" && Notices.snooze) {
          const next = Notices.isDnd() ? 0 : 60;
          Notices.snooze(next);
          toast(next ? "Notifications snoozed for 1 hour." : "Notifications resumed.");
          const pop = $("#notice-center");
          if (pop && !pop.hidden) paint(pop, Notices.centerHtml());
          convHead(App.thread);
        }
      }
      else if (t.closest("[data-notices-clear]")) {
        e.preventDefault();
        if (typeof Notices !== "undefined" && Notices.clearHistory) {
          Notices.clearHistory();
          const pop = $("#notice-center");
          if (pop && !pop.hidden) paint(pop, Notices.centerHtml());
          convHead(App.thread);
        }
      }
      else if (t.closest("[data-notice-item]")) {
        const pop = $("#notice-center");
        if (pop) pop.hidden = true;
        $("[data-toggle-notices]")?.setAttribute("aria-expanded", "false");
      }
      else if (t.closest("[data-toggle-inspector]")) {
        e.preventDefault();
        if (Panel.what) closePanel(true);
        else openPanel(needsMe().length ? "needs" : App.tasks.some(x => !ENDED.includes(x.status)) ? "tasks" : "activity", { history: true, focus: true });
        convHead(App.thread);
      }
      else if (t.closest("[data-panel-back]")) {
        e.preventDefault();
        openPanel(/** @type {HTMLElement} */ (t.closest("[data-panel-back]")).dataset.panelBack || "activity", { history: true, focus: true });
      }
      else if (t.closest("[data-pause-computer]")) {
        const b = /** @type {HTMLButtonElement} */ (t.closest("[data-pause-computer]"));
        b.disabled = true;
        try {
          await Server.computers.pause(b.dataset.pauseComputer);
          toast("Computer paused and accesses taken back.");
          await refresh();
        } catch (err) { toast(err.message); b.disabled = false; }
      }
      else if (t.closest("[data-wake-computer]")) {
        const b = /** @type {HTMLButtonElement} */ (t.closest("[data-wake-computer]"));
        const id = b.dataset.wakeComputer;
        b.disabled = true;
        try {
          await Server.computers.wake(id);
          openPanel(`computer:${id}`, { history: true, focus: true });
          await refresh();
        } catch (err) { toast(err.message); b.disabled = false; }
      }
      else if (t.closest("[data-overview-kill]")) {
        const act = /** @type {HTMLElement} */ (t.closest("[data-overview-kill]")).dataset.overviewKill;
        const sel = `[data-kill-${act === "release" ? "release" : "press"}="server"]`;
        const btn = $(`[data-admin-kill] ${sel}`) || $(`[data-admin-kill-top] ${sel}`);
        if (btn) btn.click();
      }
      else if (t.closest("[data-stop-subagent]")) {
        const b = /** @type {HTMLButtonElement} */ (t.closest("[data-stop-subagent]"));
        const id = b.dataset.stopSubagent;
        if (!id) return;
        b.disabled = true;
        try {
          await Server.tasks.cancel(id);
          toast("Sub-agent stopped.");
          await refresh();
        } catch (err) { toast(err.message); b.disabled = false; }
      }
      else if (t.closest("[data-stop-all-work]")) {
        const b = /** @type {HTMLButtonElement} */ (t.closest("[data-stop-all-work]"));
        if (!await ask("Stop all of your running tasks, pause your schedules, and pause your active computers?", "Stop my active work", true)) return;
        b.disabled = true;
        try {
          const r = await Server.tasks.stopAll();
          toast(`Stopped ${r.tasks_cancelled || 0} task(s), paused ${r.schedules_paused || 0} schedule(s) and ${r.computers_paused || 0} computer(s).`);
          await refresh();
        } catch (err) { toast(err.message); b.disabled = false; }
      }
      else {
        const pop = $("#notice-center");
        if (pop && !pop.hidden && !t.closest("#notice-center")) {
          pop.hidden = true;
          $("[data-toggle-notices]")?.setAttribute("aria-expanded", "false");
        }
      }
    });
  }
  go();
}

function stop() {
  App.sessionGeneration++;
  Resources.reset();
  background.forEach(clearInterval); background = [];
  if (visibilityHandler) { document.removeEventListener("visibilitychange", visibilityHandler); visibilityHandler = null; }
  lastVersionPollMs = 0; lastRefreshMs = 0; lastNoticesPollMs = 0;
  Notices.reset();
  App.timers.forEach(clearInterval); App.timers = [];
  App.leave.forEach(f => { try { f(); } catch { /* ignore */ } }); App.leave = [];
  window.onhashchange = null;
  Panel.what = null; Panel.autoOpened = false; Panel.userClosed = false;
  Panel.rendered=null;Resources.forget("Work");
  Theme.scope(null, null);
  AuroraBoxes.scope(null, null);
  stopPanelLive();
  adminStop();
  Panel.unwatch?.(); Panel.unwatch=null; Panel.anchors={}; Panel.layers={}; Panel.opener=null; Panel.drawn=null; Panel.what=null; App.draftRecord=null;
  App.me = null; App.computers = null; App.connectors = null; App.inbox = []; App.staged = []; App.briefing = null; App.tasks = []; App.schedules = []; App.threads = []; App.projects = []; App.workspaces = []; App.thread = null; App.feed = null; App.reported = new Set(); App.reporting = new Set();
  // What one person was typing or filtering by is not the next one's here.
  Panel.lib = { q: "", project: "", kind: "all" };
  delete Lib.all; Lib.q=""; Lib.cat="all"; Lib.tab="discover";
  for (const k of Object.keys(drafts)) delete drafts[k];
}

/** The server is there but could not answer (its database, say). The session
 * is kept, so this is not sign-in: say so, and try again on request
 * (decisions/2026-11-14-a-database-fault-is-not-a-sign-out.md).
 * @param {string} message */
function unavailableScreen(message) {
  stop();
  document.body.innerHTML = `<div class="signin"><h1>Magnus</h1><h3>The server cannot answer just now</h3>
    <p class="muted">${esc(message)}</p><div class="row"><button class="primary" data-act="retry-start">Try again</button></div></div>`;
  $("[data-act=retry-start]")?.addEventListener("click", () => start(), { once: true });
}

/** What the sign-in page says after a session ended, across the reload. */
const SESSION_NOTE = "magnus.session-note";

/**
 * The person's session here ends (sign-out, or the server ended it). The app
 * ends what ran for them (desktop/src-tauri/src/session.rs) and the page
 * loads afresh, so nothing one person had in it reaches whoever signs in
 * next (decisions/2026-11-23-sign-out-ends-everything.md).
 * @param {string} [note] said on the sign-in page that follows
 */
async function endSession(note = "") {
  stop();
  await Magnus.signOut().catch(() => {});
  try { if (note) sessionStorage.setItem(SESSION_NOTE, note); } catch { /* the note is lost, not the sign-out */ }
  location.hash = "";
  location.reload();
}

async function start() {
  let ended = "";
  try { ended = sessionStorage.getItem(SESSION_NOTE) || ""; sessionStorage.removeItem(SESSION_NOTE); } catch { /* nothing to say */ }
  try {
    const r = await Magnus.restore();
    if (r && r.update_required) return updateRequiredScreen(r.update_required);
    if (r && r.unavailable) return unavailableScreen(r.unavailable);
    if (r && r.person) { App.me = r.person; App.appliance = r.appliance; return boot(); }
    signInScreen((r && r.note) || ended);
  } catch (e) { signInScreen(e.message || String(e)); }
}

document.addEventListener("DOMContentLoaded", start);
