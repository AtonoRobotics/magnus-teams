// Workspace shell, sidebar navigation, Command Palette, Shortcuts modal, and document wiring (extracted from app.js).
"use strict";

// ---------- shell ----------

// One workspace: the conversation in the middle, what was asked of it beside
// it in a panel, and the sidebar. Everything it shows is read together
// (refresh), so a task, a schedule or a decision is one object wherever it
// appears, and acting on it anywhere redraws every place it is shown.
const DECISION_KINDS=new Set(["monitor","secure_form","takeover_request","payment","approval","question","contract_change","review","policy_proposal","app_tools","deletion_request","endorsement"]);
// Magnus's own ideas (suggestions) are no decision waiting: they are in
// Explore's For you (explore-ideas.js).
// Whether an item asks the person to decide: an approval, a question, a
// blocking notice. An observation, a message or a suggestion does not.
function wasAsk(i) { return DECISION_KINDS.has(i.kind) || (i.kind === "notice" && !!i.blocking); }
const needsDecision=i=>i.status==="open" && i.can_respond && wasAsk(i) && !(i.details?.notify_at && Date.parse(i.details.notify_at)>Date.now());
const needsMe = () => App.inbox.filter(needsDecision);
// Approvals one click may allow together (Nova spec §4.5 Tier 2): of the same
// kind as the first that waits (the same tool), and never Tier 1 — money or
// what cannot be undone — nor a prohibited one. Fewer than two is no batch.
const BATCH_EXCLUDED_TIERS = new Set(["financial", "irreversible", "prohibited"]);
const batchableApprovals = pending => {
  const ok = pending.filter(x => x.kind === "approval" && x.can_respond && !BATCH_EXCLUDED_TIERS.has(x.tier));
  const tool = ok[0]?.details?.tool;
  const same = tool ? ok.filter(x => x.details?.tool === tool) : [];
  return same.length >= 2 ? same.map(x => x.id) : [];
};
const isSystemSchedule = sc => !!(sc?.system || /heartbeat/i.test(sc?.title || sc?.text || ""));
const isSystemTask = t => !!(t?.system || (t?.group?.kind === "schedule" && /heartbeat/i.test(t.group.title || "")) || /^heartbeat\b/i.test(t?.title || t?.text || ""));
const isQuietSystemTask = t => isSystemTask(t) && !t.needs_you && t.status !== "failed";

// The workspace's icons, drawn as in the Magnus Workspace design: 24px
// outlines in the current color.
const UI = {
  plus: '<path d="M12 5v14M5 12h14"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
  bell: '<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  tasks: '<path d="M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01"/>',
  calendar: '<rect x="3" y="5" width="18" height="16" rx="2"/><path d="M16 3v4M8 3v4M3 10h18"/>',
  file: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>',
  chat: '<path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12z"/>',
  trash: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>',
  gear: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>',
  done: '<circle cx="12" cy="12" r="9"/><path d="m8.5 12 2.5 2.5 4.5-5"/>',
  need: '<circle cx="12" cy="12" r="9"/><path d="M12 8v5M12 16h.01"/>',
  working: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  stopped: '<circle cx="12" cy="12" r="9"/><path d="M15 9l-6 6M9 9l6 6"/>',
  globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>',
  attach: '<path d="m21 11-8.6 8.6a5 5 0 0 1-7-7l8.5-8.6a3.3 3.3 0 0 1 4.7 4.7l-8.6 8.6a1.7 1.7 0 0 1-2.3-2.3l8-8"/>',
  send: '<path d="M12 19V5M5 12l7-7 7 7"/>',
  shield: '<path d="M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z"/><path d="m9 12 2 2 4-4"/>',
  close: '<path d="M18 6 6 18M6 6l12 12"/>',
  download: '<path d="M12 4v11M7 10l5 5 5-5M5 20h14"/>',
  chev: '<path d="m9 6 6 6-6 6"/>',
  grid: '<rect x="4" y="4" width="7" height="7" rx="1.5"/><rect x="13" y="4" width="7" height="7" rx="1.5"/><rect x="4" y="13" width="7" height="7" rx="1.5"/><rect x="13" y="13" width="7" height="7" rx="1.5"/>',
  sparkles: '<path d="m12 3 1.6 4.4L18 9l-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.6Z"/><path d="M19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8Z"/>',
  palette: '<circle cx="12" cy="12" r="9"/><circle cx="9" cy="10" r="1"/><circle cx="15" cy="10" r="1"/><circle cx="10" cy="15" r="1"/>',
  pencil: '<path d="M12 20h9M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/>',
  mail: '<rect x="3" y="5" width="18" height="14" rx="2"/><path d="m3 7 9 6 9-6"/>',
  team: '<circle cx="9" cy="8" r="3"/><path d="M3 20a6 6 0 0 1 12 0"/><path d="M16 5.5a3 3 0 0 1 0 5M18 14.5a6 6 0 0 1 3 5.5"/>',
  mic: '<path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><line x1="12" y1="19" x2="12" y2="22"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none"/>',
  sidebar: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M15 4v16"/>',
  pause: '<rect x="6" y="5" width="4" height="14" rx="1"/><rect x="14" y="5" width="4" height="14" rx="1"/>',
  play: '<polygon points="6 4 20 12 6 20 6 4"/>',
  expand: '<path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7"/>',
  inbox: '<path d="M22 12h-6l-2 3h-4l-2-3H2"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11Z"/>',
  layers: '<path d="m12 2 10 5-10 5L2 7Z"/><path d="m2 17 10 5 10-5"/><path d="m2 12 10 5 10-5"/>',
  brain: '<path d="M9.5 2a3.5 3.5 0 0 0-3.2 4.9A3.5 3.5 0 0 0 5 13.5 3.5 3.5 0 0 0 8.5 17H9v3a1 1 0 0 0 2 0V4.5A2.5 2.5 0 0 0 9.5 2Z"/><path d="M14.5 2A2.5 2.5 0 0 0 13 4.5V20a1 1 0 0 0 2 0v-3h.5a3.5 3.5 0 0 0 3.5-3.5 3.5 3.5 0 0 0-1.3-6.6A3.5 3.5 0 0 0 14.5 2Z"/>',
};
const icon = (k, cls = "") => `<svg class="ic${cls ? ` ${cls}` : ""}" viewBox="0 0 24 24" aria-hidden="true">${UI[k]}</svg>`;

function displayTitle(title) {
  const t = String(title || "").trim();
  if (!t || t.startsWith("This turn is not a message from the person")) return "Automated check";
  // A title is the first words of a message, which may be Markdown: it is
  // shown as the words, without the marks. A title is cut mid-sentence, so a
  // mark left without its pair goes too, and a list item inside bold loses
  // its number.
  const lead = /^#{1,6}\s+|^\s*(?:[-*+>]|\d+\.)\s+/gm;
  const plain = t.replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1").replace(lead, "")
    .replace(/(\*\*|__|~~|`)(.+?)\1/g, "$2").replace(/(^|[\s(])[*_](\S(?:.*?\S)?)[*_](?=[\s).,!?:;]|$)/g, "$1$2")
    .replace(/\*\*|~~|`|(^|\s)__|__(?=\s|$)/g, "$1").replace(lead, "").replace(/\s+/g, " ").trim();
  return plain || t;
}

// The sidebar is a chat agent's (decisions/2026-11-28-chat-first-navigation.md):
// New chat and Search, then the projects, personal and shared in one list,
// then Recents, every conversation newest first, personal and shared alike.
// The rest of the app is one row of icons at its foot, above the person.
function navigationTree() {
  const current=href=>(location.hash || "#/home")===href ? 'class="on" aria-current="page"' : "";
  const route=location.hash || "#/home";
  const waitingThreads=new Set(needsMe().map(i=>i.thread).filter(Boolean));
  const activeThreads=new Set(App.tasks.filter(t=>!ENDED.includes(t.status) && t.thread && !(typeof isQuietSystemTask==="function" ? isQuietSystemTask(t) : (t?.system && !t.needs_you && t.status!=="failed"))).map(t=>t.thread));
  const dot=t=>waitingThreads.has(t.thread) ? '<span class="nav-dot warn" aria-hidden="true"></span>' : activeThreads.has(t.thread) ? '<span class="nav-dot run" aria-hidden="true"></span>' : "";
  const conv=t=>{const ttl=displayTitle(t.title);return `<div class="nav-conv-row"><a class="conv${t.thread===App.thread ? " on" : ""}" href="#/chat/${enc(t.thread)}" ${t.thread===App.thread ? 'aria-current="page"' : ""} title="${esc(ttl)}">${dot(t)}<span class="ttl">${esc(ttl)}</span></a><button type="button" class="nav-conv-del" data-trash="${esc(t.thread)}" data-trash-label="Delete conversation"${t.thread===App.thread ? ' data-then="#/home"' : ""} title="Delete conversation" aria-label="Delete ${esc(ttl)}">${icon("trash")}</button></div>`;};
  const homes=(typeof Team!=="undefined" && Team.homes) || {};
  const countBadge=c=>c.mentions ? `<span class="pill" title="${c.mentions} mention${c.mentions===1?"":"s"}">${c.mentions}</span>` : (c.unread && c.notify!=="off") ? `<span class="small muted">${c.unread}</span>` : "";
  const teamBadge=w=>{const chs=homes[w.id]?.channels || [];const m=w.mentions ?? chs.reduce((n,c)=>n+(c.mentions||0),0);const u=w.unread ?? chs.reduce((n,c)=>n+(c.notify==="off"?0:(c.unread||0)),0);return m?`<span class="pill" title="${m} mention${m===1?"":"s"}">${m}</span>`:u?`<span class="small muted">${u}</span>`:"";};
  const shared=`<span class="nav-shared" role="img" aria-label="Shared" title="Shared">${icon("team")}</span>`;
  const row=(href,name,extra,del)=>{const on=route===href;return `<div class="nav-conv-row"><a class="conv${on ? " on" : ""}" href="${href}" ${on ? 'aria-current="page"' : ""} title="${esc(name)}">${extra.lead || ""}<span class="ttl">${esc(name)}</span>${extra.tail || ""}</a>${del}</div>`;};
  const del=(attr,id,name)=>`<button type="button" class="nav-conv-del" ${attr}="${esc(id)}" data-nav-del-name="${esc(name)}" title="Delete ${esc(name)}" aria-label="Delete ${esc(name)}">${icon("trash")}</button>`;
  // Projects: personal and shared in one list, by name; a shared one carries the people mark.
  const projects=[
    ...(App.projects || []).map(p=>({name:p.name, html:row(`#/project/${enc(p.id)}`,p.name,{lead:icon("folder")},del("data-nav-del-project",p.id,p.name))})),
    ...(App.workspaces || []).map(w=>({name:w.name, html:row(`#/team/${enc(w.id)}`,w.name,{lead:icon("folder"),tail:`${shared}${teamBadge(w)}`},(w.role==="owner" || w.role==="admin") ? del("data-nav-del-shared",w.id,w.name) : "")})),
  ].sort((a,b)=>String(a.name).localeCompare(String(b.name))).map(x=>x.html).join("");
  // Recents: every conversation, newest first. A shared chat with no time
  // of its own is recent while it has unread messages.
  const at=(v,fallback)=>{const ts=Date.parse(v || "");return Number.isNaN(ts) ? fallback : ts;};
  const recents=(App.threads || []).map(t=>({at:at(t.updated_at || t.created_at,Date.now()),html:conv(t)}));
  for (const w of (App.workspaces || [])) {
    const home=homes[w.id];
    if (!home) continue;
    for (const c of (home.channels || []).filter(x=>!x.archived && !x.deleted && !x.direct?.agent)) {
      const href=`#/team/${enc(w.id)}/c/${enc(c.id)}`;
      const on=route===href || route.startsWith(`${href}/`);
      const others=(c.direct?.people || []).filter(p=>p!==home.me);
      const name=c.direct ? (others.map(p=>home.members.find(m=>m.id===p)?.name || p).join(", ") || `${home.me_name} (you)`) : c.name==="general" ? w.name : `${c.name} · ${w.name}`;
      const lead=c.direct ? `<span class="nav-dot${others[0] && home.members.find(m=>m.id===others[0])?.online ? " ok" : ""}" aria-hidden="true"></span>` : "";
      recents.push({at:at(c.last_at || c.updated_at,c.unread ? Date.now() : 0),html:`<a class="conv${on ? " on" : ""}" href="${href}" ${on ? 'aria-current="page"' : ""} title="${esc(name)}">${lead}<span class="ttl">${esc(name)}</span>${shared}${countBadge(c)}</a>`});
    }
  }
  recents.sort((a,b)=>b.at-a.at);
  const inlineNew = typeof Projects !== "undefined" ? Projects.inlineNewHtml() : "";
  return `<div class="sect" role="group" aria-label="Projects"><div class="group"><a href="#/projects" class="nav-group-link">Projects</a><span class="nav-group-acts"><button type="button" class="icon-btn nav-group-add" data-new-project title="New project" aria-label="New project">${icon("plus")}</button></span></div>${inlineNew}${projects}</div>
    <div class="sect" role="group" aria-label="Recents"><div class="group"><span>Recents</span></div>${recents.length ? recents.map(x=>x.html).join("") : '<p class="small muted pad">No chats yet.</p>'}</div>
    ${isDev() ? `<div class="sect"><details class="developer-nav"><summary>Developer</summary>${[["runs","Runs"],["experiments","Experiments"],["system","System"],["policy","Policy"],["playbook","Playbook"],["computers","Computers"]].map(([k,l])=>`<a href="#/dev/${k}" ${current(`#/dev/${k}`)}>${l}</a>`).join("")}</details></div>` : ""}`;
}
function nav() {
  const route=location.hash || "#/home";
  const pending=typeof needsMe === "function" ? needsMe().length : 0;
  const nova = typeof Theme !== "undefined" && Theme.has?.("topBar");
  // Nova names the places as its spec does (§4.1).
  const L=nova ? { inbox: "Inbox / Approvals", briefing: "Briefing & Routines", apps: "Apps & Skills" } : { inbox: "Approvals", briefing: "Briefing", apps: "Apps" };
  // Nova draws its mockup's icons: a pencil, an inbox tray, a grid for
  // Explore and layers for Apps.
  const I=nova ? { new: "pencil", inbox: "inbox", explore: "grid", apps: "layers" } : { new: "plus", inbox: "need", explore: "sparkles", apps: "grid" };
  const initial=esc((App.me?.name || "?").trim().charAt(0).toUpperCase());
  // In Nova the person's initial is the way to Settings, as in its mockup.
  const avatar=nova ? `<a href="#/settings" class="avatar" aria-label="Settings" title="Settings">${initial}</a>` : `<span class="avatar" aria-hidden="true">${initial}</span>`;
  const more=(href,cls,[label,title],ic,test,extra="")=>`<a href="${href}" class="icon-btn ${cls}${test ? " on" : ""}" ${test ? 'aria-current="page"' : ""} aria-label="${esc(label)}" title="${esc(title)}">${icon(ic)}${extra}</a>`;
  return `${App.update?.ready ? `<div class="update-ready"><span class="small">Magnus ${esc(App.update.ready.version)} is ready.</span><button data-act="restart-update" title="Restart to update">${icon("download")}<span class="update-label">Restart to update</span></button></div>` : ""}
    <a href="#/chat/new" class="new-chat primary-nav" aria-label="New chat" title="New chat">${icon(I.new)}<span class="nav-label">New chat</span></a>${nova ? `<button type="button" class="nav-quick-jump" data-quick-jump title="Search (⌘K)">${icon("search")}<span class="nav-label">Search</span><kbd class="kbd" aria-hidden="true">⌘K</kbd></button>` : ""}<a href="#/projects" class="nav-projects primary-nav${route.startsWith("#/project") ? " on" : ""}" ${route.startsWith("#/project") ? 'aria-current="page"' : ""} aria-label="Projects" title="Projects">${icon("folder")}<span class="nav-label">Projects</span></a>
    <button type="button" class="rail-tree icon-btn" data-nav-tree aria-label="Projects and conversations" title="Projects and conversations" aria-expanded="false" aria-controls="navigation-flyout">${icon("folder")}</button>
    <div class="nav-tree">${navigationTree()}</div>
    <div class="nav-more" role="group" aria-label="More">${more("#/inbox","nav-inbox",[`Approvals${pending ? `, ${pending} waiting` : ""}`,L.inbox],I.inbox,route==="#/inbox",pending ? `<span class="nav-inbox-count" aria-hidden="true">${pending > 9 ? "9+" : pending}</span>` : "")}${more("#/briefing","nav-briefing",["Briefing and routines",L.briefing],"clock",route==="#/briefing")}${more("#/explore","nav-explore",["Explore","Explore"],I.explore,route==="#/explore")}${more("#/library","nav-apps",["Apps",L.apps],I.apps,route.startsWith("#/library"))}${more("#/trash","nav-trash",["Trash","Trash"],"trash",route==="#/trash")}</div>
    <div class="foot"><div class="who">${avatar}<div class="whoami" data-appliance="${esc(App.appliance)}"><span class="ttl" title="${esc(App.me?.id)}">${esc(App.me?.name)}</span><button class="link" data-act="sign-out">Sign out</button></div><a href="#/settings" class="icon-btn${route.startsWith("#/settings") ? " on" : ""}" ${route.startsWith("#/settings") ? 'aria-current="page"' : ""} aria-label="Settings" title="Settings">${icon("gear")}</a></div></div>`;
}
function sidebar() {
  paint($("#side"),nav());
  if (!$("#navigation-flyout")?.hidden) paint($("#navigation-flyout"),`<h2>Projects and conversations</h2>${navigationTree()}<button data-act="sign-out">Sign out</button><button data-close-tree>Close navigation list</button>`);
  if (typeof AuroraRail !== "undefined" && AuroraRail.syncBadge) AuroraRail.syncBadge();
}

// Fit is based on usable content width, independently of stored preferences.
const Workspace = (() => {
  let expanded=false, region="conversation", forceExpanded=false, installed=false, restoreDrawer=true;
  // A team workspace brings its own rail, so the app's navigation folds to
  // icons there unless the person opens it.
  function fit(width, preferred="expanded", panelWidth=Theme.LAYOUT_BOUNDS.panelDefaultWidthPx, work=false, team=false) {
    const B=Theme.LAYOUT_BOUNDS;
    const mobile=width<B.mobileMaxWidthPx;
    const temporary=(work || team) && !expanded && !forceExpanded && preferred==="expanded" && (team || width-B.navExpandedWidthPx<B.splitMinAvailablePx);
    const nav=mobile ? "drawer" : preferred==="compact" || temporary ? "compact" : "expanded";
    const navWidth=mobile ? 0 : nav==="compact" ? B.navCompactWidthPx : B.navExpandedWidthPx, available=width-navWidth;
    const eligible=work && available>=B.splitMinAvailablePx;
    const max=Math.min(B.panelMaxWidthPx,available-(B.conversationMinWidthPx+B.splitterWidthPx));
    const actual=Math.max(B.panelMinWidthPx,Math.min(B.panelMaxWidthPx,Number(panelWidth)||B.panelDefaultWidthPx));
    const panel=eligible ? Math.min(actual,Math.max(B.panelMinWidthPx,max)) : actual;
    return {nav,navWidth,available,eligible,split:eligible && !expanded,min:B.panelMinWidthPx,max:Math.max(B.panelMinWidthPx,max),panel,conversation:Math.max(B.conversationMinWidthPx,available-B.splitterWidthPx-panel),temporary};
  }
  function update() {
    const shell=$(".shell"); if (!shell) return;
    const B=Theme.LAYOUT_BOUNDS, prefs=Theme.prefs(), work=!!Panel.what && !!App.workspace;
    const m=fit(shell.clientWidth,Theme.navigation(prefs),prefs.panelWidthPx || B.panelDefaultWidthPx,work,false);
    if (work && Panel.autoOpened && (shell.clientWidth < B.autoOpenMinWidthPx || !m.eligible)) { closePanel(false); return m; }
    delete document.body.dataset.team;
    const prior=[...document.querySelectorAll("#thread, #detail .detail-body")].filter(el=>el.clientHeight).map(root=>{
      const anchor=ReadingAnchors.capture(root);
      if (anchor && root.id!=="thread") anchor.follow=false;
      return {root,anchor};
    });
    const railDrawerOpen = m.nav==="compact" && typeof AuroraRail!=="undefined" && AuroraRail.open && $("#navigation-drawer")?.open;
    shell.style.setProperty("--nav-width",`${m.navWidth}px`);shell.style.setProperty("--panel-width",`${m.panel}px`);shell.style.setProperty("--conversation-min-width",`${B.conversationMinWidthPx}px`);shell.style.setProperty("--splitter-width",`${B.splitterWidthPx}px`);
    document.body.dataset.navigation=railDrawerOpen ? "expanded" : m.nav;document.body.dataset.workspace=!work ? "conversation" : m.split ? "split" : "single";document.body.dataset.region=region;
    const height=Math.min(innerHeight,window.visualViewport?.height || innerHeight);shell.style.setProperty("--workspace-height",`${height}px`);shell.style.setProperty("--workspace-top",`${window.visualViewport?.offsetTop || 0}px`);document.body.classList.toggle("short-viewport",height<B.shortViewportMaxHeightPx);
    const toggle=$("[data-nav-toggle]");if(toggle){toggle.setAttribute("aria-label",m.nav==="expanded" ? "Collapse navigation" : "Expand navigation");toggle.title=toggle.getAttribute("aria-label");toggle.setAttribute("aria-expanded",String(m.nav==="expanded"));}
    $("[data-open-drawer]").hidden=m.nav!=="drawer";
    const selectors=$(".workspace-selectors");selectors.hidden=!work || m.split;
    for(const b of $$("[data-region]",selectors)){const on=b.dataset.region===region;b.setAttribute("aria-pressed",String(on));}
    const grow=$("[data-expand-work]");grow.hidden=!work || (!m.eligible && !expanded);grow.setAttribute("aria-label",expanded ? "Restore split view" : "Expand work");grow.title=grow.getAttribute("aria-label");grow.setAttribute("aria-pressed",String(expanded));
    $("[data-close-work]").hidden=!work || m.split || region==="work";
    $(".workspace-heading").textContent=App.workspace ? "Conversation" : location.hash.startsWith("#/library") ? "Apps" : location.hash.startsWith("#/settings") ? "Settings" : location.hash.startsWith("#/admin") ? "Admin" : location.hash.startsWith("#/dev") ? "Developer" : "Magnus";
    $(".workspace-heading").hidden=work;
    const separator=$("#workspace-splitter");separator.hidden=!m.split;separator.setAttribute("aria-valuenow",String(Math.round(m.panel)));separator.setAttribute("aria-valuemin",String(m.min));separator.setAttribute("aria-valuemax",String(m.max));separator.setAttribute("aria-valuetext",`${Math.round(m.panel)} pixels`);
    if (m.nav!=="drawer" && !railDrawerOpen && $("#navigation-drawer").open) closeDrawer(false);
    for(const a of prior){if(!a.root.clientHeight || !a.anchor)continue;ReadingAnchors.restore(a.root,a.anchor);}
    reportOpenedSoon();
    return m;
  }
  function remember() { history.replaceState({...history.state,workspace:{work:Panel.what,expanded,region}},"",location.href); }
  function choose(which) {region=which;remember();update();const target=which==="work" ? $("#detail h2") : $("#msg");if(target){if(which==="work")target.tabIndex=-1;target.focus({preventScroll:true});}}
  function opened() {const saved=history.state?.workspace;if(saved?.work===Panel.what){expanded=!!saved.expanded;region=saved.region==="conversation" ? "conversation" : "work";}else region="work";forceExpanded=false;remember();update();}
  function closed() {expanded=false;region="conversation";remember();update();}
  function openDrawer() {
    const dialog=$("#navigation-drawer"),nav=$("nav.side");if(dialog.open)return;
    dialog.append(nav);restoreDrawer=true;dialog.showModal();$("[data-close-drawer]",nav).focus();
  }
  function closeDrawer(restore=true) {const d=$("#navigation-drawer");if(!d?.open)return;restoreDrawer=restore;d.close();}
  function closeTree(restore=true) {const fly=$("#navigation-flyout");if(!fly)return;fly.hidden=true;const b=$("[data-nav-tree]");b?.setAttribute("aria-expanded","false");if(restore)b?.focus({preventScroll:true});}
  function resize(value) {const m=update();Theme.set({panelWidthPx:Math.max(m.min,Math.min(m.max,value))});update();}
  function openShortcutsModal() {
    let d = $("#shortcuts-modal");
    if (d?.open) { d.close(); return; }
    if (!d) {
      d = document.createElement("dialog");
      d.id = "shortcuts-modal";
      d.className = "shortcuts-dialog";
      d.setAttribute("aria-label", "Keyboard shortcuts");
      d.innerHTML = `<div class="shortcuts-head"><h2>Keyboard shortcuts</h2><button type="button" class="icon-btn" data-close-shortcuts aria-label="Close shortcuts">${icon("close")}</button></div>
        <ul class="shortcuts-list">
          <li><span>Quick jump &amp; search</span><kbd class="shortcut-kbd">⌘ / Ctrl + K</kbd></li>
          <li><span>Toggle sidebar</span><kbd class="shortcut-kbd">⌘ / Ctrl + B</kbd></li>
          <li><span>Send message</span><kbd class="shortcut-kbd">Enter</kbd></li>
          <li><span>New line in message</span><kbd class="shortcut-kbd">Shift + Enter</kbd></li>
          <li><span>Stop active work (empty composer)</span><kbd class="shortcut-kbd">Esc</kbd></li>
          <li><span>Format composer text (bold / italic / code / link)</span><kbd class="shortcut-kbd">⌘ / Ctrl + B / I / E / K</kbd></li>
          <li><span>Triage waiting approvals &amp; inbox (Next / Prev / Allow / Dismiss)</span><kbd class="shortcut-kbd">J / K / A / D</kbd></li>
          <li><span>Navigate / commands, @context or palette</span><kbd class="shortcut-kbd">↑ / ↓</kbd></li>
          <li><span>Keyboard shortcuts</span><kbd class="shortcut-kbd">⌘ / Ctrl + / or ?</kbd></li>
        </ul>`;
      d.addEventListener("click", e => { if (e.target === d || e.target.closest("[data-close-shortcuts]")) d.close(); });
      d.addEventListener("close", () => d.remove());
      document.body.append(d);
    }
    d.showModal();
    $("[data-close-shortcuts]", d)?.focus();
  }
  function openPalette() {
    if (typeof CommandBar !== "undefined") CommandBar.open();
  }
  function install() {
    expanded=false;region="conversation";forceExpanded=false;
    const d=$("#navigation-drawer");d.addEventListener("click",e=>{if(e.target===d){const r=d.getBoundingClientRect();if(e.clientX<r.left || e.clientX>r.right || e.clientY<r.top || e.clientY>r.bottom)closeDrawer();}});d.addEventListener("close",()=>{const nav=$("nav.side",d);if(nav)$(".shell").prepend(nav);if(restoreDrawer)$("[data-open-drawer]")?.focus({preventScroll:true});});
    $("#workspace-bar").onclick=e=>{const b=e.target.closest("button");if(!b)return;if(b.matches("[data-open-drawer]"))openDrawer();if(b.dataset.region)choose(b.dataset.region);if(b.matches("[data-expand-work]")){expanded=!expanded;if(expanded)region="work";remember();update();}if(b.matches("[data-close-work]"))closePanel(true);};
    $("[data-nav-toggle]").onclick=()=>{const m=update(),navigation=m.nav==="compact" ? "expanded" : "compact";forceExpanded=navigation==="expanded";Theme.set({navigation});update();};
    $("[data-close-drawer]").onclick=()=>closeDrawer();
    $("[data-rail-search]").onclick=()=>openPalette();
    if (typeof CommandBar !== "undefined") CommandBar.install();
    const separator=$("#workspace-splitter"), B=Theme.LAYOUT_BOUNDS;
    separator.onkeydown=e=>{const m=update();let width=m.panel;if(e.key==="Home")width=m.min;else if(e.key==="End")width=m.max;else if(["ArrowLeft","ArrowRight"].includes(e.key))width+=(e.key==="ArrowLeft" ? 1 : -1)*(e.shiftKey ? B.splitterLargeStepPx : B.splitterStepPx);else return;e.preventDefault();resize(width);};
    separator.onpointerdown=e=>{if(e.button!==0)return;const m=update(),x=e.clientX,width=m.panel;separator.setPointerCapture(e.pointerId);separator.onpointermove=move=>resize(width+x-move.clientX);separator.onpointerup=()=>{separator.onpointermove=null;separator.onpointerup=null;separator.releasePointerCapture(e.pointerId);};};
    separator.ondblclick=()=>resize(B.panelDefaultWidthPx);
    if(!installed){installed=true;
      window.addEventListener("resize",()=>{forceExpanded=false;update();closeTree(false);});window.visualViewport?.addEventListener("resize",update);window.visualViewport?.addEventListener("scroll",()=>$(".shell")?.style.setProperty("--workspace-top",`${window.visualViewport?.offsetTop || 0}px`));window.addEventListener("magnus-theme",update);
      document.addEventListener("click",e=>{
        const tree=e.target.closest("[data-nav-tree]");if(tree){const fly=$("#navigation-flyout");if(fly.hidden){fly.hidden=false;sidebar();tree.setAttribute("aria-expanded","true");$("a,button",fly)?.focus({preventScroll:true});}else closeTree();return;}
        if(e.target.closest("[data-close-tree]"))closeTree();else if(!e.target.closest("#navigation-flyout"))closeTree(false);
        const destination=e.target.closest("nav.side a, #navigation-flyout a");
        if(destination){if(e.button===0 && !e.ctrlKey && !e.metaKey && !destination.dataset.open && !destination.dataset.startDm && destination.hash!==location.hash)App.routeFocus={opener:destination,composer:destination.hash.startsWith("#/chat/new")};closeTree(false);closeDrawer(false);}
        if(e.target.closest("#navigation-flyout [data-act=sign-out]"))$("nav.side [data-act=sign-out]")?.click();
      });
      document.addEventListener("keydown",e=>{
        if(e.defaultPrevented)return;
        if(e.key==="Escape" && !$("#navigation-flyout")?.hidden){e.preventDefault();closeTree();}
        if((e.metaKey || e.ctrlKey) && e.key.toLowerCase()==="k"){e.preventDefault();openPalette();return;}
        if((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase()==="b"){
          const stage = typeof Theme !== "undefined" && Theme.stage?.(document.documentElement.dataset.style);
          if(!stage && !document.querySelector("dialog[open]")){e.preventDefault();$("[data-nav-toggle]")?.click();}
          return;
        }
        if(((e.metaKey || e.ctrlKey) && e.key === "/") || (!e.metaKey && !e.ctrlKey && !e.altKey && e.key === "?" && !e.target?.closest?.("input, textarea, select, [contenteditable]"))){
          if(!document.querySelector("dialog[open]")){e.preventDefault();openShortcutsModal();}
        }
      });
    }
    update();
  }
  return {fit,update,install,opened,closed,choose,openDrawer,openPalette,openShortcutsModal};
})();

function shell() {
  document.body.innerHTML = `<div class="shell"><nav class="side" aria-label="Sections"><div class="brand"><span class="logo" aria-hidden="true"><svg width="20" height="20" viewBox="0 0 64 64" fill="none"><polygon points="18.7,23 24.7,23 16.3,50 10.3,50" fill="currentColor" opacity="0.52"/><polygon points="34.4,19 40.4,19 30.8,50 24.8,50" fill="currentColor"/><polygon points="47.7,23 53.7,23 45.3,50 39.3,50" fill="currentColor" opacity="0.52"/></svg></span><span class="brand-name">Magnus</span><button class="icon-btn" data-nav-toggle aria-label="Collapse navigation" title="Collapse navigation">${icon("chev")}</button><button class="icon-btn" data-close-drawer aria-label="Close navigation">${icon("close")}</button></div>
    <button class="icon-btn rail-search" data-rail-search aria-label="Search conversations, tasks and files" title="Search conversations, tasks and files">${icon("search")}</button>
    <form id="find" role="search">${icon("search")}<label class="sr" for="q">Search conversations, tasks and files</label><input id="q" type="search" placeholder="Search…" autocomplete="off"><kbd class="kbd" aria-hidden="true">⌘K</kbd></form><div id="side"></div></nav>
    <header id="workspace-bar" class="workspace-bar"><button class="icon-btn" data-open-drawer aria-label="Open navigation">${icon("tasks")}</button><span class="workspace-heading">Conversation</span><div class="workspace-selectors" aria-label="Workspace region"><button data-region="work" aria-pressed="false">Work</button><button data-region="conversation" aria-pressed="true">Conversation</button></div><span class="grow"></span><div class="admin-kill" data-admin-kill-top role="group" aria-label="Kill switch" hidden></div><button class="icon-btn" data-expand-work aria-label="Expand work">${icon("grid")}</button><button class="icon-btn" data-close-work data-act="close-panel" aria-label="Close work">${icon("close")}</button></header>
    <main id="main" tabindex="-1"></main><div id="workspace-splitter" role="separator" tabindex="0" aria-label="Conversation width" aria-orientation="vertical" hidden></div><aside id="detail" class="detail" aria-label="Work" hidden><div class="admin-kill" data-admin-kill role="group" aria-label="Kill switch" hidden></div></aside></div>
    <dialog id="navigation-drawer" class="navigation-drawer" aria-label="Navigation"></dialog><div id="navigation-flyout" class="navigation-flyout" role="dialog" aria-label="Projects and conversations" hidden></div>
    <dialog id="workspace-search" class="workspace-search" aria-label="Search conversations, tasks and files"><form><label for="workspace-query">Jump or search conversations, tasks and files</label><input id="workspace-query" type="search" role="combobox" aria-autocomplete="list" aria-controls="workspace-palette-list" aria-expanded="false" placeholder="Jump to a page, chat, or search everything…" autocomplete="off"><div id="workspace-palette-list" class="palette-list" role="listbox" aria-label="Quick jump"></div><div class="row"><button type="submit">Search all</button><button type="button" data-close-search>Close search</button></div></form></dialog>`;
  Workspace.install();
  $("nav.side").addEventListener("click", async e => {
    if (e.target.closest("[data-act=restart-update]")) return restartToUpdate();
    if (e.target.closest("[data-quick-jump]")) return Workspace.openPalette();
    if (e.target.closest("[data-act=sign-out]")) {
      MessageDrafts.clear();
      await endSession();
    }
  });
  $("#find").onsubmit = e => { e.preventDefault(); const q = $("#q").value.trim(); if (q) openPanel(`search:${q}`,{history:true,focus:true,opener:$("#q")}); };
  sidebar();
  wirePanel();
  if (!App.wired) { App.wired = true; wireDocument(); }
}

// One open menu and one shared tooltip, for both input families.
const Menus = (() => {
  let current = null, trigger = null, anchorPosition = null;
  const items = () => current ? $$('[role="menuitem"]:not(:disabled), [role="menuitemcheckbox"]:not(:disabled)', current) : [];
  function close(restore = false) {
    if (!current) return;
    current.hidden = true; trigger?.setAttribute("aria-expanded", "false");
    const back = trigger; current = trigger = null; anchorPosition=null;
    if (restore && back?.isConnected) back.focus({ preventScroll: true });
  }
  function open(button, focus = true) {
    const menu = document.getElementById(button.dataset.menu);
    if (!menu) return;
    if (current === menu) return close(true);
    // A transformed entry becomes the containing block of fixed descendants.
    // End its entrance before placing a viewport-relative menu.
    button.closest('.entry.enter')?.classList.remove('enter');
    close(); current = menu; trigger = button; menu.hidden = false;
    button.setAttribute("aria-expanded", "true");
    const anchor = button.getBoundingClientRect(), r = menu.getBoundingClientRect();
    anchorPosition={left:anchor.left,top:anchor.top};
    menu.style.left = `${Math.max(8, Math.min(anchor.left, innerWidth - r.width - 8))}px`;
    menu.style.top = `${anchor.bottom + r.height <= innerHeight - 8 ? anchor.bottom : Math.max(8, anchor.top - r.height)}px`;
    if (focus) items()[0]?.focus({ preventScroll: true });
  }
  function install() {
    document.addEventListener("click", e => {
      const button = e.target.closest("[data-menu]");
      if (button) { e.preventDefault(); open(button); return; }
      if (current && (e.target.closest('[role="menuitem"], [role="menuitemcheckbox"]') || !current.contains(e.target))) close();
    });
    document.addEventListener("keydown", e => {
      if (!current) return;
      if (e.key === "Escape") { e.preventDefault(); close(true); return; }
      if (e.key === "Tab") { trigger?.focus({ preventScroll: true }); close(); return; }
      // A menu drawn as a row (aria-orientation="horizontal") moves with Left and Right.
      const row = current.getAttribute("aria-orientation") === "horizontal", next = row ? "ArrowRight" : "ArrowDown", prev = row ? "ArrowLeft" : "ArrowUp";
      if (![next, prev, "Home", "End"].includes(e.key)) return;
      e.preventDefault(); const list = items(), n = list.indexOf(document.activeElement);
      const at = e.key === "Home" ? 0 : e.key === "End" ? list.length - 1 : (n + (e.key === next ? 1 : -1) + list.length) % list.length;
      list[at]?.focus({ preventScroll: true });
    });
    document.addEventListener("scroll", e => {
      if(!current || current.contains(e.target))return;
      const now=trigger.getBoundingClientRect();
      if(!anchorPosition || Math.abs(now.left-anchorPosition.left)>1 || Math.abs(now.top-anchorPosition.top)>1)close();
    }, true);
    window.addEventListener("resize", () => close(true));
  }
  function prune() {
    if (current && (!current.isConnected || !trigger?.isConnected)) {
      close();
      $("#conv-head h1")?.focus({ preventScroll: true });
    }
  }
  function capture(root) {
    return current && root.contains(current) ? {id:current.id, action:document.activeElement?.dataset?.msgAct, focused:current.contains(document.activeElement)} : null;
  }
  function restore(state) {
    if (!state) return;
    close();
    const button = $$("[data-menu]").find(b => b.dataset.menu === state.id);
    if (button) { open(button,state.focused); if (state.focused && state.action) items().find(b => b.dataset.msgAct === state.action)?.focus({preventScroll:true}); }
  }
  return { install, close, prune, capture, restore };
})();
const Tooltips = (() => {
  let trigger = null, tip = null, showTimer, closeTimer, hovered = false, overTip = false, focused = false, suppressed = null, warm = 0;
  const eligible = node => node instanceof Element ? node.closest("[data-tooltip]") : null;
  const label = el => el.dataset.tooltip || el.getAttribute("aria-label") || el.textContent.trim();
  function hide() {
    clearTimeout(showTimer); clearTimeout(closeTimer); showTimer=null;closeTimer=null;
    if (tip) { const ids = (trigger?.getAttribute("aria-describedby") || "").split(/\s+/).filter(x => x && x !== tip.id); if (ids.length) trigger?.setAttribute("aria-describedby", ids.join(" ")); else trigger?.removeAttribute("aria-describedby"); tip.remove(); tip = null; }
  }
  function place() {
    if (!tip || !trigger?.isConnected) return hide();
    const r = trigger.getBoundingClientRect(), v = window.visualViewport, x = v?.offsetLeft || 0, y = v?.offsetTop || 0, w = v?.width || innerWidth, h = v?.height || innerHeight;
    tip.style.maxWidth = `${Math.min(280, w - 16)}px`;
    const t = tip.getBoundingClientRect();
    // The navigation's next item sits just below, and a label held open over
    // it took its click: there the label goes beside.
    if (trigger.closest("nav.side")) {
      tip.style.left = `${Math.min(r.right + 8, x + w - t.width - 8)}px`;
      tip.style.top = `${Math.max(y + 8, Math.min(r.top + (r.height - t.height) / 2, y + h - t.height - 8))}px`;
      return;
    }
    tip.style.left = `${Math.max(x + 8, Math.min(r.left + (r.width - t.width) / 2, x + w - t.width - 8))}px`;
    const top = r.bottom + 8 + t.height <= y + h - 8 ? r.bottom + 8 : r.top - t.height - 8;
    tip.style.top = `${Math.max(y + 8, Math.min(top, y + h - t.height - 8))}px`;
  }
  function show() {
    showTimer=null;
    if (!trigger?.isConnected || suppressed === trigger || !(hovered || focused)) return;
    tip = document.createElement("div"); tip.id = "magnus-tooltip"; tip.className = "tooltip"; tip.role = "tooltip"; tip.textContent = label(trigger);
    document.body.append(tip); trigger.setAttribute("aria-describedby", [trigger.getAttribute("aria-describedby"), tip.id].filter(Boolean).join(" "));
    tip.onpointerenter = () => { overTip = true; clearTimeout(closeTimer); };
    tip.onpointerleave = () => { overTip = false; scheduleClose(); };
    place(); warm = Date.now() + 1000;
  }
  function scheduleClose() {
    clearTimeout(closeTimer);
    if (!hovered && !focused && !overTip) closeTimer = setTimeout(() => { hide(); if (suppressed === trigger) suppressed = null; }, 150);
  }
  function enter(el, focus, touch = false) {
    if (!el || touch) return;
    if (el !== trigger) { hide(); trigger = el; hovered = false; overTip = false; focused = false; }
    if (focus) focused = true; else hovered = true;
    clearTimeout(closeTimer); clearTimeout(showTimer);
    if (!tip && suppressed !== el) showTimer = setTimeout(show, focus || warm > Date.now() ? 80 : 350);
  }
  function dismiss() { if (trigger && (hovered || focused)) suppressed = trigger; hide(); }
  function decorate(root) {
    for (const el of [root, ...root.querySelectorAll("[title]")]) {
      if (!el.hasAttribute?.("title")) continue;
      const title = el.getAttribute("title"); if (title) { el.dataset.tooltip = title; if (el.matches("button, a") && !el.getAttribute("aria-label") && !el.textContent.trim()) el.setAttribute("aria-label", title); }
      el.removeAttribute("title");
    }
  }
  function install() {
    decorate(document.body);
    document.addEventListener("pointerover", e => { const el = eligible(e.target); if (el && !el.contains(e.relatedTarget)) enter(el, false, e.pointerType === "touch"); });
    document.addEventListener("pointerout", e => { if (trigger?.contains(e.target) && !trigger.contains(e.relatedTarget)) { hovered = false; if (!focused && suppressed === trigger) suppressed = null; scheduleClose(); } });
    document.addEventListener("focusin", e => enter(eligible(e.target), true));
    document.addEventListener("focusout", e => { if (trigger?.contains(e.target) && !trigger.contains(e.relatedTarget)) { focused = false; if (!hovered && suppressed === trigger) suppressed = null; scheduleClose(); } });
    document.addEventListener("click", dismiss, true);
    document.addEventListener("keydown", e => { if (e.key === "Escape" && (tip || showTimer)) dismiss(); });
    document.addEventListener("scroll", dismiss, true); window.addEventListener("resize", place); window.visualViewport?.addEventListener("resize", place);
    new MutationObserver(records => {
      for (const r of records) { if (r.type === "attributes") decorate(r.target); for (const n of r.addedNodes || []) if (n.nodeType === 1) decorate(n); }
      if (trigger && !trigger.isConnected) { hide(); trigger = null; }
    }).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["title"] });
  }
  return { install, dismiss };
})();
// What works wherever it is shown, set once for the page.
function wireDocument() {
  document.addEventListener("toggle",reportOpenedSoon,true);
  document.addEventListener("scroll",reportOpenedSoon,{capture:true,passive:true});
  window.addEventListener("popstate",()=>{
    if (App.currentHash!==location.hash || !App.workspace) return;
    if (history.state?.work) openPanel(history.state.work,{focus:true}); else closePanel(false);
  });
  document.addEventListener("click", e => { const b=e.target.closest("[data-attach-open]"); if (b) b.closest(".attachment-picker").querySelector("input").click(); });
  Menus.install(); Tooltips.install(); wireMarkdown(document);
  const style = document.createElement("style"); style.textContent = Compose.markdownStyles; document.head.append(style);
  document.addEventListener("load", e => { if (e.target.matches?.("iframe.composed")) { try { wireMarkdown(e.target.contentDocument); sizeComposedFrame(e.target); } catch { /* frame unavailable */ } } }, true);
  let uiBlocksQueued = 0;
  const syncDocumentWidgets = () => {
    Menus.prune();
    for (const [frame, sizing] of frameSizing) if (!frame.isConnected) { sizing.observer.disconnect(); frame._frameActions?.dispose(); delete frame._frameActions; frameSizing.delete(frame); }
    if (!uiBlocksQueued) uiBlocksQueued = requestAnimationFrame(() => { uiBlocksQueued = 0; drawUiBlocks(document); });
  };
  window.addEventListener("magnus-render", syncDocumentWidgets);
  if (typeof AuroraCore !== "undefined") AuroraCore.onDom(syncDocumentWidgets);
  // Anything that names an item opens it beside the conversation, without
  // leaving it.
  document.addEventListener("click", e => {
    const o = e.target.closest("[data-open]");
    if (!o || e.button !== 0 || e.ctrlKey || e.metaKey) return;
    e.preventDefault();
    // From Settings, Trash or a developer view: back to the conversation,
    // with it open beside.
    Panel.opener=o; Panel.focusNext=true;
    if (!App.workspace) { App.openAfter = o.dataset.open; location.hash = "#/home"; return; }
    openPanel(o.dataset.open,{history:true,focus:true,opener:o});
  });
  document.addEventListener("click", async e => {
    const cs = e.target.closest("[data-conv-share]");
    if (cs && typeof SharedCanvas !== "undefined") { e.preventDefault(); SharedCanvas.openShareDialog({ thread: cs.dataset.convShare, title: cs.dataset.convTitle || "" }); return; }
    const ps = e.target.closest("[data-project-share]");
    if (ps && typeof ProjectShare !== "undefined") { e.preventDefault(); ProjectShare.open(ps.dataset.projectShare || "", ps.dataset.projectName || ""); return; }
    const dm = e.target.closest("[data-start-dm]");
    if (!dm || e.button !== 0 || e.ctrlKey || e.metaKey) return;
    e.preventDefault();
    const [ws, person] = String(dm.dataset.startDm || "").split(":");
    if (!ws || !person) return;
    try {
      const r = await Server.workspaces.direct(ws, { people: [person] });
      if (typeof loadHome === "function") await loadHome(ws);
      location.hash = `#/team/${enc(ws)}/c/${enc(r.id)}`;
    } catch (err) { toast(err.message); }
  });
  // A decision is made from its card wherever the card is shown: in its
  // conversation, beside it, or under Needs you.
  document.addEventListener("click", e => {
    const b = e.target.closest("button[data-item]");
    if (b) respond(b, b.closest("article") || document);
  });
  // A library file downloads from wherever it is listed; a move control
  // moves its item into a project or out, wherever it is shown.
  document.addEventListener("click", async e => {
    const b = e.target.closest("[data-file-download]");
    if (!b) return;
    e.preventDefault();
    try { toast(`Saved ${await Magnus.download(`/v1/files/${enc(b.dataset.fileDownload)}`, b.dataset.name)}`); } catch (err) { toast(err.message); }
  });
  document.addEventListener("click", async e => {
    const b = e.target.closest("[data-verdict], [data-verdict-save]");
    if (!b) return;
    const bar = b.closest("[data-verdict-for]"), id = bar.dataset.verdictFor;
    const verdict = b.dataset.verdict || $("[data-verdict].on", bar)?.dataset.verdict;
    if (!verdict) { toast("Pick right, partly or wrong first."); return; }
    const box = $("textarea", bar);
    try {
      await Server.tasks.verdict(id, verdict, box.value);
      delete drafts[box.dataset.draft];
      $$("[data-verdict]", bar).forEach(x => { const on = x.dataset.verdict === verdict; x.classList.toggle("on", on); x.setAttribute("aria-pressed", on); });
      toast(b.dataset.verdictSave !== undefined ? "Note saved." : "Recorded.");
      await refresh();
    } catch (err) { toast(err.message); }
  });
  // Delete, wherever an item offers it: to the trash, where it can be
  // restored until it is gone for good.
  document.addEventListener("click", async e => {
    const b = e.target.closest("[data-trash]");
    if (!b) return;
    const label = b.dataset.trashLabel || b.textContent.trim() || b.getAttribute("aria-label") || "Delete";
    if (!await ask(`${label}? It moves to the trash, where you can restore it until it is gone for good.`, label)) return;
    b.disabled = true;
    const itemId = b.dataset.trash;
    try {
      await Server.trash.add(itemId);
      toast("Moved to the trash.", 6000, {
        label: "Undo",
        onClick: async () => {
          try { await Server.trash.restore(itemId); toast("Restored."); await refresh(); }
          catch (err) { toast(err.message); }
        },
      });
      if (b.closest("#detail")) {
        if (Panel.what === `file:${itemId}`) await openPanel("files");
        else if (Panel.what === `task:${itemId}`) await openPanel("tasks");
        else if (Panel.what === `project:${itemId}`) await openPanel("projects");
      }
      if (b.dataset.then) location.hash = b.dataset.then; else await refresh();
    } catch (err) { toast(err.message); b.disabled = false; }
  });
  document.addEventListener("change", async e => {
    const sel = e.target.closest("select[data-move]");
    if (!sel) return;
    try {
      await Server.files.move(sel.dataset.move, sel.value || null);
      toast(sel.value ? `Moved to ${sel.selectedOptions[0].textContent}.` : "Taken out of the project.");
      await refresh();
    } catch (err) { toast(err.message); }
  });
}

// A view of the workspace (a conversation) keeps the panel beside it; any
// other view (Settings, Trash, the developer's) has the page to itself.
function route(re, view, workspace = false) { App.routes.push([re, view, workspace]); }

// A Delete button for an item: to the trash, then to `then` (or redrawn).
const trashButton = (item, label, then = "") => `<button data-trash="${esc(item)}"${then ? ` data-then="${esc(then)}"` : ""}>${esc(label)}</button>`;
