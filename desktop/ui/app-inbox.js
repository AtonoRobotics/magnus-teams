// Inbox, approval cards, respond(), and activity views (extracted from app.js).
"use strict";

// ---------- Inbox ----------

const KIND = {
  approval: "Approval", question: "Question", review: "Review", notice: "Stopped", message: "Note from the agent", observation: "Observation", interrupt: "Interruption",
  policy_proposal: "Policy proposal", suggestion: "Suggestion", app_tools: "App changed",
  monitor: "Safety monitor", secure_form: "Secure sign-in", takeover_request: "Take over", payment: "Purchase", contract_change: "Contract change", deletion_request: "Deletion request", shared: "Shared with you", endorsement: "Company fact?",
  outside_request: "Request from an outside agent", outside_reply: "Reply to an outside agent",
};
const TIER = { read: "reads", sandbox: "workspace only", reversible_write: "can be undone", irreversible: "cannot be undone", financial: "money", prohibited: "prohibited" };
const AUDIENCE = { private: "only you", org: "people in your organization", public: "anyone" };

function fieldValue(v) {
  if (v === null || v === undefined) return `<span class="faint">none</span>`;
  if (typeof v === "string") return `<pre>${esc(v)}</pre>`;
  if (typeof v === "number" || typeof v === "boolean") return `<pre>${esc(String(v))}</pre>`;
  return `<pre>${esc(JSON.stringify(v, null, 2))}</pre>`;
}

function taintNote(t) {
  if (!t || (!t.untrusted && !t.private)) return "";
  const parts = [];
  if (t.untrusted) parts.push("has read content from outside sources, which could be steering it");
  if (t.private) parts.push("has read private data");
  return `<p class="small muted">This task ${parts.join(" and ")}.</p>`;
}

function buttons(i, list) {
  if (!i.can_respond) return `<p class="small faint">Waiting for someone with the right role to decide.</p>`;
  return `<div class="row" style="margin-top:12px">${list.map(([d, label, cls, extra]) => `<button class="${cls || ""}" data-item="${esc(i.id)}" data-decision="${d}" ${extra || ""}>${esc(label)}</button>`).join("")}</div>`;
}

function card(i, composed) {
  const d = i.details || {};
  const tier = i.kind === "approval" ? (i.tier || d.risk) : "";
  const head = `<div class="spread"><span class="kind">${esc(KIND[i.kind] || i.kind)}${tier ? ` · ${esc(TIER[tier] || tier)}` : ""}</span>
    <span class="small faint">${i.blocking ? `<strong style="color:var(--fg)">A task is waiting</strong> · ` : ""}${esc(when(i.created_at))}${i.task_id && i.person === App.me?.id ? ` · <a href="#/task/${enc(i.task_id)}" data-open="task:${esc(i.task_id)}">task</a>` : ""}</span></div>`;
  let body = "";
  if (i.status !== "open") {
    const r = i.resolution || {};
    if (i.kind === "approval") return `<div class="card flat item approval">${head}${askResolved(i)}</div>`;
    return `<div class="card flat item ${esc(i.kind)} ${esc(tier)}">${head}<h4>${esc(i.summary)}</h4><p class="small muted">${esc(r.decision)}${r.scope ? ` (${esc(r.scope)})` : ""} by ${esc(String(r.by || "").replace(/^person:/, ""))}${r.text ? `: ${esc(r.text)}` : ""}</p></div>`;
  }
  switch (i.kind) {
    // An ask: the server's one sentence, the content one tap away, and
    // its answers (asks.js). In the conversation the work came from too.
    case "approval":
      return `<article class="card item approval" data-id="${esc(i.id)}">${head}${i.app ? appOfferCard(i.app, i.person === App.me?.id) : ""}${askCard(i, i.can_respond)}</article>`;
    case "question": {
      const opts = Array.isArray(d.options) ? d.options : [];
      body = `${taintNote(d.taint)}${i.can_respond ? `${opts.length ? `<div class="row">${opts.map(o => `<button data-item="${esc(i.id)}" data-decision="answer" data-text="${esc(o)}">${esc(o)}</button>`).join("")}</div>` : ""}
        <textarea rows="2" data-draft="ans-${esc(i.id)}" placeholder="Your answer" style="margin-top:10px"></textarea>
        <div class="row" style="margin-top:8px"><button class="primary" data-item="${esc(i.id)}" data-decision="answer">Send answer</button></div>` : buttons(i, [])}`;
      break;
    }
    case "notice": {
      // Ideas waiting in Explore: where they are, nothing stopped.
      if (ExploreIdeas.isNotice(i)) {
        body = `<p class="small"><a href="#/explore">Open Explore</a> to start or dismiss them.</p>${buttons(i, [["dismiss", "Dismiss"]])}`;
        break;
      }
      const ev = d.evidence;
      const list = [];
      // A schedule's own notice stands for no run to continue: its Resume does that.
      if (typeof d.reason === "string" && !d.reason.startsWith("schedule_") && i.run_id) list.push(["resume", "Continue", "primary"]);
      if (d.reason === "budget") list.push(["extend", "Double the budget and continue"]);
      const askMore = typeof spendNoticeAction === "function" ? spendNoticeAction(d) : null;
      if (askMore) list.push(askMore);
      list.push(["dismiss", "Dismiss"]);
      body = `<p class="small"><span class="muted">Why it stopped:</span> ${esc(d.reason)}</p>
        ${ev ? `<div class="evidence fail"><div class="small muted">Evidence</div>${fieldValue(ev)}</div>` : ""}${buttons(i, list)}`;
      break;
    }
    // Magnus's own idea from a task or a review: one tap starts it (spec rev
    // 140). Proposals from before then read as suggestions.
    case "suggestion": {
      const timed = d.every_minutes || d.daily_at;
      body = `<div class="what"><div class="field"><span>${timed ? "Each run" : "Task"}</span><span>${esc(d.plan || d.text)}</span></div>
        ${timed ? `<div class="field"><span>When</span><span>${esc(rule(d))}</span></div><div class="field"><span>Ends</span><span>${esc(ends(d))}</span></div>` : ""}
        ${(d.permissions || d.needs?.permissions || []).length ? `<div class="field"><span>May</span><span>${esc((d.permissions || d.needs?.permissions).join("; "))}</span></div>` : ""}</div>
        ${(d.evidence || []).length ? `<div class="evidence"><div class="small muted">Why</div><ul class="small">${(d.evidence || []).map(c => `<li>${esc(c.title || c.tool || c.call || "")}: “${esc(c.shows)}”</li>`).join("")}</ul></div>` : ""}${taintNote(d.taint)}
        ${buttons(i, [["accept", timed ? "Schedule it" : "Start it", "primary"], ["reject", "Dismiss"]])}`;
      break;
    }
    case "message":
      body = `${taintNote(d.taint)}${buttons(i, [["dismiss", "Dismiss"]])}`;
      break;
    case "observation":
    case "interrupt":
      body = `${d.why ? `<p>${esc(d.why)}</p>` : ""}${d.evidence ? `<div class="evidence"><div class="small muted">Evidence</div>${fieldValue(d.evidence)}</div>` : ""}${d.notify_at && Date.parse(d.notify_at) > Date.now() ? `<p class="small faint">Pings ${esc(when(d.notify_at))}.</p>` : ""}${taintNote(d.taint)}${buttons(i, [["dismiss", "Dismiss"]])}`;
      break;
    // An outside agent's request, or the reply to one waiting to be released (outside-agents.js).
    case "outside_request":
    case "outside_reply":
      body = outsideAgentCard(i);
      break;
    case "shared":
      body = `<div class="msg magnus"><div class="small faint">${esc(d.speaker)} · ${esc(when(d.at))}</div><div class="pre">${esc(d.text)}</div></div>${taintNote(d.taint)}
        ${buttons(i, [["dismiss", "Dismiss"]])}`;
      break;
    case "deletion_request":
      body = `<div class="what"><div class="field"><span>From</span><span>${esc(d.requested_by)}</span></div><div class="field"><span>What</span><span>${esc(d.what)}</span></div>
        <div class="field"><span>Due</span><span>${esc(new Date(d.due).toLocaleDateString())}</span></div></div>
        ${i.can_respond ? `<textarea rows="2" data-draft="ans-${esc(i.id)}" placeholder="A note for them: what was deleted, or why not" style="margin-top:10px"></textarea>` : ""}
        ${buttons(i, [["done", "Deleted", "primary"], ["decline", "Decline"]])}`;
      break;
    // An entry to make a company fact: what it says, where it came from, and
    // what the appliance found when it checked its citations.
    case "endorsement": {
      const ev = d.verification?.evidence || [];
      const how = { corroborated: "found in independent sources", system_of_record: "found in a system of record" }[d.verification?.how];
      body = `<div class="msg magnus"><div class="pre">${esc(d.text)}</div></div>
        <div class="what"><div class="field"><span>Now</span><span>${esc(d.integrity)}${how ? ` (${esc(how)})` : ""}</span></div>
        <div class="field"><span>Readable by</span><span>${esc((d.readers || []).length ? d.readers.map(c => c.join(" or ")).join(", and ") : "everyone")}</span></div>
        <div class="field"><span>Asked by</span><span>${esc(d.requested_by)}</span></div>
        <div class="field"><span>Source</span>${ev.length ? `<ul class="small">${ev.map(c => `<li>${c.found ? "✓" : "✗ not found in"} ${esc(c.source)}: “${esc(c.quote)}”</li>`).join("")}</ul>` : fieldValue(d.source)}</div></div>
        ${i.can_respond ? `<textarea rows="2" data-draft="ans-${esc(i.id)}" placeholder="A note: how you checked it (optional)" style="margin-top:10px"></textarea>` : ""}
        ${buttons(i, [["endorse", "Make it a company fact", "primary"], ["decline", "Decline"]])}`;
      break;
    }
    case "monitor":
      body = `<div class="what"><div class="field"><span>Action</span>${fieldValue(d.tool || d.action)}</div><div class="field"><span>Why it was held</span>${fieldValue(d.reason)}</div></div>
        ${buttons(i, [["allow", "Allow", "primary"], ["reject", "Reject"]])}`;
      break;
    case "secure_form":
      body = secureFormCard(i);
      break;
    case "takeover_request":
      body = takeoverRequestCard(i);
      break;
    case "payment":
      body = paymentCard(i);
      break;
    // One card per app, never per tool, and never a description (spec rev 140).
    case "app_tools":
      body = appToolsCard(i);
      break;
    default:
      body = `<div class="what">${fieldValue(d)}</div>`;
  }
  return `<article class="card item ${esc(i.kind)} ${esc(tier)}" data-id="${esc(i.id)}">${head}<h4>${esc(i.summary)}</h4>${body}</article>`;
}

// Draw composed outcomes into their frames, from trusted code only.
async function respond(btn, root) {
  // Sent once, however many handlers see the click.
  if (btn.disabled) return;
  const id = btn.dataset.item;
  let [decision, scope] = btn.dataset.decision.split(":");
  // Asking for more spend is not an answer to the notice: it opens the ask.
  if (decision === "ask-more") { if (typeof askMoreFromNotice === "function") askMoreFromNotice(btn); return; }
  const body = { decision };
  if (scope) body.scope = scope;
  const box = $(`[data-draft="ans-${CSS.escape(id)}"]`, root) || $(`[data-draft="rev-${CSS.escape(id)}"]`, root);
  if (btn.dataset.text) body.text = btn.dataset.text;
  else if (box && box.value.trim()) body.text = box.value.trim();
  if (decision === "approve") {
    const editBox = $(`[data-ask-edit="${CSS.escape(id)}"]`, root);
    if (editBox) {
      const edited = {};
      for (const inp of $$("[data-edit-field]", editBox)) {
        if (inp.value !== (inp.dataset.orig ?? "")) edited[inp.dataset.editField] = inp.value;
      }
      if (Object.keys(edited).length) body.edited_input = edited;
    }
    if (scope === "always" && body.edited_input) {
      scope = "once";
      body.scope = "once";
    } else if (scope === "always") {
      const risk = btn.dataset.risk || btn.closest?.(".ask[data-tier]")?.dataset.tier || "";
      if (risk === "irreversible" || risk === "financial") {
        const ok = await ask(`Allow future ${risk} actions like this automatically without asking? You can review or remove this anytime in Rules.`, "Allow always", true);
        if (!ok) return;
      }
    }
  }
  if (decision === "answer" && !body.text) { toast("Write an answer first."); return; }
  btn.disabled = true;
  try {
    await Server.inbox.answer(id, body);
    // Said as given only now the server took it (aurora-alerts.js, aurora-stage.js).
    document.dispatchEvent(new CustomEvent("magnus:answered", { detail: { item: id, decision, button: btn } }));
    delete drafts[`ans-${id}`]; delete drafts[`rev-${id}`]; delete drafts[`cc-${id}`];
    if (decision === "approve" && scope === "always") toast("Allowed from now on. You can review or remove this anytime in Rules.", 5000);
    else toast({ endorse: "It is now a company fact.", approve: "Allowed.", reject: "Done.", accept: "Accepted.", answer: "Sent.", dismiss: "Dismissed.", resume: "Continuing.", extend: "Budget doubled.", keep: "Kept.", disconnect: "Disconnected." }[decision] || "Done.");
    if (decision === "approve" && scope !== "always" && typeof Server !== "undefined" && Server.rules?.suggestions) {
      Server.rules.suggestions().then(r => {
        const sg = (r?.suggestions || [])[0];
        if (!sg) return;
        const nice = String(sg.tool || "").replace(/^[^_]+__/, "").replace(/_/g, " ");
        toast(`You've allowed “${nice}” (${sg.connector}) ${sg.count} times.`, 9000, {
          label: "Allow automatically",
          onClick: async () => {
            try {
              await Server.rules.save(null, { connector: sg.connector, tool: sg.tool, destinations: [sg.destination], effect: "allow", words: sg.words });
              toast("Standing rule saved.");
            } catch (err) { toast(err.message); }
          },
        });
      }).catch(() => {});
    }
    await refresh();
  } catch (e) { toast(e.message); btn.disabled = false; }
}

// Needs you: every decision waiting on the person (U1), most pressing
// first, each the same card as in its conversation, with a way there.
// Compact rows for the home feed: ended tasks link to their detail,
// items to the inbox below, where the open ones already wait (#367).
function feedRows(feed) {
  const rows = [];
  for (const t of feed.ended) rows.push(`<div class="feed-row"><a href="#/task/${enc(t.task_id)}">${esc(t.title || "A task")}</a> <span class="chip ${t.status === "failed" ? "bad" : "ok"}">${esc(t.status)}</span></div>`);
  for (const i of feed.arrived) rows.push(`<div class="feed-row"><a href="#/item/${enc(i.id)}" data-open="item:${esc(i.id)}">${esc(i.summary || i.kind)}</a> <span class="small muted">new ${esc(i.kind)}</span></div>`);
  for (const i of feed.decided) rows.push(`<div class="feed-row"><a href="#/item/${enc(i.id)}" data-open="item:${esc(i.id)}">${esc(i.summary || i.kind)}</a> <span class="small muted">${esc(i.resolution?.decision || "decided")}</span></div>`);
  return rows.join("");
}

function activityLinks(selected="activity") {
  return `<nav class="activity-views" aria-label="Activity views">${[["activity","Overview"],["tasks","Tasks"],["schedules","Schedules"],["rules","Rules"]].map(([k,l])=>`<a href="#/${k}" data-open="${k}" ${selected===k ? 'aria-current="page"' : ""}>${l}</a>`).join("")}</nav>`;
}
function companionStatus() {
  const active = App.tasks.filter(t => !ENDED.includes(t.status)).length;
  const waiting = needsMe().length;
  if (waiting) return { tone: "warn", dot: "warn", label: "Working" };
  if (active) return { tone: "run", dot: "ok", label: "Working" };
  return { tone: "ok", dot: "ok", label: "Ready" };
}
function dayBucketLabel(iso) {
  if (!iso) return "Earlier";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "Earlier";
  const now = new Date();
  const startToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const startItem = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const DAY_MS = 86_400_000;
  const days = Math.round((startToday - startItem) / DAY_MS);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
function companionHeader() {
  const st = companionStatus();
  const taskCount = (App.tasks || []).filter(t => dayBucketLabel(t.ended_at || t.created_at) === "Today").length;
  const list = (typeof Lib !== "undefined" && Array.isArray(Lib.all))
    ? Lib.all
    : Array.isArray(App.connectors)
      ? App.connectors
      : null;
  const appCount = list
    ? list.filter(c => (typeof stateOf === "function" ? stateOf(c) : c.state) === "connected").length
    : null;
  const subText = `${taskCount} task${taskCount === 1 ? "" : "s"} today${appCount !== null ? ` · ${appCount} app${appCount === 1 ? "" : "s"}` : ""}`;
  return SidePanel.head({
    letter: "M",
    avatarHref: "#/settings/appearance",
    avatarTitle: "Theme & accent",
    dot: st.dot,
    tone: st.tone,
    label: st.label,
    titleHtml: "Magnus",
    subText,
    topActs: true,
  });
}
function companionFooter() {
  const comp = (App.computers || []).find(x => x.owner?.person === App.me?.id) || (App.computers || [])[0];
  if (!comp) return "";
  const compState = controlWords(comp, false);
  const awake = comp.status === "running";
  return SidePanel.computerFoot({
    id: comp.id,
    kindLabel: "Cloud Computer",
    stateText: compState.pill,
    awake,
    pauseId: awake ? comp.id : null,
    watchOpen: awake ? `computer:${comp.id}` : null,
    wakeId: awake ? null : comp.id,
  });
}
function timelineRowHtml(t) {
  const [ic] = standing(t);
  const ts = when(t.ended_at || t.created_at);
  const brandHtml = SidePanel.brand(`${t.title || ""} ${t.summary || ""} ${t.text || ""}`, "tl-brand")
    || `<span class="tl-brand tl-ic st ${ic}" aria-hidden="true">${icon(ic)}</span>`;
  return `<li class="tl-row" data-state="${STATE(t)}" data-task="${esc(t.task_id)}"><a href="#/task/${enc(t.task_id)}" data-open="task:${esc(t.task_id)}">${brandHtml}<div class="tl-info"><div class="tl-title-line"><span class="ttl">${esc(t.title || firstLine(t.text) || "Task")}</span><span class="mono faint tl-time">${esc(ts)}</span></div></div></a></li>`;
}
function timelineGroups(active, done) {
  const groups = [];
  if (active.length) groups.push(["Active now", active.slice(0, 6)]);
  const byDay = new Map();
  for (const t of done.slice(0, 14)) {
    const label = dayBucketLabel(t.ended_at || t.created_at);
    if (!byDay.has(label)) byDay.set(label, []);
    byDay.get(label).push(t);
  }
  for (const [label, list] of byDay) groups.push([label, list]);
  if (!groups.length) return `<p class="empty">No work yet. Ask Magnus anything in the conversation.</p>`;
  return groups.map(([label, list]) => `<details class="tl-day" data-key="day:${esc(label)}" open>
    <summary class="tl-day-head"><span>${esc(label)}</span><span class="tl-day-count">${list.length}</span></summary>
    <div class="gbox"><ul class="tlist tl-stack">${list.map(timelineRowHtml).join("")}</ul></div>
  </details>`).join("");
}
// A decision waiting on the person, with its app's icon, tier, preview and answers.
function needsInlineCard(i) {
  const ic = SidePanel.brand(`${i.kind || ""} ${i.summary || ""} ${i.ask?.sentence || ""}`) || `<span class="gbox-ic warn" aria-hidden="true">${icon("need")}</span>`;
  const quickMap = {
    approval: ["approve", "reject", "Approve", "Deny"],
    review: ["accept", "dismiss", "Accept", "Dismiss"],
    endorsement: ["endorse", "decline", "Endorse", "Decline"],
  };
  const quick = quickMap[i.kind];
  const fields = typeof askSummaryFields === "function" ? askSummaryFields(i) : { rows: [], body: "" };
  const preview = [
    ...fields.rows.slice(0, 2).map(([k, v]) => `${k}: ${v}`),
    i.consequence || fields.body || "",
  ].filter(Boolean).join(" · ");
  const tier = typeof askTierBadge === "function" ? askTierBadge(i) : "";
  const acts = quick
    ? `<button type="button" data-item="${esc(i.id)}" data-decision="${quick[1]}">${quick[3]}</button>
      <button type="button" class="primary ok-btn" data-item="${esc(i.id)}" data-decision="${quick[0]}">${quick[2]}</button>`
    : `<a class="btn primary" href="#/inbox" data-open="item:${esc(i.id)}">Open →</a>`;
  return `<div class="gbox needs-inline-card">
    <a class="gbox-row" href="#/inbox" data-open="item:${esc(i.id)}">${ic}<span class="gbox-main"><span class="gbox-lbl">${esc(i.summary || i.ask?.sentence || (KIND && KIND[i.kind]) || "Approval needed")}</span><span class="small muted">${tier}${tier ? " " : ""}${esc((KIND && KIND[i.kind]) || i.kind || "Pending decision")}</span>${preview ? `<span class="needs-inline-preview">${esc(preview)}</span>` : ""}</span></a>
    <div class="needs-inline-acts">${acts}</div>
  </div>`;
}
// The schedules coming up, each with its app's icon or a calendar. The
// heartbeat is retired (decisions/2026-11-26-the-heartbeat-is-retired.md):
// nothing here offers to start one.
function schedRows(scheds) {
  return scheds.length ? scheds.slice(0, 3).map(sc => {
    const icHtml = SidePanel.brand(`${sc.title || ""} ${sc.text || ""}`, "gbox-ic sch has-logo") || `<span class="gbox-ic sch">${icon("calendar")}</span>`;
    return `<div class="trow-wrap"><a class="gbox-row sched-card" href="#/schedules" data-open="schedule:${esc(sc.id)}">${icHtml}<span class="gbox-main"><span class="gbox-lbl">${esc(sc.title)}</span><span class="small muted">${esc(rule(sc))}</span></span><span class="mono small muted">${esc(sc.paused ? "Paused" : sc.next ? until(sc.next) : "")}</span></a><button type="button" class="trow-del" data-act="delete-schedule" data-id="${esc(sc.id)}" title="Delete schedule" aria-label="Delete ${esc(sc.title)}">${icon("trash")}</button></div>`;
  }).join("") : `<div class="gbox-empty"><span class="small muted">No scheduled tasks yet</span></div>`;
}
const guardrailsRow = () => `<div class="gbox companion-guard"><a class="gbox-row" href="#/rules" data-open="rules"><span class="gbox-ic">${icon("shield")}</span><span class="gbox-main"><span class="gbox-lbl">Safety Guardrails</span><span class="small muted">Ask before external actions &amp; payments</span></span>${icon("chev", "chev")}</a></div>`;
// The overview's parts, which the panel stacks and an Aurora stage
// (aurora-stage.js) lays out as cards.
// What Magnus noticed on its own and queued for the person: observations,
// and interruptions it already delivered. Clear all dismisses every open one.
const OBSERVATION_KINDS = new Set(["observation", "interrupt"]);
// The most ids one batch call takes (`MAX_BATCH_INBOX_ITEMS` on the server).
const MAX_BATCH_INBOX_ITEMS = 50;
async function clearObservations(btn) {
  const ids = App.inbox.filter(i => i.status === "open" && OBSERVATION_KINDS.has(i.kind)).map(i => i.id);
  if (!ids.length) return;
  btn.disabled = true;
  let cleared = 0;
  try {
    for (let n = 0; n < ids.length; n += MAX_BATCH_INBOX_ITEMS) {
      const res = await Server.inbox.batch(ids.slice(n, n + MAX_BATCH_INBOX_ITEMS), { decision: "dismiss" });
      cleared += (res?.results || []).filter(r => r.ok).length;
    }
    toast(`Cleared ${cleared} observation${cleared === 1 ? "" : "s"}.`);
  } catch (err) { toast(err.message); }
  btn.disabled = false;
  await refresh();
}
addEventListener("click", e => {
  const btn = e.target.closest?.("button[data-clear-observations]");
  if (btn) clearObservations(btn);
});
function activityParts(composed) {
  const quietSys = t => typeof isQuietSystemTask === "function" ? isQuietSystemTask(t) : !!(t?.system && !t.needs_you && t.status !== "failed");
  const sysSched = sc => typeof isSystemSchedule === "function" ? isSystemSchedule(sc) : !!(sc?.system || /heartbeat/i.test(sc?.title || sc?.text || ""));
  const visibleTasks = App.tasks.filter(t => !quietSys(t));
  const active = visibleTasks.filter(t => !ENDED.includes(t.status));
  const done = visibleTasks.filter(t => ENDED.includes(t.status)).sort((a, b) => (b.ended_at || b.created_at || "").localeCompare(a.ended_at || a.created_at || ""));
  const pending = needsMe();
  const scheds = App.schedules.filter(sc => !sc.ended && !sysSched(sc));
  const other = App.inbox.filter(i => i.status === "open" && !needsDecision(i) && !ExploreIdeas.isIdea(i));
  const hasOpenIdeas = typeof ExploreIdeas !== "undefined" ? ExploreIdeas.open(App.inbox).length > 0 : false;
  const shownOther = other.filter(i => !(typeof ExploreIdeas !== "undefined" && ExploreIdeas.isNotice(i) && !hasOpenIdeas));
  const feed = App.feed && (App.feed.ended.length || App.feed.arrived.length || App.feed.decided.length) ? feedRows(App.feed) : "";
  const needsBanner = SidePanel.waitingBanner({
    several: false,
    count: pending.length,
    viewAllHref: "#/inbox",
    viewAllOpen: "needs",
    clearedHref: "#/inbox",
    clearedOpen: "needs",
    rowsHtml: pending.slice(0, 1).map(needsInlineCard).join(""),
  });
  const curTask = active[0];
  const units = curTask?.plan?.units || [];
  const doneUnits = units.filter(u => u.status === "done").length;
  const pct = units.length ? Math.round((100 * doneUnits) / units.length) : null;
  const footStatus = pending.length
    ? `<span class="small muted">${esc(`Waiting on ${pending.length} approval${pending.length === 1 ? "" : "s"}`)}</span>`
    : curTask?.status === "suspended"
      ? `<span class="small muted">${esc(WAITING[curTask.waiting_on] || "Waiting")}</span>`
      : curTask && typeof statusLineSlot === "function"
        ? statusLineSlot(curTask, "small muted")
        : `<span class="small muted">${esc(curTask?.status_line || "In progress")}</span>`;
  const activeCard = curTask ? SidePanel.activeCard({
    href: `#/task/${enc(curTask.task_id)}`,
    open: `task:${curTask.task_id}`,
    rightMeta: when(curTask.created_at),
    title: curTask.title || firstLine(curTask.text) || "Running task",
    pct,
    footHtml: footStatus,
    inspectLabel: "Inspect →",
  }) : "";
  const recentList = (done.length ? done : active).slice(0, 3);
  const recentHtml = recentList.length
    ? `<ul class="tlist tl-stack">${recentList.map(timelineRowHtml).join("")}</ul>`
    : `<p class="empty">No work yet. Ask Magnus anything in the conversation.</p>`;
  const schedCard = `<section class="companion-scheds">
    <div class="companion-sec-head"><span>Up Next</span><a class="sec-head-link" href="#/schedules" data-open="schedules">${scheds.length} schedule${scheds.length === 1 ? "" : "s"}</a></div>
    <div class="scheds-list">
      ${schedRows(scheds)}
    </div>
  </section>`;
  const told = shownOther.filter(i => OBSERVATION_KINDS.has(i.kind)).length;
  const clearAll = told ? `<div class="spread"><span class="small muted">${told} observation${told === 1 ? "" : "s"}</span><button type="button" class="small" data-clear-observations>Clear all</button></div>` : "";
  const updates = (feed || shownOther.length) ? `<details class="collapsed-done" data-key="other"><summary>Updates (${shownOther.length}${feed ? " + last session" : ""})</summary>${feed ? `<div style="margin:8px 0;">${feed}</div>` : ""}${clearAll}${shownOther.map(i => card(i, composed)).join("")}</details>` : "";
  return { active, done, pending, scheds, curTask, needsBanner, activeCard, recentHtml, schedCard, updates };
}
function briefingCard() {
  const b = App.briefing;
  if (!b) return "";
  const total = (b.needs_count || 0) + (b.staged_count || 0) + (b.active_count || 0) + (b.completed_24h || 0) + (b.overdue_count || 0);
  if (!total) return "";
  const stat = (n, lbl, attrs) => n ? `<a class="briefing-stat" ${attrs}><strong>${n}</strong><span>${esc(lbl)}</span></a>` : "";
  const stats = [
    stat(b.needs_count, "waiting", 'href="#/inbox" data-open="needs"'),
    stat(b.staged_count, "drafts", 'href="#/inbox" data-open="needs"'),
    stat(b.active_count, "running", 'href="#/tasks" data-open="tasks"'),
    stat(b.completed_24h, "done 24h", 'href="#/tasks" data-open="tasks"'),
    stat(b.overdue_count, "overdue", 'href="#/settings/memory"'),
  ].filter(Boolean).join("");
  const overdue = Array.isArray(b.overdue) && b.overdue.length
    ? `<ul class="small muted" style="margin:8px 0 0;padding-left:16px">${b.overdue.slice(0, 3).map(o => `<li><strong>${esc(o.what || "Promise")}</strong>${o.to ? ` · ${esc(o.to)}` : ""}${o.by ? ` <span class="faint">(due ${esc(when(o.by))})</span>` : ""}</li>`).join("")}</ul>`
    : "";
  return `<section class="card flat briefing-card" data-briefing><div class="spread"><strong class="small">Briefing</strong><span class="small faint">Last 24h</span></div><div class="briefing-stats">${stats}</div>${overdue}</section>`;
}
function activityView(composed) {
  const p = activityParts(composed);
  return `${typeof briefingCard === "function" ? briefingCard() : ""}
    ${p.needsBanner}
    ${p.activeCard}
    <section class="companion-timeline">
      <div class="companion-sec-head"><span>Recent Work</span><span>Today</span></div>
      ${p.recentHtml}
    </section>
    ${p.schedCard}
    ${p.updates}`;
}

function stagedDraftCard(st) {
  const inp = st.input && typeof st.input === "object" ? st.input : {};
  const editable = Object.entries(inp).filter(([k, v]) => /^(to|cc|subject|title|body|message|text|content|channel)$/i.test(k) && typeof v === "string");
  const fieldsHtml = editable.map(([k, v]) => {
    const label = esc(k.replace(/_/g, " "));
    const multi = /^(body|message|text|content)$/i.test(k) || v.length > 80 || v.includes("\n");
    return multi
      ? `<label class="small">${label}<textarea data-staged-field="${esc(k)}" data-orig="${esc(v)}" rows="3">${esc(v)}</textarea></label>`
      : `<label class="small">${label}<input type="text" data-staged-field="${esc(k)}" data-orig="${esc(v)}" value="${esc(v)}"></label>`;
  }).join("");
  const scopes = Array.isArray(st.offered_scopes) && st.offered_scopes.length ? st.offered_scopes : ["once"];
  const scopeLabels = { once: "Just this once", recipient: "Always allow this recipient", domain: "Always allow this domain" };
  return `<article class="card item" data-staged="${esc(st.id)}">
    <div class="spread"><span class="kind">Staged draft · ${esc(st.app_title || st.app)}</span><span class="small faint">${esc(when(st.created_at))}</span></div>
    <p class="ask-sentence">${esc(st.sentence || st.tool)}</p>
    ${fieldsHtml ? `<details class="ask-edit" data-staged-edit><summary class="small">Edit draft before sending</summary><div class="stack" style="margin-top:6px">${fieldsHtml}</div></details>` : ""}
    <div class="row" style="margin-top:8px;flex-wrap:wrap">
      ${scopes.length > 1 ? `<select data-staged-scope aria-label="Future scope">${scopes.map(s => `<option value="${esc(s)}">${esc(scopeLabels[s] || s)}</option>`).join("")}</select>` : ""}
      <button type="button" class="primary" data-staged-act="sent" data-id="${esc(st.id)}">Send</button>
      <button type="button" data-staged-act="discarded" data-id="${esc(st.id)}">Discard</button>
    </div>
  </article>`;
}

function needsView(composed) {
  const open=needsMe();
  const staged = Array.isArray(App.staged) ? App.staged : [];
  const now=open.filter(i=>i.blocking), later=open.filter(i=>!i.blocking);
  const where = i => {
    const t = i.thread && App.threads.find(x => x.thread === i.thread);
    return t ? `<div class="small where">In <a href="#/chat/${enc(t.thread)}">${esc(t.title)}</a></div>` : "";
  };
  const appBadge = i => {
    const hay = `${i.kind || ""} ${i.summary || ""} ${i.ask?.sentence || ""}`.toLowerCase();
    const b = typeof AppLogos !== "undefined" && AppLogos.forText ? AppLogos.forText(hay) : null;
    if (b) return `<span class="appr-ic ${b.cls} has-logo" aria-hidden="true">${b.svg}</span>`;
    if (/mail|gmail|email/.test(hay)) return `<span class="appr-ic mail" aria-hidden="true">M</span>`;
    return `<span class="appr-ic" aria-hidden="true">${icon("globe")}</span>`;
  };
  const batchable = open.filter(i => i.kind === "approval" && i.can_respond);
  const batchBar = batchable.length >= 2 ? `<div class="card flat spread" data-batch-bar style="margin-bottom:10px;gap:8px;flex-wrap:wrap">
    <label class="row small"><input type="checkbox" data-batch-all> Select all (${batchable.length})</label>
    <div class="row"><button type="button" class="primary small" data-batch-decide="approve">Allow selected</button><button type="button" class="small" data-batch-decide="reject">Don't allow selected</button></div>
  </div>` : "";
  const list = items => `<div class="appr-list">${items.map(i => `<div class="needs-item" data-needs="${esc(i.id)}">
    <div class="spread">${where(i)}${batchable.length >= 2 && i.kind === "approval" && i.can_respond ? `<label class="row small muted"><input type="checkbox" data-batch-id="${esc(i.id)}"> Select</label>` : ""}</div>
    ${card(i, composed)}
  </div>`).join("")}</div>`;
  const stagedHtml = staged.length ? `<section data-staged-section style="margin-top:12px"><div class="companion-sec-head"><span>Staged drafts</span><span class="count">${staged.length}</span></div><div class="appr-list">${staged.map(stagedDraftCard).join("")}</div></section>` : "";
  // What was decided, not what was only read: a dismissed observation was
  // never an approval.
  const decided = App.inbox.filter(i => i.status !== "open" && wasAsk(i)).sort((a, b) => (b.created_at || "").localeCompare(a.created_at || "")).slice(0, 15);
  const resText = i => {
    const d = i.resolution?.decision || i.status || "decided";
    return { approve: "Allowed", reject: "Rejected", accept: "Accepted", answer: "Answered", dismiss: "Dismissed", endorse: "Endorsed", decline: "Declined" }[d] || d;
  };
  const historyHtml = decided.length ? `<section class="appr-history"><div class="companion-sec-head"><span>Approvals history</span><span class="count">${decided.length}</span></div>
    <div class="appr-list">${decided.map(i => `<div class="appr-hist-row">
      <div class="appr-row-top">${appBadge(i)}<div class="appr-row-main">
        <div class="appr-hist-title">${esc(KIND[i.kind] || i.kind)}</div>
        <div class="small muted">${esc(i.summary || i.ask?.sentence || i.kind)}</div>
        <div class="small faint appr-meta">${esc(resText(i))} · ${esc(when(i.created_at))}</div>
        <details class="appr-payload" data-key="hist:${esc(i.id)}"><summary class="small">View details</summary><div class="gbox appr-payload-box">${i.kind === "approval" ? askContent(i.details || {}) : fieldValue(i.details || i.resolution || {})}</div></details>
      </div></div>
    </div>`).join("")}</div></section>` : "";
  const guardrails = guardrailsRow();
  // Nova heads the queue as its mockup does: the queue, how many wait, and
  // the keys that decide (A allows, D declines: nova-ux.js).
  const nova = typeof Theme !== "undefined" && Theme.has?.("topBar");
  const queueHead = n => nova ? `<div class="companion-sec-head appr-queue-head"><span>Approval queue</span><span class="count"${n ? "" : " data-none"}>${n} waiting</span>${n ? `<span class="appr-keys" aria-hidden="true"><kbd>A</kbd>allow <kbd>D</kbd>decline</span>` : ""}</div>` : `<div class="companion-sec-head"><span>Approvals</span><span class="count">${n}</span></div>`;
  if (!open.length && !staged.length) return `${nova ? queueHead(0) : ""}<p class="empty">You're all caught up. Nothing is waiting on you right now.</p>${guardrails}${historyHtml}`;
  return `${open.length ? `${queueHead(open.length)}${batchBar}${list([...now, ...later])}` : ""}${stagedHtml}${guardrails}${historyHtml}`;
}
