// Personal chat, turns, tasks, and home view (extracted from app.js).
"use strict";

// ---------- Chat ----------

// The chat is the one conversation (chat-view.js) with the personal
// chat's source (personal-chat.js).
// A conversation goes on until the person starts a new one; opening Magnus
// returns to the latest.

// Lays out a conversation; its view, or false when the person moved on
// while it loaded.
async function chatLayout(main, thread, project) {
  const seq = App.seq;
  App.thread = thread;
  sidebar();
  paint(main, ChatView.layout({ banners: true }));
  convHead(thread, project);
  banners();
  $("#banners", main).onclick = e => {
    if (e.target.closest("[data-act=seen-versions]")) { save("version", App.versions?.latest_seq ?? 0); banners(); }
    // Dismissing the updates notice, or opening Activity from it, closes it until the next session.
    if (e.target.closest("[data-act=dismiss-updates], [data-open=activity]")) { App.feedDismissed = true; banners(); }
  };
  const record = Conversation.draft({ thread, project });
  const view = await Conversation.open(main, { framed: true, record, conv: personalChat(thread), side: personalComposer(), stale: () => seq !== App.seq,
    placeholder: thread ? "Reply, or give Magnus more to do" : "Ask anything, or give Magnus work to do" });
  if (view) App.draftRecord = record;
  return view || false;
}
// Above the conversation: its project (a link to it) and its title, and
// what can be done to the whole conversation, plus a single toggle for the
// on-demand right sidebar.
function convHead(thread, project) {
  const el = $("#conv-head");
  if (!el) return;
  const here = thread && App.threads.find(t => t.thread === thread);
  const p = here ? here.project : project && App.projects.find(x => x.id === project);
  const ttl = here ? displayTitle(here.title) : "New conversation";
  const projLink = p ? `<a class="conv-proj small muted" href="#/project/${enc(p.id)}">${esc(p.name)}</a><span class="conv-sep" aria-hidden="true">/</span>` : "";
  const move = (App.projects.length > 0 || here?.project) ? moveControl(thread, here?.project, App.projects, true) : "";
  const shareBtn = thread ? `<button type="button" class="shared-promote-btn" data-conv-share="${esc(thread)}" data-conv-title="${esc(ttl)}" title="Share into a shared project (clean-room)">${icon("team")}<span>Share</span></button>` : "";
  const pending = needsMe().length;
  const dnd = typeof Notices !== "undefined" && Notices.isDnd?.();
  const nCount = typeof Notices !== "undefined" && Array.isArray(Notices.history) ? Notices.history.length : 0;
  const noticeBtn = `<div class="notice-center-wrap"><button type="button" class="icon-btn stage-dock-btn${dnd ? " muted" : ""}" data-toggle-notices aria-expanded="false" aria-controls="notice-center" title="${dnd ? "Notifications snoozed (click to open)" : "Notifications & Do Not Disturb"}" aria-label="${dnd ? "Notifications snoozed" : `Notifications${nCount ? `, ${nCount} recent` : ""}`}">${icon("bell")}${!dnd && nCount ? `<span class="stage-dock-badge" aria-hidden="true">${Math.min(nCount, 99)}</span>` : ""}</button><div id="notice-center" class="notice-center card" role="region" aria-label="Notifications" hidden></div></div>`;
  const sidebarBtn = `<button type="button" class="icon-btn stage-dock-btn" data-toggle-inspector aria-pressed="${Panel.what ? "true" : "false"}" title="${Panel.what ? "Close workspace sidebar" : "Open workspace details (Tasks, Routines, Approvals, Files, Apps)"}" aria-label="${Panel.what ? "Close workspace sidebar" : `Open workspace sidebar${pending ? `, ${pending} waiting` : ""}`}">${icon("sidebar")}${pending && !Panel.what && !(typeof Theme !== "undefined" && Theme.has?.("hideDockPendingBadge")) ? `<span class="stage-dock-badge" aria-hidden="true">${pending}</span>` : ""}</button>`;
  const nova = typeof Theme !== "undefined" && Theme.has?.("topBar");
  const chip = computerChip(App.computers);
  const inMenu = nova ? `${chip}${shareBtn}` : "";
  const moreMenu = thread
    ? `<details class="conv-more"><summary class="icon-btn" title="More conversation actions" aria-label="More conversation actions">&bull;&bull;&bull;</summary><div class="conv-more-menu card">${inMenu}${move}<button type="button" class="conv-more-del" data-trash="${esc(thread)}" data-then="#/home">${icon("trash")}<span>Delete conversation</span></button></div></details>`
    : nova && chip ? `<details class="conv-more"><summary class="icon-btn" title="More actions" aria-label="More actions">&bull;&bull;&bull;</summary><div class="conv-more-menu card">${chip}</div></details>` : "";
  paint(el, `<div class="conv-title">${projLink}<h1 title="${esc(ttl)}">${esc(ttl)}</h1></div>
    <div class="row">${nova ? "" : `${chip}${shareBtn}`}${moreMenu}${noticeBtn}${sidebarBtn}</div>`);
}

async function showNewChat(main, project) {
  const view = await chatLayout(main, null, project);
  if (!view) return;
  const record = App.draftRecord;
  paint($("#thread", main), ChatView.listHtml([], view));
  if (App.appSearch) { record.text=App.appSearch;record.selection=[record.text.length,record.text.length];App.appSearch=null;const box=$("#msg",main);if(box){box.value=record.text;box.setSelectionRange(...record.selection);} }
  sidebar();
}

async function showThread(main, thread) {
  const view = await chatLayout(main, thread, null);
  if (!view) return;
  App.redraw = view.draw;
  onLeave(() => { App.redraw = null; view.conv.stop(); hush(); });
  await view.draw();
}

// Open a conversation in place, with `panel` beside it. Replacing the
// window address reloads this app on every entry.
function showChat(main, hash, view, panel, panelOptions) {
  if (location.hash !== hash) history.replaceState({...history.state}, "", hash);
  if (panel) openPanel(panel, panelOptions);
  return view(main);
}

// Home is the latest conversation (a new one when there is none). The right
// sidebar stays closed until the user explicitly opens it to look up current
// or past details.
async function home(main, panel = App.openAfter) {
  App.openAfter = null;
  const seq = App.seq;
  const threads = (await Server.chat.threads()).threads;
  App.threads = threads;
  if (seq !== App.seq) return;
  if (threads.length) {
    const id = threads[0].thread;
    return showChat(main, `#/chat/${enc(id)}`, m => showThread(m, id), panel);
  }
  return showChat(main, "#/chat/new", m => showNewChat(m), panel);
}

route(/^#\/(?:home|chat)?$/, main => home(main), true);
// Draft links from the earlier durable-draft build open a new conversation.
route(/^#\/draft\/([^/]+)$/, () => { location.replace("#/chat/new"); }, true);
route(/^#\/chat\/new$/, m => showNewChat(m), true);
route(/^#\/chat\/new\/([^/]+)$/, (m, project) => showNewChat(m, project), true);
// Where the views over tasks, schedules, files and decisions used to be
// pages: each opens beside the latest conversation.
route(/^#\/inbox$/, main => home(main, "needs"), true);
route(/^#\/(activity|tasks|schedules|files)$/, (main, k) => home(main, k), true);
route(/^#\/projects$/, main => Projects.showList(main));
route(/^#\/briefing$/, main => NovaPages.showBriefing(main));
route(/^#\/approve(?:\?(.*))?$/, (main, qs) => MobileApprove.show(main, qs));
// The Library is the connector catalog, a page of its own (connectors spec,
// Library); a connector's page is under it.
route(/^#\/explore$/, main => Discover.show(main));
route(/^#\/library(?:\/([^/]+))?$/, (main, name) => showLibrary(main, name));
route(/^#\/new$/, main => home(main), true);
route(/^#\/file\/([^/]+)$/, (main, id) => home(main, `file:${id}`), true);
route(/^#\/project\/([^/]+)$/, (main, id) => Projects.show(main, id));
// A task opens beside the conversation it came from.
route(/^#\/task\/([^/]+)$/, async (main, id) => {
  const seq = App.seq;
  const t = await Server.tasks.get(id);
  if (seq !== App.seq) return;
  if (!t.thread) return home(main, `task:${id}`);
  return showChat(main, `#/chat/${enc(t.thread)}`, m => showThread(m, t.thread), `task:${id}`);
}, true);

function rule(e) {
  const m = e.every_minutes;
  if (m) {
    for (const [size, one, many] of [[10080, "week", "weeks"], [1440, "day", "days"], [60, "hour", "hours"]]) {
      if (m % size === 0) return m === size ? `Every ${one}` : `Every ${m / size} ${many}`;
    }
    return m === 1 ? "Every minute" : `Every ${m} minutes`;
  }
  if (e.daily_at) {
    const raw = String(e.daily_at).trim();
    const wd = raw.match(/^weekdays\s+(\d{1,2}:\d{2})$/i);
    if (wd) return `Weekdays at ${wd[1]}`;
    const wk = raw.match(/^weekly\s+([a-z,]+)\s+(\d{1,2}:\d{2})$/i);
    if (wk) {
      const days = wk[1].split(",").map(d => d.trim().slice(0, 3)).filter(Boolean).map(d => d[0].toUpperCase() + d.slice(1).toLowerCase()).join(", ");
      return `Weekly (${days}) at ${wk[2]}`;
    }
    const mo = raw.match(/^monthly\s+(\d{1,2})\s+(\d{1,2}:\d{2})$/i);
    if (mo) return `Monthly on day ${mo[1]} at ${mo[2]}`;
    const once = raw.match(/^once\s+(.+)$/i);
    if (once) return `Once on ${once[1].replace("T", " ")}`;
    return `Every day at ${raw}`;
  }
  return "";
}
// The decision an entry waits on, as its card, where the person can make it:
// the same card as under Needs you and beside the conversation.
function waitingCard(id, ui) {
  const i = id && App.inbox.find(x => x.id === id && x.status === "open");
  return i ? card(i, ui.composed || []) : "";
}
// When a schedule stops: a time, a number of runs, both, or never.
function ends(e) {
  const parts = [];
  if (e.until) parts.push(`no run after ${new Date(e.until).toLocaleString()}`);
  if (e.max_runs) parts.push(`at most ${e.max_runs} run${e.max_runs === 1 ? "" : "s"}`);
  return parts.length ? parts.join(", ") : "until you stop it";
}
// Where a task stands, as the design draws it: an icon and a pill. A task
// waiting on a decision the person can make needs them.
const needsYou = t => !!t.waiting_item && needsMe().some(i => i.id === t.waiting_item);
function standing(t) {
  if (needsYou(t)) return ["need", `<span class="chip warn">Needs you</span>`];
  return [{ completed: "done", running: "working", failed: "stopped", cancelled: "stopped" }[t.status] || "working", t.status === "completed" ? "" : chip(t.status)];
}
// A task in its conversation: one row that opens it beside the thread, its
// status live; what it waits on from the person follows the request's rows.
function taskCard(t) {
  const why = t.status === "suspended" ? esc(WAITING[t.waiting_on] || "") : t.status === "running" ? statusLineSlot(t, "sub why") : "";
  const [ic, pill] = standing(t);
  return ChatView.workRow({ id: t.task_id, status: t.status, icon: ic, pill, title: t.title, tip: t.text,
    sub: t.status === "running" ? why : why ? `<span class="sub why">${why}</span>` : `<span class="sub">${esc(firstLine(t.text))}</span>`,
    href: `#/task/${enc(t.task_id)}`, open: `task:${t.task_id}`, opened: Panel.what === `task:${t.task_id}`, stop: !ENDED.includes(t.status) });
}
// A schedule where it was confirmed: a row that opens it, with when it runs
// next and how its last run went.
function scheduleText(sc) {
  const last = sc.last;
  return `${sc.ended ? `Ended: ${sc.ended}` : sc.paused ? `Paused: ${sc.paused}` : `Next run ${until(sc.next)}`} · ${sc.last_failed ? `Last firing could not start: ${sc.last_failed.error}` : last ? `Last result: ${(STATUS[last.status] || ["", last.status])[1]}, ${when(last.ended_at || last.created_at)}` : "No results yet"}`;
}
// A card's answers: accept (one tap, which grants the work), Not now, or
// Decline, which asks for the reason. A grant card, or one already put off,
// is not put off (`later` false); one put off is deleted, as a decline.
function cardActs(e, ui, accept, later = true, decline = "Decline") {
  if (ui.declining === e.card) return `<div class="row cc-act decline-why"><label class="sr" for="why-${esc(e.card)}">Why not?</label>
    <input id="why-${esc(e.card)}" data-draft="why-${esc(e.card)}" placeholder="Why not? Magnus reads this before suggesting again">
    <button class="primary" data-decide="decline">${esc(decline)}</button><button class="link" data-decide="decline-cancel">Cancel</button></div>`;
  return `<div class="row cc-act"><button class="primary" data-decide="confirm">${esc(accept)}</button>${later ? `<button data-decide="later">Not now</button>` : ""}<button class="link" data-decide="decline-open">${esc(decline)}</button></div>`;
}

// A grant's fields as a form (#235): the kind of work, the apps it covers,
// what may go ahead without asking besides, and its budget of model cost
// per period. `k` keys its drafts; `f` holds the values it starts from;
// `apps` are the person's apps to choose from.
function grantForm(k, f, apps) {
  const id = n => `g-${esc(k)}-${n}`, d = n => `data-draft="${id(n)}"`;
  const lifted = x => (f.allow || []).includes(x) ? "checked" : "";
  const choices = apps?.length ? apps : (f.apps || []).map(name => ({ name, title: name }));
  return `<div class="grant-form" data-grant-form>
    <label for="${id("title")}" class="small muted">What kind of work</label><input id="${id("title")}" ${d("title")} data-g="title" value="${esc(f.title || "")}" style="width:100%">
    <div class="sugg-part"><span class="small muted">In these apps</span>${choices.map(a => `<label class="row toggle small"><input type="checkbox" data-g-app="${esc(a.name)}" ${d(`app-${a.name}`)} ${(f.apps || []).includes(a.name) ? "checked" : ""}><span>${esc(a.title || a.name)}</span></label>`).join("")}</div>
    <div class="sugg-part"><span class="small muted">Without asking, Magnus may also</span>
      <label class="row toggle small"><input type="checkbox" data-g-allow="send" ${d("send")} ${lifted("send")}><span>Send on my behalf</span></label>
      <label class="row toggle small"><input type="checkbox" data-g-allow="irreversible" ${d("irreversible")} ${lifted("irreversible")}><span>Delete or commit me to anything</span></label>
      <p class="small faint">Spending money always asks.</p></div>
    <div class="row"><label for="${id("max")}" class="small muted">Budget</label><span class="small">$</span><input id="${id("max")}" type="number" min="0" step="0.5" data-g="max_usd" ${d("max")} value="${esc(f.max_usd ?? "")}" style="width:6em">
      <select aria-label="Per" data-g="period" ${d("period")}>${["day", "week", "month"].map(p => `<option value="${p}" ${(f.period || "week") === p ? "selected" : ""}>a ${p}</option>`).join("")}</select><span class="small muted">of model cost</span></div>
  </div>`;
}

// The grant a form holds, as the appliance takes it.
function readGrantForm(root) {
  const f = $("[data-grant-form]", root);
  return {
    title: $("[data-g=title]", f).value.trim(), period: $("[data-g=period]", f).value, max_usd: Number($("[data-g=max_usd]", f).value),
    apps: $$("[data-g-app]", f).filter(i => i.checked).map(i => i.dataset.gApp), allow: $$("[data-g-allow]", f).filter(i => i.checked).map(i => i.dataset.gAllow),
  };
}

// What a card rests on: each item, and the tool or task it points at.
function evidenceList(e, label) {
  return `<div class="sugg-part"><span class="small muted">${label}</span><ul class="sugg-list">${(e.evidence || []).map(x => `<li>${esc(x.shows)} <span class="small muted">(${x.call ? `from ${esc(x.tool)}` : `task: ${esc(x.title || "your request")}`})</span></li>`).join("")}</ul></div>`;
}

// What became of a card that is no longer open, other than a schedule it made.
function cardOutcome(e) {
  if (e.state === "declined") return `<p class="small"><span class="chip">Declined</span> ${esc(e.title || "")}${e.reason ? ` <span class="muted">· ${esc(e.reason)}</span>` : ""}</p>`;
  if (e.state === "later") return `<p class="small"><span class="chip">Not now</span> ${esc(e.title || "")}</p>`;
  if (e.started) return `<p class="small"><span class="chip ok">Accepted</span> ${esc(e.title || "")} <a href="#/task/${enc(e.started)}" data-open="task:${esc(e.started)}">See the task</a></p>`;
  if (e.kind === "grant_card" && e.granted) return `<p class="small"><span class="chip ok">Granted</span> ${esc(e.title || "")} <a href="#/settings/data">See it in Settings</a></p>`;
  return "";
}

function scheduleLine(e) {
  if (e.state !== "confirmed") return cardOutcome(e);
  const sc = App.schedules.find(x => x.id === e.schedule);
  if (!sc) return `<p class="small" data-schedule-gone="${esc(e.schedule || "")}"><span class="chip">Details unavailable</span> ${esc(e.title || "This schedule")} · Schedule details are not currently available.</p>`;
  const act = (k, label) => `<button class="link small" data-sched-act="${k}" data-schedule-id="${esc(sc.id)}">${label}</button>`;
  const chip = sc.ended ? `<span class="chip">Ended</span>` : `<span class="chip sch">${sc.paused ? "Paused" : "Scheduled"}</span>`;
  const acts = sc.ended ? act("cancel", "Delete") : `${sc.paused ? act("resume", "Resume") : act("pause", "Pause")}${act("stop", "Stop")}${act("cancel", "Delete")}`;
  return `<div class="reqgroup sched-state" data-schedule="${esc(sc.id)}"><a class="rg-row" href="#/schedules" data-open="schedule:${esc(sc.id)}" title="Open the schedule">
    <span class="st sch">${icon("calendar")}</span><span class="rg-main"><span class="ttl">${esc(sc.title)}</span><span class="sub" data-last>${esc(scheduleText(sc))}</span></span>${chip}${icon("chev", "chev")}</a>
    <div class="card-acts">${acts}<a class="small" href="#/schedules" data-open="schedule:${esc(sc.id)}">Open</a>${sc.started ? `<a class="small" href="#/chat/${enc(`schedule-${sc.id}`)}" data-schedule-runs>Its runs</a>` : ""}</div></div>`;
}

// Motion (U7): what is new in the thread rises into place, a task whose
// status changed shows its new status gently, and finished work gets a quiet
// moment. A repaint draws every node afresh, so only what changed is marked;
// the stylesheet decides, by the motion setting, how each mark moves, if at
// all.
function motion(el, ui) {
  const first = !ui.seen;
  ui.seen ||= new Set();
  ui.status ||= {};
  for (const n of $$(".entry[data-entry]", el)) {
    const k = n.dataset.entry;
    if (!first && k && !ui.seen.has(k)) {
      n.classList.add("enter");
      n.addEventListener("animationend",()=>n.classList.remove("enter"),{once:true});
    }
    ui.seen.add(k);
  }
  for (const n of $$("[data-chat-task]", el)) {
    const k = n.dataset.chatTask, st = n.dataset.status, was = ui.status[k];
    if (was && was !== st) n.classList.add(st === "completed" ? "finished" : "changed");
    ui.status[k] = st;
  }
}

// The clipboard where the page may use it; the old way where it may not.
async function copyText(t) {
  try { await navigator.clipboard.writeText(t); return; } catch { /* fall back */ }
  const a = document.createElement("textarea");
  a.value = t; a.style.position = "fixed"; a.style.opacity = "0"; document.body.append(a); a.select();
  const ok = document.execCommand("copy"); a.remove();
  if (!ok) throw new Error("This app may not use the clipboard.");
}

// What a message says, as text to copy.
const entryText = e => e.kind === "result" ? `${e.title} (${e.status})\n\n${e.summary || ""}`
  : e.kind === "tasks" ? e.tasks.map(t => `- ${t.title}: ${t.text}`).join("\n") : e.text || "";
const safeName = e => `${(entryText(e).split(/\s+/).slice(0, 6).join(" ").replace(/[^\p{L}\p{N} -]/gu, " ").trim().replace(/\s+/g, "-") || "message")}.md`;

// Icons for what can be done to a chat entry: the shapes inside each 24px
// outline icon from Lucide (lucide-static 0.563.0), under this licence:
// ISC License
//
// Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2026 as part of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2026.
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
//
// ---
//
// The MIT License (MIT) (for portions derived from Feather)
//
// Copyright (c) 2013-2026 Cole Bemis
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
const ICONS = {
  copy: '<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>', // copy
  edit: '<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/><path d="m15 5 4 4"/>', // pencil
  retry: '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>', // rotate-ccw
  listen: '<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z"/><path d="M16 9a5 5 0 0 1 0 6"/><path d="M19.364 18.364a9 9 0 0 0 0-12.728"/>', // volume-2
  hush: '<circle cx="12" cy="12" r="10"/><rect x="9" y="9" width="6" height="6" rx="1"/>', // circle-stop
  branch: '<line x1="6" x2="6" y1="3" y2="15"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/>', // git-branch
  share: '<circle cx="18" cy="5" r="3"/><circle cx="6" cy="12" r="3"/><circle cx="18" cy="19" r="3"/><line x1="8.59" x2="15.42" y1="13.51" y2="17.49"/><line x1="15.41" x2="8.59" y1="6.51" y2="10.49"/>', // share-2
  export: '<path d="M12 15V3"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/>', // download
  save: '<path d="M12 10v6"/><path d="M9 13h6"/><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>', // folder-plus
  task: '<path d="M16 5H3"/><path d="M11 12H3"/><path d="M16 19H3"/><path d="M18 9v6"/><path d="M21 12h-6"/>', // list-plus
  great: '<circle cx="12" cy="12" r="10"/><path d="M8 14s1.5 2 4 2 4-2 4-2"/><line x1="9" x2="9.01" y1="9" y2="9"/><line x1="15" x2="15.01" y1="9" y2="9"/>', // smile
  reply: '<polyline points="9 17 4 12 9 7"/><path d="M20 18v-2a4 4 0 0 0-4-4H4"/>',
  annoying: '<circle cx="12" cy="12" r="10"/><path d="M16 16s-1.5-2-4-2-4 2-4 2"/><line x1="9" x2="9.01" y1="9" y2="9"/><line x1="15" x2="15.01" y1="9" y2="9"/>', // frown
  delete: '<path d="M10 11v6"/><path d="M14 11v6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M3 6h18"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>', // trash-2
};

// What only the personal chat draws: the chat view (chat-view.js) draws a
// message and an answer itself, for every conversation.
function entry(e, ui = {}) {
  switch (e.kind) {
    case "opener": return `<div class="msg magnus" data-opener><div class="small faint">Magnus · ${esc(when(e.at))}</div><div class="small muted">${e.app_title ? `You connected ${esc(e.app_title)}, so I had a look at what I could take over there.` : "You are new here, so I had a look at what I could take over for you."}</div></div>`;
    case "turn": return e.status === "failed" || e.status === "cancelled"
      ? `<div class="msg magnus"><div class="small bad">Could not answer: ${esc(e.text || e.status)}</div></div>`
      : `<div class="msg magnus"><div class="small faint">${e.status === "running" ? "Working on an answer…" : esc(WAITING[e.waiting_on] || "paused")}</div>${typeof PlanSurface !== "undefined" ? PlanSurface.renderLiveCard(e.task_id) : ""}${e.status === "running" ? `<div class="pre" data-live="${esc(e.task_id)}"></div>` : ""}</div>${waitingCard(e.waiting_item, ui)}`;
    case "tasks": {
      const done = e.tasks.filter(t => t.status === "completed").length, need = e.tasks.filter(needsYou).length;
      const showHead = e.tasks.length > 1 || need > 0;
      return `<div class="reqgroup chatcard" data-chat-tasks="${esc(e.from_run)}">${showHead ? `<div class="rg-head"><span>From this request</span><span class="small muted num">${done} of ${e.tasks.length} done${need ? ` · ${need} need${need === 1 ? "s" : ""} you` : ""}</span></div>` : ""}
        <ul class="rg-rows">${e.tasks.map(taskCard).join("")}</ul></div>
        ${e.tasks.map(t => t.waiting_item && waitingCard(t.waiting_item, ui) ? `<div class="decide" data-for-task="${esc(t.task_id)}">${waitingCard(t.waiting_item, ui)}</div>` : "").join("")}`;
    }
    case "sub_agents": return subAgentsCard(e, ui);
    case "result": return `<div class="result" data-result="${esc(e.task_id)}">${ui.movedOn?.has(e.task_id) ? `<blockquote class="asked small"><span class="faint">You asked:</span> ${esc(firstLine(e.text))}</blockquote>` : ""}<details open data-key="result:${esc(e.task_id)}"><summary><span class="ttl">${esc(e.title)}</span> <span class="muted">${esc(firstLine(e.summary))}</span><a class="small see" href="#/task/${enc(e.task_id)}" data-open="task:${esc(e.task_id)}">Outcome</a></summary>
      <div class="pre">${esc(e.summary || "")}</div>${e.view ? shownFrame(e.id, "The deliverable") : ""}${e.files?.length ? `<div class="previews">${e.files.map(f => filePreview(f, livePages(e))).join("")}</div>` : ""}${evidenceSection(e)}</details></div>`;
    case "connector_card": return appOfferCard(e);
    case "suggestion_card": {
      // Put off ("Not now"), it is kept, to accept or delete later.
      const later = e.state === "later";
      if (e.state !== "open" && !later) return `<div data-card="${esc(e.card)}">${e.schedule ? scheduleLine(e) : cardOutcome(e)}</div>`;
      const timed = e.every_minutes || e.daily_at;
      return `<div class="confirm chatcard suggestion" data-card="${esc(e.card)}"${later ? " data-later" : ""}><div class="cc-head"><span class="st sch">${icon(timed ? "calendar" : "tasks")}</span><strong>${esc(e.title)}</strong>${later ? ` <span class="chip">Not now</span>` : ""}</div>
        <p class="cc-text">${esc(e.plan)}</p>
        ${e.changed ? `<p class="small"><strong>What changed:</strong> ${esc(e.changed)}</p>` : ""}
        ${evidenceList(e, "Why")}
        <div class="sugg-part"><span class="small muted">What it may do</span><ul class="sugg-list">${(e.permissions || []).map(x => `<li>${esc(x)}</li>`).join("")}</ul></div>
        ${timed ? `<div class="tiles"><div><span>When</span><strong>${esc(rule(e))}</strong></div><div><span>Ends</span><strong>${esc(ends(e))}</strong></div></div>` : ""}
        <p class="small muted">It acts for you (${esc(e.acts_for)}), with your access; what it needs from you shows under Needs you.</p>${taintNote(e.taint)}
        ${cardActs(e, ui, "Accept", !later, later ? "Delete" : "Decline")}</div>`;
    }
    case "grant_card": {
      if (e.state !== "open") return `<div data-card="${esc(e.card)}">${cardOutcome(e)}</div>`;
      return `<div class="confirm chatcard suggestion grant" data-card="${esc(e.card)}"><div class="cc-head"><span class="st sch">${icon("tasks")}</span><strong>Let Magnus handle this on its own?</strong></div>
        <p class="cc-text">${esc(e.why)}</p>
        ${grantForm(e.card, e, e.your_apps)}
        <p class="small muted">Tasks Magnus starts for this go ahead without asking, as you (${esc(e.acts_for)}), within these limits and your organisation's policy; it reports every action. Change or revoke it in Settings.</p>${taintNote(e.taint)}
        ${cardActs(e, ui, "Accept", false)}</div>`;
    }
    // A schedule set up at the person's request: shown as set, with no confirmation (spec rev 140).
    case "schedule_card": return `<div data-card="${esc(e.card)}">${scheduleLine(e)}</div>`;
    default: return "";
  }
}

route(/^#\/chat\/([^/]+)$/, async (main, thread) => {
  if (thread === "new") return showNewChat(main);
  return showThread(main, thread);
}, true);
