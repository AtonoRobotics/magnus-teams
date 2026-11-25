// Tasks, schedules, and task detail views.

// Where a task stands for the person looking: waiting on them, working,
// waiting on something else (a set time), done, or failed or cancelled.
const STATE = t => t.needs_you ? "needs" : t.status === "running" ? "running" : t.status === "suspended" ? "waiting" : t.status === "completed" ? "done" : "failed";
const SECTIONS = [["needs", "Needs you"], ["running", "Running"], ["waiting", "Waiting"], ["done", "Done"], ["failed", "Failed"]];

// Tasks grouped by the request that started them: a chat message or a
// schedule; a task started on its own is a group of one. A group sits in the
// section of its most pressing task.
function groupTasks(tasks) {
  const by = new Map();
  for (const t of tasks) {
    const key = t.group ? `${t.group.kind}:${t.group.id}` : `task:${t.task_id}`;
    if (!by.has(key)) by.set(key, { key, group: t.group, tasks: [] });
    by.get(key).tasks.push(t);
  }
  const groups = [...by.values()];
  for (const g of groups) {
    g.tasks.sort((a, b) => (a.created_at || "").localeCompare(b.created_at || ""));
    g.count = Object.fromEntries(SECTIONS.map(([s]) => [s, g.tasks.filter(t => STATE(t) === s).length]));
    g.section = ["needs", "running", "waiting", "failed"].find(s => g.count[s]) || "done";
    g.latest = g.tasks.map(t => t.ended_at || t.created_at || "").sort().pop();
  }
  groups.sort((a, b) => b.latest.localeCompare(a.latest));
  return SECTIONS.map(([state, label]) => ({ state, label, groups: groups.filter(g => g.section === state) })).filter(x => x.groups.length);
}

function progress(g) {
  const c = g.count, cancelled = g.tasks.filter(t => t.status === "cancelled").length;
  const parts = [`${c.done} of ${g.tasks.length} done`];
  if (c.needs) parts.push(`${c.needs} need${c.needs === 1 ? "s" : ""} you`);
  if (c.running) parts.push(`${c.running} running`);
  if (c.waiting) parts.push(`${c.waiting} waiting`);
  if (c.failed - cancelled) parts.push(`${c.failed - cancelled} failed`);
  if (cancelled) parts.push(`${cancelled} cancelled`);
  return parts.join(" · ");
}

function taskRow(t) {
  const why = t.status === "suspended" ? WAITING[t.waiting_on] || (t.needs_you ? "waiting for you" : "") : "";
  const canDelete = typeof ENDED !== "undefined" && ENDED.includes(t.status);
  const delBtn = canDelete
    ? `<button type="button" class="trow-del" data-trash="${esc(t.task_id)}" data-trash-label="Delete task" title="Delete task" aria-label="Delete ${esc(t.title || "task")}">${typeof icon === "function" ? icon("trash") : "×"}</button>`
    : "";
  return `<li class="trow" data-state="${STATE(t)}" data-task="${esc(t.task_id)}"><div class="trow-wrap"><a href="#/task/${enc(t.task_id)}" data-open="task:${esc(t.task_id)}"><span class="ttl" title="${esc(t.text)}">${esc(t.title)}</span>${chip(t.status)}<span class="small muted why">${esc(why)}</span><span class="small faint when">${esc(when(t.ended_at || t.created_at))}</span></a>${delBtn}</div></li>`;
}

// A group: its request and progress, the tasks that need the person in
// view, the rest folded away.
function groupBlock(g, section) {
  if (!g.group && g.tasks.length === 1) return `<ul class="tlist">${taskRow(g.tasks[0])}</ul>`;
  const href = g.group.kind === "chat" ? `href="#/chat/${enc(g.group.thread)}"` : `href="#/schedules" data-open="schedule:${esc(g.group.id)}"`;
  const shown = section === "needs" ? g.tasks.filter(t => t.needs_you) : [];
  const rest = g.tasks.filter(t => !shown.includes(t));
  return `<div class="tgroup" data-group="${esc(g.key)}"><div class="ghead"><a class="ttl" ${href} title="${esc(g.group.title)}">${g.group.kind === "schedule" ? "Schedule: " : ""}${esc(g.group.title)}</a><span class="small muted">${esc(progress(g))}</span></div>
    ${shown.length ? `<ul class="tlist">${shown.map(taskRow).join("")}</ul>` : ""}
    ${rest.length ? `<details data-key="${esc(g.key)}"><summary class="small muted">${shown.length ? `The other ${rest.length}` : `${rest.length} task${rest.length === 1 ? "" : "s"}`}</summary><ul class="tlist">${rest.map(taskRow).join("")}</ul></details>` : ""}</div>`;
}

// Tasks, as a view over the same tasks the conversations show: grouped by
// the request that started them, in sections by where they stand.
const MORE = new Set();
function tasksView() {
  const LIMIT = { done: 15, failed: 15 };
  const sections = groupTasks(App.tasks);
  const activeCount = App.tasks.filter(t => t.status === "running" || t.status === "suspended").length + App.schedules.filter(s => !s.paused && !s.ended).length;
  const stopBar = activeCount ? `<div class="spread" style="margin-bottom:8px"><span class="small muted">Active work running or scheduled</span><button type="button" class="danger small" data-stop-all-work>Stop my active work</button></div>` : "";
  return sections.length ? `${stopBar}${sections.map(sec => {
    const n = sec.groups.reduce((k, g) => k + g.count[sec.state], 0);
    const cap = LIMIT[sec.state] && !MORE.has(sec.state) ? LIMIT[sec.state] : Infinity;
    const left = sec.groups.length - Math.min(cap, sec.groups.length);
    return `<section class="tsec" data-section="${sec.state}"><h3>${esc(sec.label)} <span class="count">${n}</span></h3>
      ${sec.groups.slice(0, cap).map(g => groupBlock(g, sec.state)).join("")}
      ${left ? `<button class="link small" data-more="${sec.state}">Show ${left} more</button>` : ""}</section>`;
  }).join("")}` : offered("No tasks yet. Ask for something in a conversation.");
}

// An empty Tasks or Schedules: what Magnus offered to take over (its ideas
// in Explore's For you, explore-ideas.js) that the person has not answered,
// each linking there, in Magnus's voice. With none, the plain line.
function offered(none) {
  const ideas = ExploreIdeas.open(App.inbox);
  if (!ideas.length) return `<p class="empty">${esc(none)}</p>`;
  return `<div class="empty" data-offered><p>Nothing here yet. I had a look around, and these are mine to take off your hands as soon as you say so:</p>
    <ul class="tlist">${ideas.map(i => `<li class="trow" data-offer="${esc(i.id)}"><a href="#/explore"><span class="ttl">${esc(i.details?.title || i.summary)}</span></a></li>`).join("")}</ul></div>`;
}

// A schedule's next run and its last result, in one line.
function scheduleState(sc) {
  const last = sc.last;
  return `${(sc.paused || sc.ended) ? "" : `Next run ${esc(until(sc.next))} · `}${sc.last_failed ? `Last firing <span class="chip bad">could not start</span> ${esc(sc.last_failed.error)} <span class="faint">${esc(when(sc.last_failed.at))}</span>`
    : last ? `Last result: ${chip(last.status)} <a href="#/task/${enc(last.task_id)}" data-open="task:${esc(last.task_id)}">${esc(firstLine(last.summary) || last.title)}</a> <span class="faint">${esc(when(last.ended_at || last.created_at))}</span>` : "No results yet"}`;
}

// When a schedule ends, what it may spend a run, and why it is paused or
// skipped a run: the same lines in the list and in the schedule itself.
function scheduleFacts(sc) {
  return `${sc.ended ? `<div class="small" data-ended><span class="chip">Ended</span> ${esc(sc.ended)}.</div>` : ""}
    <div class="small muted" data-ends>${sc.until || sc.max_runs ? `Ends: ${esc(ends(sc))}` : "Runs until you stop it"}${sc.max_runs ? ` · ${esc(sc.started)} of ${esc(sc.max_runs)} runs so far` : ""}${sc.budget ? ` · up to $${esc(sc.budget.max_usd)} a run` : ""}</div>
    ${sc.paused ? `<div class="small" data-paused><span class="chip bad">Paused</span> ${esc(sc.paused)}. ${sc.paused_on ? `It goes on once you answer that run (<a href="#/task/${enc(sc.paused_on)}" data-open="task:${esc(sc.paused_on)}">open it</a>).` : "Nothing runs until you resume it."}</div>` : ""}
    ${sc.skipped ? `<div class="small muted" data-skipped>Skipped a run ${esc(when(sc.skipped.at))}: ${esc(sc.skipped.why)} (<a href="#/task/${enc(sc.skipped.task)}" data-open="task:${esc(sc.skipped.task)}">open it</a>)</div>` : ""}`;
}
// Every schedule (active, paused, or ended) offers clear actions without redundant Stop/Delete buttons.
function scheduleButtons(sc) {
  if (sc.ended) return `<button type="button" class="danger" data-act="delete-schedule" data-id="${esc(sc.id)}">Delete schedule</button>`;
  const runNow = `<button type="button" data-act="run-schedule" data-id="${esc(sc.id)}">Run now</button>`;
  const toggle = sc.paused ? (!sc.paused_on ? `<button type="button" data-act="resume-schedule" data-id="${esc(sc.id)}">Resume</button>` : "") : `<button type="button" data-act="pause-schedule" data-id="${esc(sc.id)}">Pause</button>`;
  return `${runNow}${toggle}<button type="button" class="danger" data-act="delete-schedule" data-id="${esc(sc.id)}">Delete</button>`;
}

const WEEKDAY_OPTS = [["mon", "Mon"], ["tue", "Tue"], ["wed", "Wed"], ["thu", "Thu"], ["fri", "Fri"], ["sat", "Sat"], ["sun", "Sun"]];

function parseScheduleCadence(sc) {
  const raw = String(sc?.daily_at || "").trim();
  if (sc?.every_minutes) return { kind: "every", at: "08:00", days: ["mon"], monthDay: 1, onceAt: "", every: sc.every_minutes };
  const wd = raw.match(/^weekdays\s+(\d{1,2}:\d{2})$/i);
  if (wd) return { kind: "weekdays", at: wd[1], days: ["mon", "tue", "wed", "thu", "fri"], monthDay: 1, onceAt: "", every: 60 };
  const wk = raw.match(/^weekly\s+([a-z,]+)\s+(\d{1,2}:\d{2})$/i);
  if (wk) return { kind: "weekly", at: wk[2], days: wk[1].toLowerCase().split(",").map(s => s.trim()), monthDay: 1, onceAt: "", every: 60 };
  const mo = raw.match(/^monthly\s+(\d{1,2})\s+(\d{1,2}:\d{2})$/i);
  if (mo) return { kind: "monthly", at: mo[2], days: ["mon"], monthDay: Number(mo[1]) || 1, onceAt: "", every: 60 };
  const once = raw.match(/^once\s+(.+)$/i);
  if (once) return { kind: "once", at: "08:00", days: ["mon"], monthDay: 1, onceAt: once[1].slice(0, 16), every: 60 };
  return { kind: "daily", at: raw || "08:00", days: ["mon"], monthDay: 1, onceAt: "", every: 60 };
}

function scheduleCadenceFields(sc) {
  const c = parseScheduleCadence(sc);
  const opt = (v, label) => `<option value="${v}"${c.kind === v ? " selected" : ""}>${label}</option>`;
  return `<div class="row" style="flex-wrap:wrap">
      <label class="small">Cadence<select name="kind">${opt("weekdays", "Weekdays (Mon–Fri)")}${opt("daily", "Every day")}${opt("weekly", "Specific days")}${opt("monthly", "Monthly")}${opt("every", "Every N minutes")}${opt("once", "One-time")}</select></label>
      <label class="small">Time<input type="time" name="daily_at" value="${esc(c.at)}"></label>
      <label class="small">Every (min)<input type="number" name="every_minutes" min="5" max="10080" value="${esc(String(c.every))}" style="width:5.5em"></label>
      <label class="small">Day of month<input type="number" name="month_day" min="1" max="31" value="${esc(String(c.monthDay))}" style="width:4.5em"></label>
      <label class="small">One-time date<input type="datetime-local" name="once_at" value="${esc(c.onceAt)}"></label>
      <label class="small">Max runs<input type="number" name="max_runs" min="0" placeholder="Unlimited" value="${esc(sc?.max_runs ? String(sc.max_runs) : "")}" style="width:6em"></label>
    </div>
    <div class="row small" style="flex-wrap:wrap;gap:6px" aria-label="Weekly days">${WEEKDAY_OPTS.map(([k, lbl]) => `<label class="row" style="gap:3px"><input type="checkbox" name="days" value="${k}"${c.days.includes(k) ? " checked" : ""}>${lbl}</label>`).join("")}</div>`;
}

// Schedules, soonest first, ended last: each opens beside the conversation.
function schedulesView() {
  const key = sc => `${sc.ended ? "1" : "0"}${sc.next || ""}`;
  const list = [...App.schedules].sort((a, b) => key(a).localeCompare(key(b)));
  const hb = list.find(sc => !sc.ended && /heartbeat/i.test(sc.title || sc.text || ""));
  const hbCard = `<div class="card flat" style="margin-bottom:10px;"><div class="spread"><div><strong>Heartbeat</strong><div class="small muted">${hb ? `Every 30 min · ${hb.paused ? "Paused" : `Next run ${esc(until(hb.next))}`}` : "Proactive 30-min check-in for urgent emails, conflicts, or stalled threads."}</div></div>${hb ? `<div class="row"><span class="chip ${hb.paused ? "warn" : "ok"}">${hb.paused ? "Paused" : "Active"}</span><button type="button" class="danger small" data-act="delete-schedule" data-id="${esc(hb.id)}">Delete</button></div>` : `<button type="button" class="primary" data-enable-heartbeat>Enable (30m)</button>`}</div></div>`;
  const creator = `<details class="card flat" data-key="sched-new" style="margin-bottom:10px">
      <summary class="small"><strong>+ New routine</strong></summary>
      <form data-create-schedule style="margin-top:8px;display:flex;flex-direction:column;gap:8px">
        <label class="small">What should Magnus do?<textarea name="text" rows="2" placeholder="e.g. Every weekday morning, summarize urgent emails and flag calendar conflicts" style="width:100%"></textarea></label>
        ${scheduleCadenceFields({ daily_at: "weekdays 08:30" })}
        <div class="row"><button type="submit" class="primary">Create routine</button></div>
      </form>
    </details>`;
  const header = `<div class="spread" style="margin-bottom:10px;"><span class="small muted">${list.length ? `${list.length} active or past schedule${list.length === 1 ? "" : "s"}` : "Let Magnus find repetitive work you can hand off."}</span><button type="button" data-open-discovery>Discover automations</button></div>`;
  return header + creator + hbCard + (list.length ? list.map(sc => `<div class="card flat sched" data-schedule="${esc(sc.id)}">
      <div class="spread"><a class="ttl" href="#/schedules" data-open="schedule:${esc(sc.id)}" title="${esc(sc.text)}">${esc(sc.title)}</a><span class="small muted">${esc(rule(sc))} ${scheduleButtons(sc)}</span></div>
      ${scheduleFacts(sc)}
      <div class="small muted" data-last>${scheduleState(sc)}</div>
      ${sc.runs.some(r => r.needs_you) ? `<ul class="tlist">${sc.runs.filter(r => r.needs_you).map(taskRow).join("")}</ul>` : ""}</div>`).join("")
    : offered("No schedules yet. Ask in a conversation for something done every day or every week."));
}

// One schedule: what it does, when, inline edit form, how its runs went, and controls.
function scheduleView(id) {
  const sc = App.schedules.find(x => x.id === id);
  if (!sc) return `<p class="empty">Schedule details are not available.</p>`;
  const editForm = !sc.ended ? `<details class="card flat" data-key="sched-edit:${esc(sc.id)}" style="margin:10px 0">
      <summary class="small"><strong>Edit schedule</strong></summary>
      <form class="sched-edit-form" data-edit-schedule="${esc(sc.id)}" style="margin-top:8px;display:flex;flex-direction:column;gap:8px">
        <label class="small">Instructions<textarea name="text" rows="3" style="width:100%">${esc(sc.text)}</textarea></label>
        ${scheduleCadenceFields(sc)}
        <div class="row"><button type="submit" class="primary">Save changes</button></div>
      </form>
    </details>` : "";
  const dryRun = !sc.ended ? `<button type="button" data-act="dry-run-schedule" data-id="${esc(sc.id)}">Dry run</button>` : "";
  return `<h2 class="ptitle">${esc(sc.title)}</h2><p class="small muted">${esc(rule(sc))}</p>
    <div class="card flat sched" data-schedule="${esc(sc.id)}"><p>${esc(sc.text)}</p>${scheduleFacts(sc)}<div class="small" data-last>${scheduleState(sc)}</div></div>
    <div class="row" style="flex-wrap:wrap">${moveControl(sc.id, sc.project, App.projects)}${dryRun}${scheduleButtons(sc)}</div>
    ${editForm}
    <h3>Runs</h3>${sc.runs.length ? `<ul class="tlist">${sc.runs.map(taskRow).join("")}</ul>` : `<p class="empty">None yet.</p>`}
    ${sc.failures?.length ? `<h3>Firings that could not start</h3><ul class="small">${sc.failures.map(f => `<li>${esc(when(f.at))}: ${esc(f.error)}</li>`).join("")}</ul>` : ""}`;
}


// ---------- Task detail ----------

function plan(t) {
  const units = t.plan?.units || [];
  if (!units.length) return `<p class="empty">No plan: it is optional, and shows progress when kept.</p>`;
  const mark = { done: "ok", active: "run", dropped: "", pending: "" };
  // Once the task has ended nothing is in progress: a unit the agent left
  // active or pending was simply not marked done.
  const ended = ["completed", "failed", "cancelled"].includes(t.status);
  const status = u => (ended && (!u.status || u.status === "active" || u.status === "pending")) ? ["", "not marked done"] : [mark[u.status] || "", u.status || "pending"];
  return `<p class="small faint">Version ${esc(t.plan_version)}. Statuses are the agent's own account.</p>
    <table class="t"><tbody>${units.map(u => { const [cls, label] = status(u); return `<tr><td class="mono small">${esc(u.id)}</td><td>${esc(u.goal)}${u.done_when ? `<div class="small muted">Done when ${esc(u.done_when)}</div>` : ""}</td><td><span class="chip ${cls}" data-unit-status="${esc(label)}">${esc(label)}</span></td></tr>`; }).join("")}</tbody></table>`;
}

function trace(tl) {
  const steps = tl.steps.map(s => `<div class="step"><div class="small faint">Step ${esc(s.step)}</div>
    ${s.text ? `<p>${esc(s.text)}</p>` : ""}
    ${s.calls.map(c => `<div class="call"><div><span class="mono">${esc(c.tool)}</span> <span class="small faint">issued by ${esc(c.issuer)}</span>
      ${(c.decisions || []).map(d => `<span class="chip ${d.outcome === "allow" || d.outcome === "dedupe" ? "ok" : d.outcome === "ask" ? "warn" : "bad"}"${d.reason || d.rule ? ` title="${esc([d.reason, d.rule].filter(Boolean).join(" · "))}"` : ""}>${esc(d.stage)}: ${esc(d.outcome)}</span>${d.reason ? ` <span class="small muted">${esc(d.reason)}</span>` : ""}${d.rule ? ` <span class="small faint mono">(${esc(d.rule)})</span>` : ""}`).join(" ")}</div>
      <details><summary class="small">Input${c.result !== null && c.result !== undefined ? " and result" : ""}</summary>${fieldValue(c.input)}${c.result !== null && c.result !== undefined ? `<div class="small muted" style="margin-top:6px">Result${c.is_error ? " (error)" : ""}</div><pre>${esc(c.result)}</pre>` : ""}</details></div>`).join("")}
  </div>`).join("");
  const notices = (tl.notices || []).map(n => `<div class="evidence none small"><span class="muted">Kernel, step ${esc(n.step)}:</span> ${esc(n.text)}</div>`).join("");
  return (steps || `<p class="empty">No steps yet.</p>`) + (notices ? `<h3>Kernel notices</h3>${notices}` : "");
}

// What the outcome page draws first: the deliverable the agent composed
// (the summary when it gave none), then each delivered file it does not
// show, in full.
function deliverable(t) {
  const view = t.outcome_view ? [...t.outcome_view] : t.summary && t.status !== "running" ? [{ type: "text", text: t.summary }] : [];
  const shown = new Set(view.filter(c => c.type === "file" || c.type === "image").map(c => c.file));
  for (const o of latestOutputs(t)) if (!shown.has(o.name)) view.push({ type: "file", file: o.name });
  return view;
}

