// The developer view: the same log as the operator view, read in full (V5).
// Every number links to the runs behind it, and every run to its log (V4).
"use strict";

const SRC_COLORS = ["#2f5bd3", "#0f766e", "#b45309", "#7c3aed", "#be123c", "#4d7c0f", "#0369a1", "#a16207", "#6b7280", "#c2410c"];
const FAILURE = {
  grader_disagrees: "grader disagrees", verification: "contract check failed", boundary: "denied at the boundary", provider: "model provider",
  agent: "agent gave up", budget: "budget", waiting_on_person: "waiting on a person", kill_switch: "kill switch", cancelled: "cancelled", stalled: "stalled", grader: "grader",
};
const n4 = v => v === null || v === undefined ? "–" : typeof v === "number" ? (Number.isInteger(v) ? v.toLocaleString() : v.toFixed(3)) : esc(v);
const pct = v => v === null || v === undefined ? "–" : `${(v * 100).toFixed(1)}%`;

// A drill-down hands a list of runs to the Runs view.
function drill(title, runs) {
  const key = String(Date.now());
  try { sessionStorage.setItem(`drill.${key}`, JSON.stringify({ title, runs })); } catch { /* fall back below */ }
  return `#/dev/runs?drill=${key}`;
}
function drillLink(title, runs, label) {
  if (!runs?.length) return `<span>${label}</span>`;
  return `<a href="#" data-drill="${esc(JSON.stringify({ title, runs }))}">${label}</a>`;
}
document.addEventListener("click", e => {
  const a = e.target.closest("[data-drill]");
  if (!a) return;
  e.preventDefault();
  const d = JSON.parse(a.dataset.drill);
  location.hash = drill(d.title, d.runs);
});

const devOnly = main => { if (!isDev()) { paint(main, `<p class="empty">This needs the developer role.</p>`); return true; } return false; };

// ---------- Runs ----------

route(/^#\/dev\/runs(?:\?(.*))?$/, async (main, q) => {
  if (devOnly(main)) return;
  const params = new URLSearchParams(q || "");
  let d = null;
  if (params.get("drill")) { try { d = JSON.parse(sessionStorage.getItem(`drill.${params.get("drill")}`)); } catch { d = null; } }
  let filter = { status: params.get("status") || "", failure: params.get("failure") || "", experiment: params.get("experiment") || "" };
  const draw = async () => {
    const qs = new URLSearchParams();
    for (const [k, v] of Object.entries(filter)) if (v) qs.set(k, v);
    if (d?.runs) qs.set("ids", d.runs.join(","));
    const { runs } = await api("GET", `/v1/dev/runs?${qs}`);
    paint(main, `<h1>Runs</h1>
      ${d ? `<div class="banner">Showing the ${d.runs.length} runs behind <strong>${esc(d.title)}</strong>. <a href="#/dev/runs">All runs</a></div>` : ""}
      <div class="row" style="margin:12px 0">
        <select data-f="status" aria-label="Status"><option value="">Any status</option>${Object.keys(STATUS).map(s => `<option ${filter.status === s ? "selected" : ""}>${s}</option>`).join("")}</select>
        <select data-f="failure" aria-label="Where it went wrong"><option value="">Any outcome</option>${Object.entries(FAILURE).map(([k, l]) => `<option value="${k}" ${filter.failure === k ? "selected" : ""}>${esc(l)}</option>`).join("")}</select>
        <input data-f="experiment" placeholder="Experiment id" value="${esc(filter.experiment)}" aria-label="Experiment">
        <span class="small faint">${runs.length} runs</span></div>
      <table class="t"><thead><tr><th>Task</th><th>Status</th><th>Where it went wrong</th><th>Configuration</th><th class="n">Calls</th><th class="n">Tokens in</th><th class="n">Cost</th><th class="n">Started</th></tr></thead>
      <tbody>${runs.map(r => `<tr class="click" data-href="#/dev/run/${enc(r.run)}"><td>${esc(r.title)}${r.experiment ? `<div class="small faint mono">${esc(r.experiment.id)} · ${esc(r.experiment.case)} · #${esc(r.experiment.trial)}</div>` : ""}</td>
        <td>${chip(r.status)}</td><td>${r.failure ? `<span class="chip bad">${esc(FAILURE[r.failure] || r.failure)}</span>` : ""}</td>
        <td class="small mono">${esc(r.config.adapter)}/${esc(r.config.model)}<div class="faint">${esc(r.config.instructions)} · pb v${esc(r.config.playbook)}</div></td>
        <td class="n">${r.model_calls}</td><td class="n">${n4(r.tokens.input + r.tokens.cache_read + r.tokens.cache_write)}</td><td class="n">$${r.usd.toFixed(3)}</td><td class="n small faint">${esc(when(r.created_at))}</td></tr>`).join("")}</tbody></table>`);
  };
  main.onclick = e => { const r = e.target.closest("[data-href]"); if (r) location.hash = r.dataset.href; };
  main.onchange = e => { const f = e.target.dataset.f; if (f) { filter[f] = e.target.value.trim(); draw(); } };
  await draw();
  every(5000, draw);
});

// ---------- Run inspector ----------

function sourcesBar(ctx) {
  const srcs = ctx.sources || [];
  const total = srcs.reduce((a, s) => a + (s.tokens || 0), 0) || 1;
  const color = i => SRC_COLORS[i % SRC_COLORS.length];
  return `<div class="stack" role="img" aria-label="Context by source">${srcs.map((s, i) => `<span style="width:${(100 * (s.tokens || 0)) / total}%;background:${color(i)}" title="${esc(s.source)}: ${s.tokens} tokens"></span>`).join("")}</div>
    <table class="t small" style="margin-top:6px"><tbody>${srcs.map((s, i) => `<tr><td><span style="display:inline-block;width:10px;height:10px;border-radius:2px;background:${color(i)};margin-right:8px"></span>${esc(s.source)}</td><td class="n">${n4(s.tokens)} tokens</td><td class="n faint">${n4(s.chars)} chars</td></tr>`).join("")}</tbody></table>
    <p class="small faint">${esc(ctx.token_basis)}. ${ctx.instructions_match_build ? "" : "Instructions differ from this build. "}${ctx.tools_match_now ? "" : "Tool definitions changed since this step."}</p>`;
}

function decisionChips(ds) {
  return (ds || []).map(d => `<span class="chip ${d.outcome === "allow" || d.outcome === "dedupe" ? "ok" : d.outcome === "ask" ? "warn" : "bad"}" title="${esc(d.reason || "")}${d.rule ? ` (${esc(d.rule)})` : ""}">${esc(d.stage)}: ${esc(d.outcome)}</span>`).join(" ");
}

route(/^#\/dev\/run\/([^/]+)$/, async (main, id) => {
  if (devOnly(main)) return;
  const draw = async () => {
    const r = await api("GET", `/v1/dev/runs/${enc(id)}`);
    const s = r.run;
    const byStep = {};
    for (const a of r.actions) { const st = String(a.step_id || "").match(/^s(\d+)\./)?.[1]; (byStep[st ?? "-"] ||= []).push(a); }
    paint(main, `<p class="small"><a href="#/dev/runs">Runs</a></p>
      <div class="spread"><h1 style="max-width:44ch">${esc(s.title)}</h1><div class="row">${chip(s.status)}${s.failure ? `<span class="chip bad">${esc(FAILURE[s.failure] || s.failure)}</span>` : ""}</div></div>
      <div class="kv" style="margin:12px 0">
        <div>Configuration</div><div class="mono small">${esc(s.config.key)}</div>
        <div>Person</div><div>${esc(s.person)}</div>
        <div>Model calls · actions</div><div class="num">${s.model_calls} · ${s.actions}</div>
        <div>Tokens</div><div class="num">${n4(s.tokens.input)} in · ${n4(s.tokens.output)} out · ${n4(s.tokens.cache_read)} cache read · ${n4(s.tokens.cache_write)} cache write</div>
        <div>Cost</div><div class="num">$${s.usd.toFixed(4)}</div>
        ${s.grades.length ? `<div>Graders</div><div>${s.grades.map(g => `<span class="chip ${g.passed ? "ok" : "bad"}">${esc(g.grader)}: ${g.passed ? "pass" : "fail"}${g.score !== null ? ` ${n4(g.score)}` : ""}</span>`).join(" ")}</div>` : ""}
      </div>
      <div class="row"><a href="#/dev/run/${enc(id)}/log">Full log (${r.events} events)</a><a href="#/task/${enc(s.task)}">As the person sees it</a></div>
      <h2>Steps</h2>
      ${r.steps.map(st => `<div class="card"><div class="spread"><strong>Step ${esc(st.step)}</strong><span class="small faint">${(st.attempts || []).length} attempt${(st.attempts || []).length === 1 ? "" : "s"}${st.reply?.served_model ? ` · ${esc(st.reply.served_model)}` : ""}</span></div>
        <h3>What the model saw</h3>${st.context ? sourcesBar(st.context) : `<p class="faint">No context recorded.</p>`}
        ${(st.attempts || []).filter(a => a.error).map(a => `<div class="evidence fail small">Attempt ${esc(a.attempt)}: ${esc(a.error.class)}: ${esc(a.error.message)}</div>`).join("")}
        <h3>What it did</h3>${st.reply ? `${st.reply.text ? `<p>${esc(st.reply.text)}</p>` : ""}
          ${(byStep[st.step] || []).map(a => `<div class="call"><span class="mono">${esc(a.tool)}</span> <span class="small faint">issued by ${esc(a.issuer || "model")}</span> ${decisionChips(a.decisions)}
            <details><summary class="small">Input and result</summary>${fieldValue(a.input)}${a.result ? `<div class="small muted" style="margin-top:6px">Result: ${esc(a.result.status)}${a.result.is_error ? " (error)" : ""}${a.result.label?.untrusted ? " · untrusted" : ""}${a.result.label?.private ? " · private" : ""}</div><pre>${esc(a.result.text)}</pre>` : `<p class="small faint">No result.</p>`}</details></div>`).join("")}
          ${st.reply.calls.filter(c => !r.actions.some(a => a.call_id === c.call_id)).map(c => `<div class="call"><span class="mono">${esc(c.tool)}</span> <span class="small faint">run record, issued by model</span></div>`).join("")}
          <p class="small faint">${st.reply.usage ? `${n4(st.reply.usage.input_tokens)} in · ${n4(st.reply.usage.output_tokens)} out · stop: ${esc(st.reply.stop)}` : ""}</p>` : `<p class="faint">No reply.</p>`}</div>`).join("")}
      <h2>Records</h2>
      ${r.records.length ? `<table class="t"><tbody>${r.records.map(x => `<tr><td class="n faint">${x.seq}</td><td>${esc(x.kind)}${x.version ? ` v${x.version}` : ""}</td><td class="small faint">${esc(x.issuer)}</td><td><details><summary class="small">Body</summary>${fieldValue(x.body || x.result)}</details></td></tr>`).join("")}</tbody></table>` : `<p class="empty">None.</p>`}
      <h2>What the harness did</h2>
      ${r.harness.length ? `<table class="t"><thead><tr><th class="n">Seq</th><th>Event</th><th>Issued by</th><th>Detail</th></tr></thead><tbody>${r.harness.map(x => `<tr><td class="n faint">${x.seq}</td><td>${esc(x.kind)}</td><td class="small">${esc(x.issuer)}</td><td class="small">${x.criterion ? `${esc(x.criterion)}: ${x.passed ? "holds" : "does not hold"}` : esc(typeof x.text === "string" ? x.text : JSON.stringify(x.text ?? ""))}</td></tr>`).join("")}</tbody></table>` : `<p class="empty">Nothing.</p>`}`);
  };
  await draw();
  every(4000, draw);
});

route(/^#\/dev\/run\/([^/]+)\/log$/, async (main, id) => {
  if (devOnly(main)) return;
  let rows = [], total = 0;
  const more = async () => {
    const after = rows.length ? rows[rows.length - 1].seq : 0;
    const r = await api("GET", `/v1/dev/runs/${enc(id)}/log?after=${after}&limit=200`);
    rows = rows.concat(r.events); total = r.total;
    paint(main, `<p class="small"><a href="#/dev/run/${enc(id)}">Run</a></p><h1>Log</h1><p class="muted">${rows.length} of ${total} events, in order. Large attachments are shortened here; the log keeps them.</p>
      <table class="t"><thead><tr><th class="n">Seq</th><th>Time</th><th>Kind</th><th>Actor</th><th>Body</th></tr></thead><tbody>
      ${rows.map(e => `<tr><td class="n faint">${e.seq}</td><td class="small faint">${esc(new Date(e.at).toLocaleTimeString())}</td><td class="mono small">${esc(e.kind)}</td><td class="small">${esc(e.actor)}</td><td><details><summary class="small">body</summary><pre>${esc(JSON.stringify(e.body, null, 2))}</pre></details></td></tr>`).join("")}</tbody></table>
      ${rows.length < total ? `<div class="row" style="margin-top:12px"><button data-act="more">Load more</button></div>` : ""}`);
  };
  main.onclick = e => { if (e.target.closest("[data-act=more]")) more(); };
  await more();
});

// ---------- Experiments ----------

route(/^#\/dev\/experiments$/, async main => {
  if (devOnly(main)) return;
  const draw = async () => {
    const { experiments } = await api("GET", "/v1/dev/experiments");
    paint(main, `<h1>Experiments</h1><p class="muted">Configurations compared across repeated trials, task by task. Trials come from the evaluation runner.</p>
      ${experiments.length ? `<table class="t"><thead><tr><th>Experiment</th><th class="n">Cases</th><th class="n">Trials</th><th class="n">Settled</th><th>Configurations</th><th class="n">Started</th></tr></thead><tbody>
      ${experiments.map(x => `<tr class="click" data-href="#/dev/experiment/${enc(x.id)}"><td class="mono">${esc(x.id)}</td><td class="n">${x.cases}</td><td class="n">${x.trials}</td><td class="n">${x.settled}</td><td class="small mono">${x.configs.map(esc).join("<br>")}</td><td class="n small faint">${esc(when(x.started))}</td></tr>`).join("")}</tbody></table>` : `<p class="empty">No experiments yet. Run <span class="mono">python3 eval/plain/run.py</span>.</p>`}`);
  };
  main.onclick = e => { const r = e.target.closest("[data-href]"); if (r) location.hash = r.dataset.href; };
  await draw();
  every(10000, draw);
});

// A request set run in chat: each request against each model, what the
// model chose, what stopped it, and what it cost; the person gives each
// request a verdict (right, partly, wrong) on its answer, and a rerun is
// compared with the run before it.
const CHOICE = { answer: "Answered", asked: "Asked back", task: "Started tasks", schedule: "Proposed a schedule" };
const secs = s => s === null || s === undefined ? "–" : s < 90 ? `${Math.round(s)} s` : s < 5400 ? `${Math.round(s / 60)} min` : `${(s / 3600).toFixed(1)} h`;
const usd = v => v === null || v === undefined ? "–" : `$${v < 0.1 ? v.toFixed(3) : v.toFixed(2)}`;
const median = xs => { const v = xs.slice().sort((a, b) => a - b); return v.length ? (v.length % 2 ? v[(v.length - 1) / 2] : (v[v.length / 2 - 1] + v[v.length / 2]) / 2) : null; };
const modelOf = key => key.split(" · ")[0];
const VERDICT_WORDS = [["right", "right"], ["partly", "partly"], ["wrong", "wrong"]];
const CHANGED = { verdict: "verdict", choice: "choice", stops: "policy stops", tasks: "tasks", setup: "instructions or harness" };
const signed = (v, f) => v === null || v === undefined ? "–" : `${v > 0 ? "+" : v < 0 ? "−" : "±"}${f(Math.abs(v))}`;

// This run of the requests against the run before: one summary once every
// trial has settled, then each request on each model side by side with what
// changed, regressions first.
function comparison(x, experiments) {
  const c = x.compare;
  if (!c) return "";
  const side = r => `<div><span class="chip ${r.verdict === "right" ? "ok" : r.verdict === "wrong" ? "bad" : r.verdict ? "warn" : ""}">${esc(r.verdict || "no verdict")}</span>
    <span class="small">${esc(CHOICE[r.choice] || r.choice)} · ${r.stops.length} stops · ${usd(r.usd)} · ${secs(r.seconds)}</span></div>`;
  const order = { regressed: 0, improved: 1, unjudged: 2, unchanged: 3 };
  const pairs = c.pairs.slice().sort((a, b) => order[a.outcome] - order[b.outcome] || b.changed.length - a.changed.length);
  const others = experiments.filter(e => e.id !== x.id);
  return `<h2>Against an earlier run</h2>
    <div class="row"><label class="small muted" for="base">Compared with</label> <select id="base">${others.map(e => `<option value="${esc(e.id)}"${e.id === c.base ? " selected" : ""}>${esc(e.id)}</option>`).join("")}</select></div>
    ${c.complete
      ? `<div class="grid" data-compare-summary style="margin:12px 0">${[["improved", "Improved", "ok"], ["unchanged", "Unchanged", ""], ["regressed", "Regressed", "bad"]].map(([k, w, cls]) => `<div><div class="small muted">${w}</div><div class="num ${cls}" style="font-size:22px;font-weight:650" data-${k}>${c.summary[k]}</div></div>`).join("")}</div>
        ${c.summary.unjudged ? `<p class="small faint">${c.summary.unjudged} not judged yet: a verdict is missing on one side.</p>` : ""}`
      : `<p class="muted" data-compare-waiting>${c.settled} of ${c.trials} trials have settled; the summary shows when the rerun completes.</p>`}
    <table class="t" data-compare><thead><tr><th>Request</th><th>Model</th><th>${esc(c.base)}</th><th>${esc(x.id)}</th><th>Changed</th></tr></thead><tbody>
    ${pairs.map(p => `<tr data-pair="${esc(p.case)}" data-outcome="${esc(p.outcome)}"><td class="small">${esc(p.text || p.case)}</td><td class="mono small">${esc(p.model)}</td>
      <td>${side(p.before)}</td><td>${side(p.after)}</td>
      <td class="small">${p.outcome !== "unchanged" && p.outcome !== "unjudged" ? `<span class="chip ${p.outcome === "improved" ? "ok" : "bad"}">${esc(p.outcome)}</span> ` : ""}${p.changed.map(f => `<span class="chip warn" data-changed="${esc(f)}">${esc(CHANGED[f] || f)}</span>`).join(" ")}
        <div class="faint">cost ${signed(p.usd_change, usd)} · time ${signed(p.seconds_change, secs)}</div></td></tr>`).join("")}</tbody></table>`;
}

function requestSet(x) {
  const configs = [...new Set(x.requests.map(r => r.config))];
  const cases = [...new Set(x.requests.map(r => r.case))].sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
  const summary = configs.map(c => {
    const rs = x.requests.filter(r => r.config === c);
    const judged = rs.filter(r => r.verdict);
    const choices = Object.entries(rs.reduce((m, r) => ({ ...m, [r.choice]: (m[r.choice] || 0) + 1 }), {}));
    return `<tr><td class="mono small">${esc(modelOf(c))}</td>
      <td class="n">${judged.length ? VERDICT_WORDS.map(([k, w]) => `${judged.filter(r => r.verdict === k).length} ${w}`).join(" · ") : "–"}${judged.length < rs.length ? ` <span class="faint small">(${rs.length - judged.length} to judge)</span>` : ""}</td>
      <td class="small">${choices.map(([k, n]) => `${esc(CHOICE[k] || k)} ${n}`).join(" · ")}</td>
      <td class="n">${rs.reduce((n, r) => n + r.stops.length, 0)} <span class="faint small">in ${rs.filter(r => r.stops.length).length}</span></td>
      <td class="n">${usd(rs.reduce((n, r) => n + (r.usd || 0), 0))}</td>
      <td class="n">${secs(median(rs.map(r => r.seconds)))}</td>
      <td class="n">${rs.filter(r => r.settled).length}/${rs.length}</td></tr>`;
  }).join("");
  const row = r => `<tr data-request="${esc(r.case)}" data-config="${esc(r.config)}"><td class="mono small">${esc(modelOf(r.config))}</td>
    <td><span class="chip ${r.choice === "task" ? "run" : r.choice === "schedule" ? "warn" : ""}">${esc(CHOICE[r.choice] || r.choice)}</span>
      ${r.tasks.length ? `<div class="small">${r.tasks.map(t => `${esc(t.title)} <span class="faint">${esc(t.status)}${t.reason ? `: ${esc(t.reason)}` : ""}</span>`).join("<br>")}</div>` : ""}
      ${r.answer ? `<details data-key="answer:${esc(r.run)}"><summary class="small">What it said</summary><div class="small" style="white-space:pre-wrap">${esc(r.answer)}</div></details>` : ""}</td>
    <td class="n">${r.stops.length ? `<span title="${esc(r.stops.map(s => `${s.tool}: ${s.outcome} at ${s.stage}${s.rule ? ` (${s.rule})` : ""}`).join("\n"))}">${r.stops.length}</span>` : "0"}</td>
    <td class="n">${usd(r.usd)}</td><td class="n">${secs(r.seconds)}${r.settled ? "" : ` <span class="chip run">running</span>`}</td>
    <td>${r.settled ? verdictBar(r.task, r.verdict_from === "answer" ? { verdict: r.verdict, note: r.note } : null) : ""}
      ${r.verdict_from === "tasks" ? `<div class="small muted">From its tasks: ${esc(r.verdict)}${r.note ? ` · ${esc(r.note)}` : ""}</div>` : ""}
      <a class="small" href="#/dev/run/${enc(r.run)}">run</a></td></tr>`;
  return `<h2>By model</h2><table class="t" data-summary><thead><tr><th>Model</th><th class="n">Verdicts</th><th>Choices</th><th class="n">Policy stops</th><th class="n">Cost</th><th class="n">Median time</th><th class="n">Settled</th></tr></thead><tbody>${summary}</tbody></table>
    <p class="small faint">Time runs from the message to the moment the reply and every task it started ended or stopped for a person. Policy stops are actions the boundary asked about or refused, in the reply and its tasks.</p>
    <h2>By request</h2>${cases.map(c => {
      const rs = x.requests.filter(r => r.case === c);
      return `<div class="card" data-case="${esc(c)}"><div style="font-weight:600">${esc(rs[0].text)}</div>
        <table class="t req"><thead><tr><th>Model</th><th>Choice</th><th class="n">Stops</th><th class="n">Cost</th><th class="n">Time</th><th>Verdict</th></tr></thead><tbody>${rs.map(row).join("")}</tbody></table></div>`;
    }).join("")}`;
}

route(/^#\/dev\/experiment\/([^/?]+)(?:\?(.*))?$/, async (main, id, query) => {
  if (devOnly(main)) return;
  const qs = new URLSearchParams(query || ""), k = qs.get("k"), base = qs.get("base");
  main.onchange = e => { if (e.target.id === "base") location.hash = `#/dev/experiment/${enc(id)}?base=${enc(e.target.value)}`; };
  const draw = async () => {
    const want = new URLSearchParams();
    if (k) want.set("k", k);
    if (base) want.set("base", base);
    const x = await api("GET", `/v1/dev/experiments/${enc(id)}${want.size ? `?${want}` : ""}`);
    if (x.requests?.length) {
      const { experiments } = x.compare ? await api("GET", "/v1/dev/experiments") : { experiments: [] };
      paint(main, `<p class="small"><a href="#/dev/experiments">Experiments</a></p><h1 class="mono">${esc(x.id)}</h1>
        <p class="muted">${x.cases.length} requests, each sent as a chat message to every model. Give each answer a verdict: right, partly or wrong.</p>${comparison(x, experiments)}${requestSet(x)}`);
      return;
    }
    const metric = (m, fmt) => `${fmt(m.mean)}${m.se !== null ? ` <span class="faint">± ${fmt(m.se)}</span>` : ""}`;
    paint(main, `<p class="small"><a href="#/dev/experiments">Experiments</a></p><h1 class="mono">${esc(x.id)}</h1>
      <p class="muted">${x.cases.length} cases. k = ${x.k} <span class="small">(pass@k: at least one of k trials passes; pass^k: all k pass; ± one standard error over cases)</span></p>
      <div class="row">${[1, 2, 3, 5].map(n => `<a href="#/dev/experiment/${enc(id)}?k=${n}" class="chip ${x.k === n ? "run" : ""}">k=${n}</a>`).join("")}</div>
      ${x.configs.map(c => `<div class="card"><div class="mono small">${esc(c.key)}</div>
        <div class="grid" style="margin:12px 0">
          <div><div class="small muted">Pass rate</div><div class="num" style="font-size:22px;font-weight:650">${metric(c.pass_rate, pct)}</div></div>
          <div><div class="small muted">pass@${x.k}</div><div class="num" style="font-size:22px;font-weight:650">${metric(c.pass_at_k, pct)}</div></div>
          <div><div class="small muted">pass^${x.k}</div><div class="num" style="font-size:22px;font-weight:650">${metric(c.pass_hat_k, pct)}</div></div>
        </div>
        <p class="small faint">Verdicts from ${c.verdict_basis.map(b => b === "grader" ? "graders" : "the kernel's own checks").join(" and ")}.</p>
        <h3>Where trials failed</h3>${c.failures.length ? `<div class="row">${c.failures.map(f => `<span class="chip bad">${drillLink(`${FAILURE[f.where] || f.where} in ${c.key}`, f.runs, `${esc(FAILURE[f.where] || f.where)}: ${f.count}`)}</span>`).join("")}</div>` : `<p class="small faint">No failed trials.</p>`}
        <h3>By case</h3><table class="t"><thead><tr><th>Case</th><th class="n">Trials</th><th class="n">Passed</th><th class="n">pass@${x.k}</th><th class="n">pass^${x.k}</th></tr></thead><tbody>
        ${c.cases.map(r => `<tr><td class="mono small">${esc(r.case)}</td><td class="n">${drillLink(`${r.case} in ${c.key}`, r.runs, `${r.settled}/${r.trials}`)}</td><td class="n">${drillLink(`passing trials of ${r.case}`, r.passed_runs, String(r.passed))}</td><td class="n">${pct(r.pass_at_k)}</td><td class="n">${pct(r.pass_hat_k)}</td></tr>`).join("")}</tbody></table></div>`).join("")}
      ${x.paired.length ? `<h2>Paired comparison</h2>${x.paired.map(p => `<div class="card"><div class="small mono">${esc(p.other)}</div><div class="small faint">against ${esc(p.base)}</div>
        <p style="margin-top:8px">Mean difference in pass rate over ${p.cases} shared cases: <strong class="num">${p.mean_difference === null ? "–" : `${p.mean_difference >= 0 ? "+" : ""}${pct(p.mean_difference)}`}</strong>${p.se !== null ? ` <span class="faint">± ${pct(p.se)}</span>` : ""}. Better on ${p.better}, worse on ${p.worse}, same on ${p.same}.</p>
        <table class="t small"><tbody>${p.by_case.map(c => `<tr><td class="mono">${esc(c.case)}</td><td class="n">${c.difference > 0 ? "+" : ""}${pct(c.difference)}</td></tr>`).join("")}</tbody></table></div>`).join("")}` : ""}`);
  };
  App.redraw = draw;
  onLeave(() => { App.redraw = null; });
  await draw();
  every(10000, draw);
});

// ---------- System ----------

route(/^#\/dev\/system$/, async main => {
  if (devOnly(main)) return;
  const draw = async () => {
    const m = await api("GET", "/v1/dev/metrics");
    const value = p => p.unavailable ? `<div class="small muted">${esc(p.unavailable)}</div>` : `<div class="v">${p.runs?.length ? drillLink(p.title, p.runs, n4(p.value)) : n4(p.value)}</div><div class="u">${esc(p.unit)}</div>`;
    paint(main, `<h1>System</h1><p class="muted">Live, from the log. ${m.runs} runs. Click a number to see the runs behind it.</p>
      <div class="grid" style="grid-template-columns:repeat(auto-fill,minmax(270px,1fr))">${m.panels.map(p => `<div class="card panel" data-panel="${esc(p.id)}"><div class="small muted" style="font-weight:600">${esc(p.title)}</div>${value(p)}
        ${p.breakdown?.length ? `<table class="t small" style="margin-top:8px"><tbody>${p.breakdown.map(b => `<tr><td>${esc(b.label)}</td><td class="n">${b.runs?.length ? drillLink(`${p.title}: ${b.label}`, b.runs, n4(b.value)) : n4(b.value)}</td></tr>`).join("")}</tbody></table>` : ""}</div>`).join("")}</div>`);
  };
  await draw();
  every(5000, draw);
});

// ---------- Policy ----------

route(/^#\/dev\/policy$/, async main => {
  if (devOnly(main)) return;
  const picked = {};
  const draw = async () => {
    const p = await api("GET", "/v1/dev/policy");
    paint(main, `<h1>Policy</h1><p class="muted">Each layer's versions, who owns it, and what the agent has proposed. Only the owner of a layer can accept a proposal for it.</p>
      <h2>Proposals</h2>${p.proposals.length ? p.proposals.map(x => `<div class="card"><div class="spread"><span><span class="mono">${esc(x.layer)}</span> · ${x.status === "waiting" ? `<span class="chip warn">waiting for ${esc(x.waiting_for.join(", ") || "nobody holds this role")}</span>` : `<span class="chip">${esc(x.decision?.decision)} by ${esc(x.decision?.by)}</span>`}</span>
        <span class="small faint">${x.run ? `<a href="#/dev/run/${enc(x.run)}">from run</a> · ` : ""}${esc(when(x.created_at))}</span></div>
        ${x.rationale ? `<p>${esc(x.rationale)}</p>` : ""}<div class="what"><pre>${esc(x.cedar)}</pre></div></div>`).join("") : `<p class="empty">The agent has proposed nothing.</p>`}
      <h2>Layers</h2>${p.layers.map(l => {
        const v = picked[l.layer] ?? l.versions.length - 1;
        const shown = v >= 0 && l.versions[v] ? l.versions[v].cedar : l.current;
        return `<div class="card"><div class="spread"><strong class="mono">${esc(l.layer)}</strong><span class="small muted">owned by ${esc(l.owners.join(", ") || "no one")}</span></div>
          <div class="row" style="margin:8px 0">${l.default ? `<span class="chip">built-in default</span>` : ""}${l.versions.map((x, i) => `<button class="${i === v ? "primary" : ""}" data-layer="${esc(l.layer)}" data-v="${i}" title="${esc(new Date(x.at).toLocaleString())} by ${esc(x.by)}">v${i + 1}</button>`).join("")}</div>
          ${v >= 0 && l.versions[v] ? `<p class="small faint">v${v + 1}, set ${esc(when(l.versions[v].at))} by ${esc(l.versions[v].by)}</p>` : ""}<div class="what"><pre>${esc(shown || "(empty)")}</pre></div></div>`;
      }).join("")}
      <h2>Measured reliability by tool</h2><p class="small muted">How each tool's calls were decided, how often they errored, and how the runs that used it ended.</p>
      <table class="t"><thead><tr><th>Tool</th><th class="n">Allowed</th><th class="n">Asked</th><th class="n">Denied</th><th class="n">Calls</th><th class="n">Call success</th><th class="n">Runs completed</th></tr></thead><tbody>
      ${p.scopes.map(s => `<tr><td class="mono small">${esc(s.tool)}</td><td class="n">${s.decided.allow}</td><td class="n">${s.decided.ask}</td><td class="n">${s.decided.deny}</td><td class="n">${s.calls}</td><td class="n">${pct(s.call_success)}</td><td class="n">${drillLink(`runs using ${s.tool}`, s.runs, pct(s.runs_completed))}</td></tr>`).join("")}</tbody></table>`);
  };
  main.onclick = e => { const b = e.target.closest("button[data-layer]"); if (b) { picked[b.dataset.layer] = Number(b.dataset.v); draw(); } };
  await draw();
  every(10000, draw);
});

// ---------- Playbook ----------

route(/^#\/dev\/playbook$/, async main => {
  if (devOnly(main)) return;
  const draw = async () => {
    const p = await api("GET", "/v1/dev/playbook");
    const items = p.items.slice().sort((a, b) => (b.added_at || "").localeCompare(a.added_at || ""));
    paint(main, `<h1>Playbook</h1><p class="muted">What the agent wrote down for itself, per person. Version ${p.version}. An item learned while reading untrusted content carries that label into every run that gets it.</p>
      ${items.length ? `<table class="t"><thead><tr><th>Item</th><th>Label</th><th class="n">Completed with it</th><th class="n">Completed before it</th></tr></thead><tbody>
      ${items.map(i => `<tr><td>${esc(i.text)}<div class="small muted">Addresses: ${esc(i.addresses)}</div><div class="small faint">${esc(i.person)} · v${i.added}${i.retired ? `, retired at v${i.retired}` : ""} · ${i.source_run ? `<a href="#/dev/run/${enc(i.source_run)}">from run</a>` : ""}</div></td>
        <td>${i.label?.untrusted ? `<span class="chip warn">untrusted</span>` : ""}${i.label?.private ? `<span class="chip">private</span>` : ""}${!i.label?.untrusted && !i.label?.private ? `<span class="chip ok">principal</span>` : ""}</td>
        <td class="n">${drillLink(`runs with ${i.id}`, i.with_item.runs, `${pct(i.with_item.rate)} <span class="faint">(${i.with_item.completed}/${i.with_item.runs.length})</span>`)}</td>
        <td class="n">${drillLink(`runs before ${i.id}`, i.before_item.runs, `${pct(i.before_item.rate)} <span class="faint">(${i.before_item.completed}/${i.before_item.runs.length})</span>`)}</td></tr>`).join("")}</tbody></table>` : `<p class="empty">No items yet.</p>`}`);
  };
  await draw();
  every(10000, draw);
});
