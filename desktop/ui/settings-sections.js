// Model provider setup, notifications, updates, grants, commitments, and data settings sections.

// ---------- Models ----------

const signins = {};
/** Connecting the server's model providers: keys and sign-ins to the vault,
 * and the conformance suite. Admin › Models draws it (admin-spend.js) with
 * `connectOnly`, below its own default and rules; every change it makes is
 * an admin change, so the server asks for the password again when due.
 * @param {HTMLElement} main @param {{connectOnly?: boolean}} [opts] */
async function modelsSection(main, opts = {}) {
  let pick = null;
  const catalogs = {};
  const catalog = async a => {
    const c=await Resources.read(`Models for ${a}`,`/v1/adapters/${enc(a)}/catalog`,undefined,true);
    if(c)catalogs[a]=c.models ? {ids:c.models.map(m=>m.id)} : {error:c.message || "the adapter sent no list"};
    return catalogs[a] || {error:"The provider is unavailable. Retrying when connected."};
  };
  const draw = async () => {
    const [providers, cur] = await Promise.all([Resources.read("Providers", "/v1/adapters",undefined,true), Resources.read("Model", "/v1/model",undefined,true)]);
    if(!providers || !cur){if(!main._html)paint(main,'<h2>Models</h2><p class="empty">Models are unavailable. Retrying when connected.</p>');return;}
    const {adapters}=providers;
    const p = pick || cur.adapter, models = await catalog(p);
    const chosen = p === cur.adapter ? cur.model : null;
    paint(main, `${opts.connectOnly ? `<h3>Connect providers</h3>` : `<h2>Models</h2>`}<p class="muted">Connect providers here. Keys and sign-ins go to the server's vault; this app keeps none.</p>
      ${opts.connectOnly ? "" : `<div class="card"><div class="small muted">New tasks run on</div><div class="mono" style="font-size:16px;margin:2px 0 10px">${esc(cur.adapter)} · ${esc(cur.model)}</div>
        <div class="row"><select id="madapter" aria-label="Provider" data-draft="model-provider">${adapters.map(a => `<option ${a.id === p ? "selected" : ""}>${esc(a.id)}</option>`).join("")}</select>
        ${models.ids ? `<select id="mname" aria-label="Model" data-draft="model-choice-${esc(p)}">${chosen && !models.ids.includes(chosen) ? `<option selected>${esc(chosen)}</option>` : ""}${models.ids.map(m => `<option ${m === chosen ? "selected" : ""}>${esc(m)}</option>`).join("")}</select><button class="primary" data-act="use">Use this model</button>` : ""}</div>
        ${models.error ? `<p class="small muted" style="margin-top:8px">No model list from ${esc(p)} yet: ${esc(models.error)}</p>` : ""}</div>`}
      ${adapters.map(a => `<div class="card" data-adapter="${esc(a.id)}"><div class="spread"><strong>${esc(a.id)}</strong>
          <span class="row">${a.conformance ? `<span class="chip ${a.conformance.passed ? "ok" : "bad"}">suite ${a.conformance.passed ? "passed" : "failed"} · ${esc(a.conformance.model)}</span>` : `<span class="chip warn">suite not run</span>`}</span></div>
        <div class="row" style="margin-top:10px"><button data-act="signin" data-adapter="${esc(a.id)}">Connect</button><button data-act="suite" data-adapter="${esc(a.id)}">Run the conformance suite</button></div>
        <div id="si-${esc(a.id)}">${signins[a.id] || ""}</div><div id="suite-${esc(a.id)}"></div></div>`).join("")}`);
    const ma = $("#madapter", main);
    if (ma) ma.onchange = () => { pick = ma.value; draw(); };
  };
  const show = (a, html) => { signins[a] = html; const el = $(`#si-${CSS.escape(a)}`, main); if (el) el.innerHTML = html; };
  const finish = async (a, step, input) => {
    try {
      await Server.models.completeSignIn(a, { flow: step.flow, input });
      delete catalogs[a];
      show(a, `<p class="small" style="color:var(--ok)">Connected.</p>`);
      draw();
    } catch (e) { show(a, `<p class="small" style="color:var(--bad)">${esc(e.message)}</p>`); }
  };
  main.onclick = async e => {
    const b = e.target.closest("button[data-act]"); if (!b) return;
    const a = b.dataset.adapter;
    try {
      if (b.dataset.act === "use") { await Server.models.choose($("#madapter").value, $("#mname").value); toast("New tasks will use this model."); draw(); }
      if (b.dataset.act === "signin") {
        b.disabled = true;
        const step = await Server.models.signIn(a);
        b.disabled = false;
        if (step.event === "error") { show(a, `<p class="small" style="color:var(--bad)">${esc(step.message)}</p>`); return; }
        const head = `<div style="margin-top:12px"><p class="small">${esc(step.instructions)}</p>`;
        if (step.flow?.kind === "api_key") {
          show(a, `${head}<label for="key-${esc(a)}">API key</label><input type="password" id="key-${esc(a)}" data-ephemeral style="width:100%" autocomplete="off">
            <div class="row" style="margin-top:8px"><button class="primary" data-act="key" data-adapter="${esc(a)}">Save to the vault</button></div></div>`);
          signins[`${a}:step`] = step;
        } else {
          show(a, `${head}${step.user_code ? `<p>Code: <span class="mono" style="font-size:18px">${esc(step.user_code)}</span></p>` : ""}
            ${step.url ? `<div class="row"><button class="primary" data-act="open" data-url="${esc(step.url)}">Open the sign-in page</button></div>` : ""}
            <p class="small muted">Waiting for you to approve on the provider's site…</p></div>`);
          finish(a, step, {});
        }
      }
      if (b.dataset.act === "open") await Magnus.openExternal(b.dataset.url);
      if (b.dataset.act === "key") {
        const v = $(`#key-${CSS.escape(a)}`, main).value.trim();
        if (!v) return toast("Paste the key first.");
        b.disabled = true;
        await finish(a, signins[`${a}:step`], { api_key: v });
      }
      if (b.dataset.act === "suite") {
        const el = $(`#suite-${CSS.escape(a)}`, main);
        b.disabled = true; el.innerHTML = `<p class="small muted">Running the suite against the live provider…</p>`;
        const r = await Server.models.conformance(a);
        el.innerHTML = `<table class="t small" style="margin-top:8px"><tbody>${r.checks.map(c => `<tr><td>${c.passed ? `<span class="chip ok">pass</span>` : `<span class="chip bad">fail</span>`}</td><td>${esc(c.name)}${c.simulated ? " <span class='faint'>(simulated)</span>" : ""}</td><td class="muted">${esc(c.detail)}</td></tr>`).join("")}</tbody></table>`;
        b.disabled = false; main._html = null;
      }
    } catch (err) { toast(err.message); b.disabled = false; }
  };
  App.redraw=draw;onLeave(()=>{if(App.redraw===draw)App.redraw=null;});
  await draw();
}


// Notifications follow the person to every device: the choices live on the appliance.
const NOTIFY = [["notify_blocking", "A task needs me now", "An approval, a question, or a review a task is waiting on."],
  ["notify_batched", "Something can wait for me", "Results to look at and proposals: decisions no task is waiting on."],
  ["notify_agent_changes", "The agent changes", "A new model, instructions, harness or policy."]];
/** This device's own choice: whether what pops up in the app also goes to
 * the system's notifications while Magnus is in the background. */
function systemNotificationsCard() {
  const allowed = Magnus.notifyPermission?.() || "unsupported";
  const why = allowed === "denied" ? "Your browser blocks notifications from this site. Allow them in its site settings, then turn this on."
    : allowed === "unsupported" ? "This browser cannot show notifications." : "Who wrote and where, never what a message says. Messages, mentions and what needs you also pop up in the app's lower corner.";
  return `<h3>On this device</h3><div class="card"><label class="row toggle"><input type="checkbox" data-system-notifications ${Notices.systemOn() ? "checked" : ""}${allowed === "unsupported" ? " disabled" : ""}>
    <span><span>Show them in ${Magnus.kind === "web" ? "this computer's" : "the system's"} notifications while Magnus is in the background</span><span class="small muted" style="display:block">${esc(why)}</span></span></label></div>`;
}

async function notificationsSection(el) {
  const draw = async () => {
    App.prefs = await Server.preferences.get();
    const wh = App.prefs.working_hours || { start: "09:00", end: "17:00" };
    const qh = App.prefs.quiet_hours || { start: "22:00", end: "07:00" };
    const budget = App.prefs.interruptions_per_day ?? 5;
    paint(el, `<h2>Notifications</h2><p class="muted">Magnus tells this computer when…</p>
      <div class="card">${NOTIFY.map(([k, label, hint]) => `<label class="row toggle"><input type="checkbox" data-pref="${k}" ${App.prefs[k] ? "checked" : ""}>
        <span><span>${esc(label)}</span><span class="small muted" style="display:block">${esc(hint)}</span></span></label>`).join("")}</div>
      <h3>Hours and interruption budget</h3>
      <div class="card" data-hours-card>
        <p class="small muted">Morning briefings run at the start of working hours. During quiet hours, non-urgent proactive notices wait until morning.</p>
        <div class="row" style="margin-top:8px;flex-wrap:wrap">
          <label class="small">Working hours <input type="time" data-hours="working_hours" data-edge="start" value="${esc(wh.start || "09:00")}"> to <input type="time" data-hours="working_hours" data-edge="end" value="${esc(wh.end || "17:00")}"></label>
          <label class="small">Quiet hours <input type="time" data-hours="quiet_hours" data-edge="start" value="${esc(qh.start || "22:00")}"> to <input type="time" data-hours="quiet_hours" data-edge="end" value="${esc(qh.end || "07:00")}"></label>
          <label class="small">Daily interruption limit <input type="number" min="1" max="50" data-pref-num="interruptions_per_day" value="${esc(String(budget))}" style="width:5em"></label>
        </div>
      </div>
      ${systemNotificationsCard()}`);
  };
  el.onchange = async e => {
    if (e.target.closest("input[data-system-notifications]")) {
      if (e.target.checked && Magnus.notifyPermission?.() !== "granted") await Notices.turnOn(); else Notices.setSystem(e.target.checked);
      return draw();
    }
    const h = e.target.closest("input[data-hours]");
    if (h) {
      const kind = h.dataset.hours;
      const start = $(`input[data-hours="${kind}"][data-edge="start"]`, el)?.value || "09:00";
      const end = $(`input[data-hours="${kind}"][data-edge="end"]`, el)?.value || "17:00";
      try { App.prefs = await Server.preferences.set({ [kind]: { start, end } }); toast("Saved."); } catch (err) { toast(err.message); }
      return;
    }
    const num = e.target.closest("input[data-pref-num]");
    if (num) {
      const v = Math.max(1, Number(num.value) || 1);
      try { App.prefs = await Server.preferences.set({ [num.dataset.prefNum]: v }); toast("Saved."); } catch (err) { toast(err.message); }
      return;
    }
    const c = e.target.closest("input[data-pref]"); if (!c) return;
    try { App.prefs = await Server.preferences.set({ [c.dataset.pref]: c.checked }); toast("Saved."); } catch (err) { toast(err.message); c.checked = !c.checked; }
  };
  await draw();
}

async function updatesSection(el) {
  const draw = async () => {
    // The server's own updates (install, roll back, channel) are Admin ›
    // Operations'; here, only what it runs and what changed.
    const [st, notes, ver] = await Promise.all([Magnus.updateState().catch(() => null), Magnus.whatsNew(true).catch(() => null), Server.version()]);
    const para = releaseNotesHtml;
    paint(el, `<h2>Updates</h2>
      ${st ? `<div class="card" data-app-version><div class="spread"><div><strong>This app</strong> <span class="mono">${esc(st.current)}</span></div>
          ${st.ready ? `<button class="primary" data-act="restart-update">Restart to update</button>` : `<button data-act="look">Check now</button>`}</div>
        <div class="small muted">${st.ready ? `Magnus ${esc(st.ready.version)} is downloaded and checked.` : st.error ? `The last look failed: ${esc(st.error)}` : st.can_update ? esc(st.can_update) : "Up to date."}</div>
        ${st.ready ? `<h3>What's new in ${esc(st.ready.version)}</h3><div class="notes">${para(st.ready.notes)}</div>` : ""}
        ${notes ? `<h3>What's new in ${esc(notes.version)}</h3><div class="notes">${para(notes.notes)}</div>` : ""}</div>` : ""}
      <div class="card" data-appliance-version><div class="spread"><div><strong>The server</strong> <span class="mono">${esc(ver.version)}</span></div></div>
        ${administers() ? `<div class="small muted">Install, roll back and the update channel are in <a href="#/admin/operations">Admin › Operations</a>.</div>` : ""}</div>`);
  };
  // A look that ends while this is showing (a download finishing) shows here
  // too, not only in the sidebar.
  App.redrawUpdates = () => el.isConnected ? draw() : (App.redrawUpdates = null);
  el.onclick = async e => {
    const b = e.target.closest("button[data-act]"); if (!b) return;
    try {
      if (b.dataset.act === "restart-update") return restartToUpdate();
      if (b.dataset.act === "look") { b.disabled = true; await Magnus.updateLook(); await refreshUpdate(); }
      el._html = null; await draw();
    } catch (err) { toast(err.message); b.disabled = false; }
  };
  await draw();
}

// One grant in Settings: what it covers, what still asks, its spend this
// period, the actions taken under it and its policy, with Edit and Revoke.
function grantRow(g, apps, editing) {
  const acts = g.actions || [];
  return `<div class="grant-row" data-grant="${esc(g.id)}" style="margin-top:10px">
    <div class="spread"><strong>${esc(g.title)}</strong>${g.revoked ? `<span class="chip">Revoked ${esc(when(g.revoked))}</span>` : `<span class="row"><button data-act="grant-edit">Edit</button><button data-act="grant-revoke">Revoke</button></span>`}</div>
    <p class="small muted">In ${esc(g.apps.join(", "))} · still asks before ${esc(g.still_asks.join(", "))} · <span class="num">$${esc(Number(g.spent).toFixed(2))} of $${esc(g.max_usd)} this ${esc(g.period)}</span></p>
    ${editing === g.id ? `${grantForm(g.id, g, apps)}<div class="row"><button class="primary" data-act="grant-save">Save</button><button class="link" data-act="grant-cancel">Cancel</button></div>` : ""}
    ${acts.length ? `<details data-key="grant-actions:${esc(g.id)}"><summary class="small">${acts.length} action${acts.length === 1 ? "" : "s"} taken under it</summary><ul class="small">${acts.slice(0, 20).map(a => `<li><span class="faint">${esc(when(a.at))}</span> ${esc(a.tool)} in <a href="#/task/${enc(a.task)}" data-open="task:${esc(a.task)}">${esc(a.task_title || "a task")}</a></li>`).join("")}</ul></details>` : `<p class="small faint">Nothing done under it yet.</p>`}
    <details data-key="grant-policy:${esc(g.id)}"><summary class="small">Its policy</summary><pre class="mono small">${esc(g.cedar)}</pre></details>
  </div>`;
}

// One commitment in Settings (#236): who said they would do what, for
// whom, by when, overdue shown, where it was said; Done, Release, Edit and
// Remove.
function commitmentRow(c, editing) {
  const STATE = { done: ["ok", "Done"], released: ["", "Released"] };
  const who = c.who === "magnus" ? "Magnus" : "You";
  const from = c.source?.kind === "app" ? `read in ${esc(c.source.app)}` : "said in a conversation";
  const status = c.status === "open" ? (c.overdue ? `<span class="chip bad">Overdue</span>` : "") : `<span class="chip ${STATE[c.status]?.[0] || ""}">${esc(STATE[c.status]?.[1] || c.status)}</span>`;
  const acts = c.status === "open" ? `<button data-act="cmt-done">Done</button><button data-act="cmt-release">Release</button>` : `<button data-act="cmt-reopen">Reopen</button>`;
  return `<div class="cmt-row" data-commitment="${esc(c.id)}" style="margin-top:10px">
    <div class="spread"><span><strong>${who}:</strong> ${esc(c.what)} ${status}</span><span class="row">${acts}<button data-act="cmt-edit">Edit</button><button data-act="cmt-remove">Remove</button></span></div>
    <p class="small muted">${c.to ? `For ${esc(c.to)} · ` : ""}${c.by ? `By ${esc(c.by)} · ` : ""}${from}, ${esc(when(c.at))}${c.quote ? ` · <span class="faint">“${esc(c.quote)}”</span>` : ""}</p>
    ${editing === c.id ? `<div class="row" style="margin-top:6px"><input data-f="what" aria-label="What" value="${esc(c.what)}" style="flex:2"><input data-f="to" aria-label="For whom" placeholder="For whom" value="${esc(c.to || "")}" style="flex:1"><input data-f="by" aria-label="By when" placeholder="YYYY-MM-DD" value="${esc(c.by || "")}" style="flex:1"></div>
      <div class="row"><button class="primary" data-act="cmt-save">Save</button><button class="link" data-act="cmt-cancel">Cancel</button></div>` : ""}
  </div>`;
}

function personalSpendCard(sp) {
  if (!sp) return "";
  const spent = Number(sp.spent_usd || 0);
  const limit = Number.isFinite(Number(sp.limit_usd)) && sp.limit_usd !== null ? Number(sp.limit_usd) : null;
  const pct = limit && limit > 0 ? Math.min(100, Math.round((spent / limit) * 100)) : 0;
  const tone = pct >= 90 ? "bad" : pct >= 70 ? "warn" : "ok";
  return `<h3>Monthly usage &amp; spend</h3>
    <div class="card" data-personal-spend>
      <div class="spread"><span><strong>${esc(sp.month || "This month")}</strong> · <span class="num">$${spent.toFixed(2)}</span>${limit !== null ? ` of <span class="num">$${limit.toFixed(2)}</span>` : " spent (no monthly cap)"}</span>
        ${limit !== null ? `<button type="button" data-act="ask-more-spend">Request more</button>` : ""}</div>
      ${limit !== null ? `<div class="stage-bar" style="margin-top:8px"><div class="stage-bar-fill" style="width:${pct}%"></div></div><div class="small muted" style="margin-top:4px"><span class="chip ${tone}">${pct}% used</span>${sp.left_usd !== null && sp.left_usd !== undefined ? ` · $${Math.max(0, Number(sp.left_usd)).toFixed(2)} remaining` : ""}</div>` : ""}
    </div>`;
}

async function memorySection(el) {
  let query = "";
  let editingC = null;
  let notes = [];
  const draw = async () => {
    const [memRes, pbRes, rel, cmtRes] = await Promise.all([
      Server.memory.summary().catch(() => ({ clean_facts: [], untrusted_facts: [], archived_facts: [] })),
      Server.memory.playbook().catch(() => ({ playbook: [] })),
      Server.memory.relationship().catch(() => ({ notes: [] })),
      Server.memory.commitments().catch(() => ({ commitments: [] })),
    ]);
    notes = rel.notes || [];
    const facts = [...(memRes.clean_facts || []), ...(memRes.untrusted_facts || [])];
    const q = query.trim().toLowerCase();
    const shownFacts = q ? facts.filter(f => `${f.fact || ""} ${f.topic || ""}`.toLowerCase().includes(q)) : facts;
    const lessons = pbRes.playbook || [];
    const commitments = cmtRes.commitments || [];
    paint(el, `<h2>Memory, Playbook &amp; Promises</h2>
      <p class="muted">Everything Magnus remembers about you, lessons it learned from your feedback, and promises you or Magnus made. No one else sees these.</p>
      <h3>What Magnus remembers <span class="count">${facts.length}</span></h3>
      <div class="card" data-memory-facts>
        <div class="row" style="margin-bottom:8px;flex-wrap:wrap">
          <input id="mem-search" type="search" value="${esc(query)}" placeholder="Search memories…" aria-label="Search memories" style="flex:1;min-width:160px">
        </div>
        ${shownFacts.length ? `<ul class="tlist">${shownFacts.map(m => `<li class="trow" data-mem-id="${esc(m.id)}"><div class="spread" style="padding:6px 0;gap:8px">
          <div><div>${esc(m.fact || "(stored fact)")}</div><div class="small muted">${m.topic ? `<span class="chip">${esc(m.topic)}</span> ` : ""}${m.by === "person" ? "Added by you" : "Learned by Magnus"} · ${esc(when(m.at))}</div></div>
          <button type="button" class="link small" data-act="mem-forget" data-id="${esc(m.id)}">Forget</button>
        </div></li>`).join("")}</ul>` : `<p class="small faint">${q ? "No matching memories." : "No saved memories yet."}</p>`}
        <form class="row" data-mem-add style="margin-top:10px;flex-wrap:wrap">
          <input name="fact" placeholder="Teach Magnus something to remember…" aria-label="New memory" style="flex:2;min-width:180px">
          <input name="topic" placeholder="Topic (optional)" aria-label="Topic" style="flex:1;min-width:110px">
          <button type="submit" class="primary">Remember</button>
        </form>
      </div>
      <h3>Learned playbook lessons <span class="count">${lessons.length}</span></h3>
      <div class="card" data-memory-playbook>
        <p class="small muted">Standing habits Magnus distilled from your corrections and reviews.</p>
        ${lessons.length ? `<ul class="tlist">${lessons.map(pb => `<li class="trow" data-pb-id="${esc(pb.id)}"><div class="spread" style="padding:6px 0;gap:8px">
          <div><div>${esc(pb.text)}</div>${pb.addresses ? `<div class="small muted">Applies to: ${esc(pb.addresses)}</div>` : ""}</div>
          <button type="button" class="link small" data-act="pb-retire" data-id="${esc(pb.id)}">Retire</button>
        </div></li>`).join("")}</ul>` : `<p class="small faint">No playbook lessons yet.</p>`}
      </div>
      <h3>How Magnus works with you</h3>
      <div class="card" data-relationship><p class="small muted">Standing instructions on tone, delegation, and style carried into every task.</p>
        ${notes.map((n, i) => `<div class="row" data-note="${i}" style="margin-top:6px"><input aria-label="Note" value="${esc(n)}" style="flex:1"><button data-act="note-save">Save</button><button data-act="note-remove">Remove</button></div>`).join("") || `<p class="small faint">Nothing yet.</p>`}
        <div class="row" style="margin-top:8px"><input id="mem-newnote" aria-label="New note" style="flex:1" placeholder="For example: keep updates to 3 bullets"><button data-act="note-add">Add</button></div></div>
      <h3>Promises &amp; commitments <span class="count">${commitments.filter(c => c.status === "open").length}</span></h3>
      <div class="card" data-commitments><p class="small muted">What you or Magnus promised to do, with due dates and overdue alerts.</p>
        ${commitments.map(c => commitmentRow(c, editingC)).join("") || `<p class="small faint">None yet.</p>`}</div>`);
  };
  el.oninput = e => {
    if (e.target.id === "mem-search") { query = e.target.value; draw(); }
  };
  el.onsubmit = async e => {
    const f = e.target.closest("form[data-mem-add]");
    if (!f) return;
    e.preventDefault();
    const fd = new FormData(f);
    const fact = String(fd.get("fact") || "").trim();
    const topic = String(fd.get("topic") || "").trim();
    if (!fact) return toast("Write something for Magnus to remember.");
    try {
      await Server.memory.add(topic ? { fact, topic } : { fact });
      toast("Saved to Magnus's memory.");
      el._html = null; await draw();
    } catch (err) { toast(err.message); }
  };
  el.onclick = async e => {
    const b = e.target.closest("button[data-act]"); if (!b) return;
    try {
      if (b.dataset.act === "mem-forget") { b.disabled = true; await Server.memory.forget(b.dataset.id); toast("Forgotten."); }
      if (b.dataset.act === "pb-retire") { b.disabled = true; await Server.memory.retirePlaybook(b.dataset.id); toast("Lesson retired."); }
      const row = b.closest("[data-note]"), note = row && notes[+row.dataset.note];
      if (b.dataset.act === "note-add") { b.disabled = true; await Server.memory.note({ action: "add", text: $("#mem-newnote", el).value }); toast("Saved."); }
      if (b.dataset.act === "note-save") { b.disabled = true; await Server.memory.note({ action: "change", note, text: $("input", row).value }); toast("Saved."); }
      if (b.dataset.act === "note-remove") { b.disabled = true; await Server.memory.note({ action: "remove", note }); toast("Removed."); }
      const cr = b.closest("[data-commitment]"), c = cr?.dataset.commitment;
      if (b.dataset.act === "cmt-edit") editingC = c;
      if (b.dataset.act === "cmt-cancel") editingC = null;
      const status = { "cmt-done": "done", "cmt-release": "released", "cmt-reopen": "open" }[b.dataset.act];
      if (status) { b.disabled = true; await Server.memory.updateCommitment(c, { status }); toast("Updated."); }
      if (b.dataset.act === "cmt-save") {
        const f = k => $(`[data-f="${k}"]`, cr).value.trim();
        if (!f("what")) return toast("Say what will be done.");
        b.disabled = true; await Server.memory.updateCommitment(c, { what: f("what"), to: f("to"), by: f("by") }); editingC = null; toast("Saved.");
      }
      if (b.dataset.act === "cmt-remove") {
        if (!await ask("Remove this promise?", "Remove")) return;
        b.disabled = true; await Server.memory.removeCommitment(c); toast("Removed.");
      }
      el._html = null; await draw();
    } catch (err) { toast(err.message); b.disabled = false; }
  };
  await draw();
}

async function dataSection(el) {
  // The person's relationship notes as last drawn: each row names its note by them.
  let notes = [];
  // The grant being edited, if any.
  let editing = null;
  let latestSpend = null;
  // The commitment being edited, if any.
  let editingC = null;
  const draw = async () => {
    const [prefs, { requests }, rel, { disagreements }, granted, { commitments }, mySpend] = await Promise.all([
      Server.preferences.get(), Server.memory.deletionRequests(), Server.memory.relationship(),
      Server.memory.disagreements(), Server.memory.grants(), Server.memory.commitments(),
      Server.spend.get().catch(() => null),
    ]);
    App.prefs = prefs;
    notes = rel.notes;
    latestSpend = mySpend;
    const STATE = { done: ["ok", "done"], decline: ["bad", "declined"] };
    paint(el, `<h2>Data and privacy</h2>
      ${personalSpendCard(mySpend)}
      <div class="card"><label class="row toggle"><input type="checkbox" data-pref="feedback_consent" ${prefs.feedback_consent ? "checked" : ""}>
        <span><span>Send problem reports</span><span class="small muted" style="display:block">When you report a problem, what you wrote and the task's record go to the people who build Magnus. Off: nothing leaves the server.</span></span></label></div>
      <h3>How Magnus works with you</h3>
      <div class="card" data-relationship><p class="small muted">What you told Magnus about working with you: tone, what you delegated, standing decisions. Every task and chat of yours carries it; no one else sees it.</p>
        ${notes.map((n, i) => `<div class="row" data-note="${i}" style="margin-top:6px"><input aria-label="Note" value="${esc(n)}" style="flex:1"><button data-act="note-save">Save</button><button data-act="note-remove">Remove</button></div>`).join("") || `<p class="small faint">Nothing yet.</p>`}
        <div class="row" style="margin-top:8px"><input id="newnote" aria-label="New note" style="flex:1" placeholder="For example: fewer jokes"><button data-act="note-add">Add</button></div></div>
      <h3>What Magnus may do on its own</h3>
      <div class="card" data-grants><p class="small muted">Kinds of your work you let Magnus handle without asking, within your organisation's policy, each with what was done under it. They are yours alone; spending money always asks.</p>
        ${granted.grants.map(g => grantRow(g, granted.apps, editing)).join("") || `<p class="small faint">None yet. Choose "Handle things like this" on a task Magnus proposes, or ask Magnus.</p>`}</div>
      <h3>What you and Magnus said you'd do</h3>
      <div class="card" data-commitments><p class="small muted">What you or Magnus said you would do, recognised from your conversations and what Magnus read for you, with for whom and by when. Open ones are in every task and chat of yours; no one else sees them.</p>
        ${commitments.map(c => commitmentRow(c, editingC)).join("") || `<p class="small faint">None yet.</p>`}</div>
      <h3>Where you decided otherwise</h3>
      <div class="card" data-disagreements><p class="small muted">When Magnus advised one thing and you decided another, it notes both, and later how it turned out. Only your own tasks and chats see these.</p>
        ${disagreements.map(d => `<p class="small"><span class="faint">${esc(when(d.at))}</span> Magnus advised: ${esc(d.position)} You decided: ${esc(d.decision)} ${d.outcome ? `Outcome: ${esc(d.outcome)}` : `<span class="faint">Outcome not known yet.</span>`}</p>`).join("") || `<p class="small faint">None.</p>`}</div>
      <h3>Delete my data</h3>
      <div class="card"><p class="small muted">The owner is asked, and has 30 days to act. What is deleted leaves every backup within that time. Recently deleted chats, projects, and files can also be restored from <a href="#/trash" data-open-trash>Trash</a>.</p>
        <label for="delwhat">What to delete</label><input id="delwhat" data-draft="delwhat" style="width:100%" placeholder="all of my data">
        <div class="row" style="margin-top:8px"><button data-act="delete-request">Ask for deletion</button><a class="button" href="#/trash" data-open-trash>Open Trash</a></div>
        ${requests.length ? `<table class="t small" data-deletions style="margin-top:10px"><tbody>${requests.map(q => `<tr><td>${esc(q.what)}${isOwner() && q.person !== App.me.id ? ` <span class="faint">(${esc(q.person)})</span>` : ""}</td>
          <td>${q.decision ? `<span class="chip ${STATE[q.decision]?.[0] || ""}">${esc(STATE[q.decision]?.[1] || q.decision)}</span>${q.note ? ` <span class="muted">${esc(q.note)}</span>` : ""}` : `<span class="chip warn">asked</span> due ${esc(new Date(q.due).toLocaleDateString())}`}</td></tr>`).join("")}</tbody></table>` : ""}</div>
      ${administers() ? `<p class="small muted" data-backups-moved>Backups and restore checks are in <a href="#/admin/operations">Admin › Operations</a>.</p>` : ""}`);
  };
  el.onchange = async e => {
    const c = e.target.closest("input[data-pref]"); if (!c) return;
    try { App.prefs = await Server.preferences.set({ [c.dataset.pref]: c.checked }); toast("Saved."); } catch (err) { toast(err.message); c.checked = !c.checked; }
  };
  el.onclick = async e => {
    const b = e.target.closest("button[data-act]"); if (!b) return;
    try {
      if (b.dataset.act === "ask-more-spend") {
        if (typeof offerMoreSpend === "function") {
          const limit = Number(latestSpend?.limit_usd) || 20;
          const spent = Number(latestSpend?.spent_usd) || 0;
          offerMoreSpend({
            error: `You have spent $${spent.toFixed(2)} of your $${limit.toFixed(2)} monthly limit. Ask an admin for more.`,
            spend_limit: { which: "person", can_ask: true, pending: false, limit_usd: limit },
          });
        } else {
          await Server.spend.ask(10, "Requested from Settings");
          toast("Spend request sent.");
        }
        return;
      }
      if (b.dataset.act === "delete-request") {
        const what = $("#delwhat", el).value.trim();
        if (!await ask(`Ask the owner to delete ${what || "all of your data"}?`, "Ask the owner")) return;
        b.disabled = true; await Server.memory.askToDelete(what); delete drafts.delwhat; toast("Asked. The owner has 30 days to act.");
      }
      const row = b.closest("[data-note]"), note = row && notes[+row.dataset.note];
      if (b.dataset.act === "note-add") { b.disabled = true; await Server.memory.note({ action: "add", text: $("#newnote", el).value }); toast("Saved."); }
      if (b.dataset.act === "note-save") { b.disabled = true; await Server.memory.note({ action: "change", note, text: $("input", row).value }); toast("Saved."); }
      if (b.dataset.act === "note-remove") { b.disabled = true; await Server.memory.note({ action: "remove", note }); toast("Removed."); }
      const g = b.closest("[data-grant]")?.dataset.grant;
      if (b.dataset.act === "grant-edit") editing = g;
      if (b.dataset.act === "grant-cancel") editing = null;
      if (b.dataset.act === "grant-save") {
        const f = readGrantForm(b.closest("[data-grant]"));
        if (!f.apps.length) return toast("Choose at least one app.");
        b.disabled = true; await Server.memory.setGrant(g, f); editing = null; toast("Saved.");
        for (const k of Object.keys(drafts)) if (k.startsWith(`g-${g}-`)) delete drafts[k];
      }
      if (b.dataset.act === "grant-revoke") {
        if (!await ask("Revoke this? From now on, what it covered asks you again.", "Revoke")) return;
        b.disabled = true; await Server.memory.removeGrant(g); toast("Revoked.");
      }
      const cr = b.closest("[data-commitment]"), c = cr?.dataset.commitment;
      if (b.dataset.act === "cmt-edit") editingC = c;
      if (b.dataset.act === "cmt-cancel") editingC = null;
      const status = { "cmt-done": "done", "cmt-release": "released", "cmt-reopen": "open" }[b.dataset.act];
      if (status) { b.disabled = true; await Server.memory.updateCommitment(c, { status }); toast(status === "done" ? "Marked done." : status === "released" ? "Released." : "Reopened."); }
      if (b.dataset.act === "cmt-save") {
        const f = k => $(`[data-f="${k}"]`, cr).value.trim();
        if (!f("what")) return toast("Say what will be done.");
        b.disabled = true; await Server.memory.updateCommitment(c, { what: f("what"), to: f("to"), by: f("by") }); editingC = null; toast("Saved.");
      }
      if (b.dataset.act === "cmt-remove") {
        if (!await ask("Remove this? It leaves your list and Magnus's context, and its words are deleted.", "Remove")) return;
        b.disabled = true; await Server.memory.removeCommitment(c); toast("Removed.");
      }
      el._html = null; await draw();
    } catch (err) { toast(err.message); b.disabled = false; }
  };
  await draw();
}

