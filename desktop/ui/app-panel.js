// Right side panel beside the conversation (extracted from app.js).
"use strict";

// ---------- The panel beside the conversation ----------

// Whatever the person opens (a task, a file, a schedule, a decision, or a
// view over them) shows here, beside the conversation, never instead of it.
// `what` is "kind:id" or a view's name.
const Panel = { anchors:{}, layers:{}, unwatch:null, opener:null, focusNext:false, drawn:null, what: null, layer: "outcome", seq: 0, live: null, liveFor: null, liveText: "", autoOpened: false, userClosed: false, lib: { q: "", project: "", kind: "all" } };

function openPanel(what, options={}) {
  // Aurora's automatic overview is the bento under the conversation; explicit Activity tab clicks still open it.
  if (what === "activity" && options.auto && typeof AuroraStage !== "undefined" && AuroraStage.on()) { if (Panel.what) closePanel(false); return; }
  if (options.history && Panel.what!==what) {
    history.replaceState({...history.state,work:Panel.what},"",location.href);
    history.pushState({...history.state,work:what},"",location.href);
  }
  if (options.opener) Panel.opener=options.opener;
  if (options.focus) Panel.focusNext=true;
  Panel.autoOpened = !!options.auto;
  if (!options.auto) Panel.userClosed = false;
  if (Panel.what !== what) {
    if (Panel.what) Panel.layers[Panel.what]=Panel.layer;
    Panel.unwatch?.(); Panel.unwatch=null;
    // A task still at work opens on its steps: its result is not there yet.
    const atWork = (App.tasks || []).some(t => `task:${t.task_id}` === what && t.status === "running");
    Panel.layer=Panel.layers[what] || (atWork ? "trace" : "outcome"); stopPanelLive();
  }
  Panel.what = what;
  const el = $("#detail");
  if (!el) return;
  // The panel glides in when it opens (as the motion setting allows).
  if (el.hidden) { el.classList.add("opening"); el.addEventListener("animationend", () => el.classList.remove("opening"), { once: true }); }
  el.hidden = false;
  document.body.classList.add("with-detail");
  sidebar();
  markOpen();
  Workspace.opened();
  return drawPanel();
}
// The conversation's row for what the panel shows is marked, without
// redrawing the thread.
function markOpen() {
  for (const r of document.querySelectorAll(".rg-row[data-open]")) r.classList.toggle("open", r.dataset.open === Panel.what);
}
function closePanel(restoreFocus=false) {
  Panel.unwatch?.(); Panel.unwatch=null;
  if (Panel.what) Panel.layers[Panel.what]=Panel.layer;
  if (restoreFocus && Panel.what) history.pushState({...history.state,work:null},"",location.href);
  if (restoreFocus) Panel.userClosed = true;
  Panel.autoOpened = false;
  stopPanelLive();
  hush();
  Panel.what = null; Panel.drawn=null;
  if (typeof AuroraStage !== "undefined") AuroraStage.closed();
  Panel.rendered=null;Resources.forget("Work");
  const el = $("#detail");
  if (!el) return;
  el.hidden = true;
  document.body.classList.remove("with-detail");
  paint(el, "");
  markOpen();
  if ($("#side")) sidebar();
  Workspace.closed();
  if (restoreFocus) {
    const target=Panel.opener?.isConnected ? Panel.opener : $("#conv-head h1");
    if (target) { target.tabIndex=target.tabIndex<0?-1:target.tabIndex; target.focus({preventScroll:true}); }
  }
  Panel.focusNext=false;
}
function stopPanelLive() {
  if (Panel.live) Panel.live();
  Panel.live = Panel.liveFor = null;
  Panel.liveText = "";
}

const PANEL_TITLES = { activity: "Activity", needs: "Approvals", tasks: "Tasks", schedules: "Routines", files: "Files", apps: "Apps", projects: "Projects", task: "Task", file: "File", schedule: "Schedule", project: "Project", item: "Item", search: "Search", connector: "Connector", rules: "Rules" };
const PANEL_TAB_PARENT = { activity: "activity", tasks: "tasks", task: "tasks", schedules: "schedules", schedule: "schedules", needs: "needs", rules: "needs", item: "needs", apps: "apps", connector: "apps", files: "files", file: "files", projects: "projects", project: "projects" };
function panelTabs(kind) {
  const cur = PANEL_TAB_PARENT[kind] || "activity", n = needsMe().length;
  const activeTasks = (App.tasks || []).filter(t => !ENDED.includes(t.status)).length;
  return SidePanel.tabs([
    ["activity", "Activity", { href: "#/activity", open: "activity" }],
    ["tasks", "Tasks", { href: "#/tasks", open: "tasks", badge: activeTasks }],
    ["schedules", "Routines", { href: "#/schedules", open: "schedules" }],
    ["needs", "Approvals", { href: "#/inbox", open: "needs", badge: n }],
    ["files", "Files", { href: "#/files", open: "files" }],
    ["apps", "Apps", { href: "#/apps", open: "apps" }],
  ], cur, { label: "Inspector" });
}

async function drawPanel() {
  const el = $("#detail");
  if (!el || !Panel.what) return;
  const n = ++Panel.seq;
  const at = Panel.what.indexOf(":");
  const [kind, id] = at < 0 ? [Panel.what, ""] : [Panel.what.slice(0, at), Panel.what.slice(at + 1)];
  const composed = [];
  let body, after = null, head = null, failed=false;
  try {
    ({ body, after, head } = await PANELS[kind](id, composed));
  } catch (e) {
    if(e.cancelled)return;
    failed=true;
    if(n===Panel.seq){Resources.bad("Work",e);if(Panel.rendered===Panel.what)return;}
    body = `<div class="banner bad">${esc(e.status === 404 ? "This is gone: it was deleted, or it is not yours." : e.message)}</div>`;
  }
  if (n !== Panel.seq || !Panel.what) return;
  if(!failed){Panel.rendered=Panel.what;Resources.good("Work");}
  const title = kind === "search" ? `Search: ${id}` : PANEL_TITLES[kind] || kind;
  const backTo = kind === "shared" ? null : (id || kind === "rules" ? PANEL_TAB_PARENT[kind] || "activity" : null);
  const backBtn = backTo ? `<button type="button" class="link small" data-panel-back="${esc(backTo)}">← Back</button>` : "";
  const isRootCompanion = !backTo && ["activity", "tasks", "schedules", "needs", "files", "apps"].includes(kind);
  const tabsHtml = kind === "shared" ? "" : panelTabs(kind);
  const headHtml = isRootCompanion
    ? `${companionHeader()}${tabsHtml}`
    : `<div class="dh-top">${backBtn}<div class="companion-top-acts"><div class="admin-kill" data-admin-kill role="group" aria-label="Kill switch" hidden></div><button type="button" class="icon-btn close" data-act="close-panel" aria-label="Close panel" title="Close panel">${icon("close")}</button></div></div>${head || `<h2>${esc(title)}</h2>`}${tabsHtml}`;
  // A view names itself in the head (a task: its title and where it
  // stands); the others are named by what they are.
  const entry=Panel.anchors[Panel.what] ||= {anchor:{follow:false,top:0}};
  if (Panel.drawn===Panel.what) { const current=ReadingAnchors.capture($(".detail-body",el)); if(current){entry.anchor=current;entry.anchor.follow=false;} }
  const prior=entry.anchor;
  // An Aurora stage (aurora-stage.js) shows what is open as a canvas beside
  // the conversation and its cards, with the same top-bar tabs.
  const stage = typeof AuroraStage !== "undefined" && AuroraStage.on();
  const markup = stage ? AuroraStage.shell({ kind, title, head, backBtn, tabsHtml, body, foot: isRootCompanion && typeof Theme !== "undefined" && Theme.has?.("companionFooterInStage") ? companionFooter() : "" }) : SidePanel.shell({ headHtml, panelKey: kind, bodyHtml: body, footHtml: isRootCompanion ? companionFooter() : "" });
  if (n !== Panel.seq || !Panel.what) return;
  const changed = paint(el, markup); liveStatusLines(el);
  if (stage) AuroraStage.settle(el, changed);
  if (typeof showKillControl === "function" && typeof AdminState !== "undefined" && AdminState.standing?.admin) showKillControl();
  if (!changed) {
    reportOpenedSoon();
    if (Panel.focusNext) { const heading=$("h2",el); if (heading) { heading.tabIndex=-1; heading.focus({preventScroll:true}); } Panel.focusNext=false; }
    return;
  }
  Panel.unwatch?.(); Panel.drawn=Panel.what;
  ReadingAnchors.restore($(".detail-body",el),prior);
  entry.anchor=prior; Panel.unwatch=ReadingAnchors.watch($(".detail-body",el),entry,false);
  for (const [iid, view, , task] of composed) fill($(`iframe[data-view="${CSS.escape(iid)}"]`, el), view, task);
  if (after) await after(el);
  loadGlimpses(el);
  if (Panel.focusNext && n===Panel.seq) { if (!el.contains(document.activeElement)) { const heading=$("h2",el); if (heading) { heading.tabIndex=-1; heading.focus({preventScroll:true}); } } Panel.focusNext=false; }
}

// A view whose content is already read (App's objects) or read here.
const plain = body => ({ body });
async function appsPanelView() {
  const connectors = await Resources.read("Apps", "/v1/connectors", "connectors", true).catch(() => null);
  if (Array.isArray(connectors)) {
    App.connectors = connectors;
    if (typeof Lib !== "undefined") Lib.all = connectors;
  }
  const rank = c => ({ connected: 0, needs_attention: 1, local: 2, available: 3 }[stateOf(c)] ?? 4);
  const mine = (connectors || []).filter(c => c.for_you !== false).sort((a, b) => rank(a) - rank(b) || (a.title || a.name).localeCompare(b.title || b.name));
  const rowHtml = c => {
    const st = stateOf(c);
    const sub = st === "connected" ? (c.account || "Connected account") : st === "needs_attention" ? "Needs attention · Sign in again" : "One-click OAuth";
    const right = st === "connected"
      ? `<span class="papp-connected"><span class="comp-dot ok" aria-hidden="true"></span>Connected</span>`
      : `<button type="button" class="papp-btn" data-act="connect" data-app="${esc(c.name)}"${c.ready === false ? ' aria-disabled="true" title="Not ready on this server yet"' : ""}>${st === "needs_attention" ? "Reconnect" : "Connect"}</button>`;
    return `<div class="papp-row" data-connector="${esc(c.name)}" data-state="${esc(st)}">
      ${tile(c)}
      <div class="gbox-main"><a class="gbox-lbl" href="#/library/${enc(c.name)}" data-open="connector:${esc(c.name)}">${esc(c.title)}</a><span class="small muted">${esc(sub)}</span></div>
      ${right}
    </div>`;
  };
  const body = `<div class="companion-sec-head"><span>Apps</span><a class="small" href="#/library">Catalog ›</a></div>
    ${mine.length ? `<div class="papp-list">${mine.map(rowHtml).join("")}</div>` : `<p class="empty">Apps are unavailable right now.</p>`}`;
  return { body, after: el => bindConnectorActs(el) };
}
const PANELS = {
  activity: (_, composed) => plain(activityView(composed)),
  needs: (_, composed) => plain(needsView(composed)),
  tasks: () => plain(tasksView()),
  schedules: () => plain(schedulesView()),
  schedule: id => plain(scheduleView(id)),
  rules: () => ({ body: '<div class="rules-host"></div>', after: el => rulesSection($(".rules-host", el)) }),
  item: (id, composed) => {
    const i = App.inbox.find(x => x.id === id);
    return plain(i ? card(i, composed) : `<p class="empty">This decision is gone.</p>`);
  },
  task: taskView,
  file: fileView,
  files: filesView,
  apps: appsPanelView,
  connector: connectorView,
  projects: projectsView,
  project: projectView,
  search: searchView,
};

// An app Magnus offered, with why: the same card as in the Library, as the
// person stands with it now, in a conversation, a task or an approval
// (connectors spec, Inside work). Only the person whose app it is gets its
// button; anyone else deciding sees where they stand.
function appOfferCard(e, theirs = true) {
  const app = { title: e.title, listing: { letters: e.letters, tint: e.tint } };
  const STATE = { needs_attention: "Needs signing in again", needs_more_access: "Needs more access", available: "Not connected" };
  const act = e.state === "connected" ? `<span class="chip ok">Connected</span>`
    : !theirs ? `<span class="chip warn">${esc(STATE[e.state] || "Not in their Apps")}</span>`
    : e.state === "unavailable" ? `<span class="small muted">No longer in your Apps.</span>`
    : e.ready === false ? `<span class="small muted">Not ready on this server yet.</span>`
    : e.state === "needs_attention" ? `<button class="primary" data-act="connect" data-app="${esc(e.connector)}">Sign in to ${esc(e.title || "")} again</button>`
    : e.state === "needs_more_access" ? `<button class="primary" data-act="connect" data-app="${esc(e.connector)}">Approve more access</button>`
    : `<button class="primary" data-act="connect" data-app="${esc(e.connector)}">Connect ${esc(e.title || "")}</button>`;
  return `<div class="confirm chatcard conncard" data-connector="${esc(e.connector)}" data-state="${esc(e.state)}"><div class="cc-head">${tile(app)}<div><strong>${esc(e.title || e.connector)}</strong>${e.publisher ? ` <span class="small muted">by ${esc(e.publisher)}</span>` : ""}</div></div>
    ${e.why ? `<p class="cc-text">${esc(e.why)}</p>` : ""}${e.summary ? `<p class="small muted">${esc(e.summary)}</p>` : ""}
    <div class="row cc-act">${act}</div></div>`;
}

// A task: its request, the decisions it waits on (the same cards as in its
// conversation), then its outcome, what was asked and its plan, or its
// trace. How the agent checked its work is internal (spec rev 140): it is in
// the developer view and the run inspector, not here.
async function taskView(id, composed) {
  const [t, tl] = await Promise.all([Server.tasks.get(id), Panel.layer === "trace" ? Server.tasks.timeline(id) : null]);
  const running = t.status === "running";
  if (running && Panel.liveFor !== id) {
    stopPanelLive();
    Panel.liveFor = id;
    Panel.live = Magnus.live(id, txt => { Panel.liveText += txt; const el = $("#plive"); if (el) el.textContent = Panel.liveText; });
  }
  if (!running && Panel.liveFor === id) stopPanelLive();
  const items = App.inbox.filter(i => i.task_id === id && i.status === "open");
  const view = deliverable(t);
  const files = latestOutputs(t);
  const cost = Number.isFinite(t.usage?.cost_usd) ? `$${t.usage.cost_usd.toFixed(2)}` : null;
  // The result leads with the deliverable, then its files, the person's
  // verdict; what it cost closes it.
  const deliverableBar = view.length ? `<div class="row small" data-deliverable-bar style="margin-bottom:6px;justify-content:flex-end">
      <button type="button" class="link small" data-act="copy-deliverable">Copy Markdown</button>
      <button type="button" class="link small" data-act="export-deliverable">Export .md</button>
      <button type="button" class="link small" data-act="fullscreen-deliverable">Fullscreen</button>
    </div>` : "";
  const outcome = `${deliverableBar}${view.length ? `<iframe class="composed" sandbox="allow-same-origin" title="The deliverable" id="pview"></iframe>` : running ? "" : `<p class="empty">No result yet.</p>`}
    ${t.spoken && App.canReadAloud ? `<div class="row" style="margin:10px 0"><button type="button" class="icon-btn" data-act="${voice.entry===`task:${id}` ? "hush" : "speak"}" data-voice-entry="task:${esc(id)}" aria-label="${voice.entry===`task:${id}` ? "Stop reading" : "Read aloud"}" data-tooltip="${voice.entry===`task:${id}` ? "Stop reading" : "Read aloud"}"><svg class="ic" viewBox="0 0 24 24" aria-hidden="true">${ICONS[voice.entry===`task:${id}` ? "hush" : "listen"]}</svg></button></div>` : ""}
    ${files.length ? `<div class="frows">${files.map(f => fileRow(f, t.project)).join("")}</div>` : ""}
    ${evidenceSection(t)}
    ${t.outcome_view && t.summary ? `<h3>Summary</h3><p data-summary>${esc(t.summary)}</p>` : ""}
    ${ENDED.includes(t.status) ? `<details data-work-feedback data-key="feedback:${esc(id)}"><summary>Feedback</summary>${verdictBar(t.task_id, t.verdict)}</details>` : ""}
    ${cost ? `<p class="foot-note num" data-cost>${cost}</p>` : ""}
    ${t.model?.name || Number.isFinite(t.usage?.model_calls) || Number.isFinite(t.usage?.actions) ? `<details data-key="usage:${esc(id)}"><summary>Execution details</summary><p class="small muted">${[t.model?.name,Number.isFinite(t.usage?.model_calls) ? `${t.usage.model_calls} model calls` : null,Number.isFinite(t.usage?.actions) ? `${t.usage.actions} actions` : null].filter(Boolean).map(esc).join(" · ")}</p></details>` : ""}`;
  const contract = `<h3>Request</h3><p class="request">${esc(t.text)}</p>
    ${t.files?.length ? `<p class="small"><span class="muted">Your files:</span> ${t.files.map(fileChip).join(" ")}</p>` : ""}
    <h3>Plan</h3>${plan(t)}`;
  const layers = { outcome, contract, trace: tl ? trace(tl) : "" };
  const [, pill] = standing({ ...t, waiting_item: items.find(i => i.can_respond)?.id });
  const head = `<h2 class="ptitle" data-task="${esc(t.task_id)}">${esc(t.title || t.text)}</h2>
    <div class="row small dh-sub">${pill}${t.waiting_on ? `<span class="muted">${esc(WAITING[t.waiting_on] || "")}</span>` : running && t.status_line ? statusLineSlot(t, "muted") : ""}</div>`;
  const body = `${items.map(i => card(i, composed)).join("")}
    ${(t.app_offers || []).filter(o => o.state !== "connected").map(appOfferCard).join("")}
    ${t.sub_agents?.length ? subAgentsCard({ id: `s:${t.task_id}`, kind: "sub_agents", task_id: t.task_id, from_run: t.run_id, agents: t.sub_agents, at: t.created_at }, { composed }) : ""}
    ${running ? `<div class="live" id="plive" aria-live="polite">${esc(Panel.liveText) || `<span class="faint">Working…</span>`}</div>` : ""}
    <div class="seg layers" role="tablist" aria-label="What to show">${[["outcome", "Result"], ["contract", "Progress"], ["trace", "Steps"]].map(([k, l]) => `<button role="tab" aria-selected="${Panel.layer === k}" class="${Panel.layer === k ? "on" : ""}" data-layer="${k}">${l}</button>`).join("")}</div>
    <section class="layer">${layers[Panel.layer]}</section>
    ${!ENDED.includes(t.status) ? `<form class="row small" data-task-steer="${esc(id)}"><input placeholder="Steer Magnus on the next step…" aria-label="Steer Magnus" autocomplete="off"><button type="submit">Steer</button></form>` : ""}
    <div class="row pactions">${moveControl(t.task_id, t.project, App.projects)}${running ? `<button data-act="pause-task" data-id="${esc(id)}">Pause</button>` : ""}${!ENDED.includes(t.status) ? `<button data-act="cancel" data-id="${esc(id)}">Stop</button>` : trashButton(t.task_id, "Delete task")}${t.status === "suspended" && !t.waiting_item ? `<button data-act="resume" data-id="${esc(id)}">Resume</button>` : ""}</div>`;
  Panel.task = t;
  return { head, body, after: el => Panel.layer === "outcome" && view.length && fill($("#pview", el), view, id, t) };
}

// A delivered file as a row: its type, its name (opens it), where it is
// kept, and Download.
const FILE_TYPES = { "application/pdf": "PDF", "text/csv": "CSV", "text/markdown": "MD", "text/plain": "TXT", "text/html": "HTML", "application/json": "JSON" };
function fileRow(f, project) {
  const type = FILE_TYPES[f.media_type] || (f.media_type || "").split("/").pop().slice(0, 4).toUpperCase() || "FILE";
  const p = project && (typeof project === "object" ? project : App.projects.find(x => x.id === project));
  return `<div class="frow" data-file-row="${esc(f.id)}"><span class="ftype ${type === "PDF" ? "pdf" : ""}">${esc(type)}</span>
    <div class="rg-main"><a class="ttl" href="#/file/${enc(f.id)}" data-open="file:${esc(f.id)}" title="${esc(f.name)}">${esc(f.name)}</a><span class="sub">Saved to Files${p ? ` · ${esc(p.name)}` : ""} · ${esc(kb(f.size))}</span></div>
    <button class="icon-btn" data-file-download="${esc(f.id)}" data-name="${esc(f.name)}" title="Download ${esc(f.name)}">${icon("download")}<span class="sr">Download</span></button></div>`;
}

// One file, drawn the way an outcome draws it, with where it came from and
// a way to save it.
async function fileView(id) {
  const f = await Server.files.info(id);
  const path = `/v1/files/${enc(id)}`;
  const body = `<h2 class="ptitle">${esc(f.name)}</h2>
    <p class="small muted">${f.source === "delivered" ? "Delivered by Magnus" : "Uploaded by you"} · ${fileFrom(f)} · ${esc(kb(f.size))} · ${esc(when(f.at))}</p>
    <div class="row"><button class="primary" data-file-download="${esc(f.id)}" data-name="${esc(f.name)}" aria-label="Download ${esc(f.name)}">Download</button> <button type="button" data-act="ask-file" data-file-id="${esc(f.id)}" data-file-name="${esc(f.name)}">Ask Magnus</button> ${moveControl(f.id, f.project, App.projects)} ${trashButton(f.id, "Delete file")}</div>
    <iframe class="composed" sandbox="allow-same-origin" title="${esc(f.name)}" id="fview"></iframe>`;
  return {
    body,
    after: async el => {
      const files = {};
      try {
        if (GLIMPSE_IMAGE.test(f.media_type)) files[f.name] = { image: await Magnus.dataUrl(path, f.media_type) };
        else if (Compose.TEXT.test(f.media_type)) files[f.name] = { text: decodeText(await Magnus.dataUrl(path, f.media_type)), media_type: f.media_type };
      } catch (e) { toast(e.message); }
      const frame = $("#fview", el);
      if (isHtmlFile(f.name, files[f.name])) { frame.replaceWith(livePage(f.name, files[f.name].text)); return; }
      frame.removeAttribute("data-sized");
  frame.srcdoc = Compose.documentFor([{ type: "file", file: f.name }], files, fontCss(), f.preview, ...look());
      frame.onload = () => { try { frame.style.height = frame.contentDocument.documentElement.scrollHeight + 4 + "px"; } catch { /* sized by CSS */ } };
    },
  };
}
