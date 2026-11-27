// The personal chat's source for the one conversation (chat-view.js):
// one person and Magnus. A message is answered, or becomes a task or a
// schedule to confirm: the model decides. What it starts shows in the thread
// as it goes: tasks with their status, the decisions they wait on (the same
// cards as under Needs you), schedules, results and their files.

/**
 * The personal chat's side of the composer: files go to the person's Files,
 * a message to /v1/chat.
 * @returns {ComposerConversation}
 */
function personalComposer() {
  return {
    upload: (file, record) => upload([file], record.thread ? { thread: record.thread } : record.project ? { project: record.project } : {}).then(r => r[0]),
    verify: f => Server.files.info(f.id),
    refresh: () => refresh(),
    stopRun: ids => Conversation.stop(ids, refresh),
    steerRun: async (ids, text) => {
      await Server.tasks.steer(ids[0], text);
      toast("Steered Magnus.");
      await refresh();
    },
    send: async ({ text, files, form, record }) => {
      const autonomy = typeof chosenAutonomy === "function" ? chosenAutonomy(form) : "standard";
      let finalText = text;
      /** @type {?string[]} */
      let approvedSteps = null;
      if (autonomy === "plan_first" && text && typeof PlanSurface !== "undefined" && typeof PlanSurface.open === "function") {
        const approved = await PlanSurface.open(text);
        if (!approved) return;
        finalText = approved;
        const planPart = approved.split("Follow this approved plan in order:\n")[1] || "";
        approvedSteps = planPart ? PlanSurface.parseSteps(planPart) : null;
      } else if (autonomy === "cautious" && text) {
        finalText = `[Session leash: Ask for my explicit approval before any write or modification.]\n\n${text}`;
      } else if (autonomy === "draft_only" && text) {
        finalText = `[Session leash: Draft and read only. Do not send, publish, or modify external state—stage drafts or present results for my review only.]\n\n${text}`;
      }
      const body = { text: finalText, model: chosenModel(form), files: files.map(f => f.id) };
      if (autonomy !== "standard") body.autonomy_mode = autonomy;
      if (record.thread) body.thread = record.thread; else if (record.project) body.project = record.project;
      const response = await Server.chat.send(body);
      const startedId = response?.task_id || response?.task || null;
      if (startedId) {
        if (approvedSteps && typeof PlanSurface !== "undefined" && typeof PlanSurface.trackTask === "function") {
          PlanSurface.trackTask(startedId, approvedSteps);
        }
        record.runningIds = [...new Set([...(record.runningIds || []), startedId])];
        if (record.cancelRequested) {
          record.cancelRequested = false;
          record.pendingRun = false;
          Server.tasks.cancel(startedId).then(() => { toast("Stopped."); refresh(); }).catch(() => {});
        }
      }
      return async () => {
        record.anchor = { follow: true };
        const threadEl = $("#thread");
        if (threadEl && typeof ReadingAnchors !== "undefined") ReadingAnchors.remember(threadEl, { follow: true });
        // A secret typed into chat was hidden by the server: say so, and where a sign-in belongs.
        if (response?.hidden?.length) toast(`Magnus hid the ${[...new Set(response.hidden)].join(" and ")} in your message: it is not kept, and Magnus never sees it. When Magnus needs a sign-in, it asks for it through a secure form.`, 8000);
        if (!record.thread && response?.thread) {
          const carry = { pendingRun: !!record.pendingRun && !record.cancelRequested, runningIds: [...(record.runningIds || [])], cancelRequested: !!record.cancelRequested };
          MessageDrafts.discard(record.id);
          const next = MessageDrafts.forThread(response.thread);
          next.text = record.text; next.files = record.files; next.model = record.model; next.modelUnder = record.modelUnder;
          next.pendingRun = carry.pendingRun; next.runningIds = carry.runningIds; next.cancelRequested = carry.cancelRequested;
          next.anchor = { follow: true };
          if (App.workspace && App.draftRecord === record) { history.replaceState(history.state, "", `#/chat/${enc(response.thread)}`); record.sending = false; await go(); return; }
        }
        await refresh();
      };
    },
  };
}

/**
 * The person's own conversation with Magnus.
 * @param {?string} thread
 * @returns {ChatConversation & { stop: () => void }}
 */
function personalChat(thread) {
  // An answer being written shows as it streams; `ui.live` holds its text so far.
  const listening = {};
  const isActive = (/** @type {?string} */ status) => !!status && !ENDED.includes(status);
  /** @param {ChatEntry[]} entries @param {ChatViewState} [view] */
  const listen = (entries, view) => {
    const running = new Set(entries.filter(e => e.kind === "turn" && isActive(e.status)).map(e => e.task_id));
    for (const id of running) if (!listening[id]) listening[id] = Magnus.live(id, t => {
      view.ui.live[id] = (view.ui.live[id] || "") + t;
      const el = $(`[data-live="${CSS.escape(id)}"]`, view.section);
      if (el) {
        const root = view.list, followed = ReadingAnchors.capture(root)?.follow;
        ReadingAnchors.hold(root, () => { patchLive(el, view.ui.live[id]); loadMarkdownImages(el); });
        if (!followed) $("[data-jump-latest]", view.section).hidden = false;
      }
    });
    for (const id of Object.keys(listening)) if (!running.has(id)) { listening[id](); delete listening[id]; if (view) delete view.ui.live[id]; }
  };
  // What the thread has beside its entries, as the server says: one person
  // and Magnus, so nothing several people add.
  /** @type {ConversationCaps} */
  let caps = {};
  const transport = personalTransport(thread);
  return {
    hello: "",
    hero: {
      title: () => { const name = (App.me?.name || "").trim().split(/\s+/)[0]; return name ? `Hello, ${name}` : "Hello"; },
      subtitle: "What should we work on today?",
      starters: [
        "Chief of Staff: Check my unread emails and resolve any schedule conflicts",
        "Procurement: Research a product, compare supplier prices, and prepare an order for approval",
        "Operations: Track open threads and follow up on anything stalled for 48 hours",
        "Engineering: Triage pull requests waiting for my review and summarize CI blockers",
      ],
      render: () => (typeof Discover !== "undefined" ? Discover.heroHtml() : null),
    },
    stop: () => listen([]),
    load: async () => {
      const r = await Resources.read("Conversation", `/v1/chat/${enc(thread)}`);
      if (!r || App.thread !== thread) return null;
      caps = r.caps || {};
      return r.entries;
    },
    prepare: async (entries, view) => {
      // A decision the thread now waits on, not read yet: read the inbox first.
      const waits = entries.flatMap(e => e.kind === "tasks" ? e.tasks.map(t => t.waiting_item) : e.kind === "sub_agents" ? e.agents.map(a => a.waiting_item) : [e.waiting_item]).filter(Boolean);
      if (waits.some(id => !App.inbox.some(i => i.id === id))) await pollInbox();
      convHead(thread);
      const ui = view.ui;
      ui.composed = [];
      // A result that comes after the person has moved on (a message of theirs
      // since the task started) quotes what they asked.
      ui.movedOn = new Set();
      const startedAt = {};
      let lastMessage = -1;
      entries.forEach((e, i) => {
        if (e.kind === "message") lastMessage = i;
        if (e.kind === "tasks") for (const t of e.tasks) startedAt[t.task_id] = i;
        if (e.kind === "result" && startedAt[e.task_id] !== undefined && lastMessage > startedAt[e.task_id]) ui.movedOn.add(e.task_id);
      });
      listen(entries, view);
      const runningIds = [...new Set([
        ...entries.filter(e => e.kind === "turn" && isActive(e.status)).map(e => e.task_id),
        ...entries.flatMap(e => e.kind === "tasks" ? e.tasks : []).filter(t => isActive(t.status)).map(t => t.task_id),
        ...entries.flatMap(e => e.kind === "sub_agents" ? e.agents : []).filter(a => isActive(a.status)).map(a => a.task_id),
      ].filter(Boolean))];
      Conversation.track(view, runningIds, entries.some((e, i) => i > lastMessage && (e.kind === "answer" || (e.kind === "turn" && !isActive(e.status)))), refresh);
    },
    drawn: (el, entries, view) => {
      bindConnectorActs(el);
      keepHeights(el);
      for (const [id, text] of Object.entries(view.ui.live)) {
        const liveEl = $(`[data-live="${CSS.escape(id)}"]`, el);
        if (liveEl && text) { patchLive(liveEl, text); loadMarkdownImages(liveEl); }
      }
      for (const e of entries.filter(e => e.view)) fill($(`iframe[data-shown="${CSS.escape(e.id)}"]`, view.section), e.view, e.task_id);
      for (const [id, v, , task] of view.ui.composed) fill($(`iframe[data-view="${CSS.escape(id)}"]`, el), v, task);
      listen(entries, view);
      loadGlimpses(el);
    },
    // Edit is for the person's own messages, Retry for Magnus's answers, Read
    // aloud for an answer, from a script written for listening; Feedback for
    // what Magnus said; the rest for every entry.
    describe: e => {
      if (!e.id || (e.kind !== "message" && e.kind !== "answer")) return null;
      const mine = e.kind === "message";
      return { mine, agent: !mine, readable: e.kind === "answer", may: e.may };
    },
    caps: () => caps,
    lead: e => (typeof App !== "undefined" && App.workspaces?.length && (e.kind === "answer" || e.kind === "message") && e.text)
      ? `<button type="button" class="share-team-act small" data-share-team="${esc(e.id)}" title="Share to team channel">Share to team</button>` : "",
    actions: (e, view, b, el) => MessageControls.adapter(e, view, b, el, transport),
    click: (e, view) => { if (typeof Discover !== "undefined" && Discover.handleHeroClick(e, view)) return; return personalCardClick(e, view); },
  };
}

/**
 * Where the personal chat's message actions go: its own routes. Delete puts
 * a message in the person's trash, where it can be restored; Edit resends,
 * and what came after leaves the thread; Share reaches someone on this
 * server through their inbox; Branch makes a new thread on the server; Save
 * puts it in the person's Files; a reaction is their private verdict.
 * @param {?string} thread @returns {ChatTransport}
 */
function personalTransport(thread) {
  const path = (id, act) => `/v1/chat/${enc(thread)}/entries/${enc(id)}/${act}`;
  const entry = (id, act, body) => Server.chat.entry(thread, id, act, body);
  return {
    text: e => entryText(e),
    resend: async (e, text) => {
      if (typeof ask === "function" && !(await ask("Edit and resend this message? Replies after it will be replaced.", "Edit & resend"))) return false;
      return entry(e.id, "resend", { text });
    },
    retry: e => entry(e.id, "resend"),
    branch: async e => { const r = await entry(e.id, "branch"); toast("Branched into a new chat."); location.hash = `#/chat/${enc(r.thread)}`; },
    people: async () => (await Server.directory()).people,
    nobody: "Nobody else is on this server.",
    share: (e, to) => entry(e.id, "share", { person: to }),
    exportPath: e => path(e.id, "export"),
    exportName: e => safeName(e),
    save: e => entry(e.id, "save"),
    script: e => entry(e.id, "spoken").then(r => r.script),
    feedback: (e, view) => ({
      task: e.task_id, verdict: e.verdict, reaction: e.reaction,
      react: async a => { await entry(e.id, "reaction", { reaction: e.reaction === a ? null : a }); await view.draw(); },
    }),
    task: async e => { const r = await entry(e.id, "task"); toast("Started a task."); await refresh(); return r; },
    pin: async () => { throw new Error("Nothing is pinned in your own chat."); },
    deleting: "Delete this message? It leaves this chat and what Magnus reads next, and waits in the trash until it is gone for good.",
    remove: e => entry(e.id, "delete"),
    after: async (view, act) => {
      await view.draw();
      if (act !== "delete") return;
      const heading = $("#conv-head h1"); if (heading) { heading.tabIndex = -1; heading.focus({ preventScroll: true }); }
    },
  };
}

async function shareToTeamModal(entry) {
  const teams = App.workspaces || [];
  if (!teams.length) return toast("You are not in any team yet.");
  const d = document.createElement("dialog");
  d.className = "ask";
  d.innerHTML = `<form method="dialog" class="col" style="gap:10px;">
    <h3>Share to team channel</h3>
    <label class="small">Team <select data-st-ws>${teams.map(w => `<option value="${esc(w.id)}">${esc(w.name)}</option>`).join("")}</select></label>
    <label class="small">Channel <select data-st-ch><option value="">Loading…</option></select></label>
    <label class="small">Message <textarea data-st-text rows="5" style="width:100%;">${esc(entry.text || "")}</textarea></label>
    <div class="row"><button type="button" data-st-cancel>Cancel</button><button type="submit" class="primary" data-st-send>Post to channel</button></div>
  </form>`;
  const wsSel = $("[data-st-ws]", d), chSel = $("[data-st-ch]", d), sendBtn = $("[data-st-send]", d);
  const loadCh = async () => {
    chSel.innerHTML = `<option value="">Loading…</option>`;
    sendBtn.disabled = true;
    try {
      const home = await Server.workspaces.home(wsSel.value);
      const chs = (home?.channels || []).filter(c => !c.archived && !c.deleted && !c.direct);
      chSel.innerHTML = chs.length ? chs.map(c => `<option value="${esc(c.id)}">#${esc(c.name)}</option>`).join("") : `<option value="">No open channels</option>`;
      sendBtn.disabled = !chs.length;
    } catch (err) { chSel.innerHTML = `<option value="">Could not load channels</option>`; toast(err.message); }
  };
  wsSel.onchange = loadCh;
  $("[data-st-cancel]", d).onclick = () => d.close();
  $("form", d).onsubmit = async ev => {
    ev.preventDefault();
    const text = $("[data-st-text]", d).value.trim(), ws = wsSel.value, ch = chSel.value;
    if (!ws || !ch || !text) return;
    sendBtn.disabled = true;
    try {
      await Server.channels.post(ws, ch, { text });
      d.close();
      toast("Shared to team channel.");
    } catch (err) { toast(err.message); sendBtn.disabled = false; }
  };
  d.addEventListener("close", () => d.remove());
  document.body.append(d);
  d.showModal();
  loadCh();
}

/** The live cards' buttons act directly, not through Magnus. @param {MouseEvent} e @param {ChatViewState} view */
async function personalCardClick(e, view) {
  const st = e.target.closest("[data-share-team]");
  if (st) {
    const item = view.entries.find(x => x.id === st.dataset.shareTeam);
    if (item) await shareToTeamModal(item);
    return;
  }
  const ui = view.ui, draw = view.draw, main = view.section;
  const ta = e.target.closest("[data-task-act]"), sa = e.target.closest("[data-sched-act]");
  if (ta || sa) {
    e.preventDefault();
    const el = ta || sa;
    el.disabled = true;
    try {
      if (ta) {
        if (!await ask("Stop this task? What it already did stays done.", "Stop")) { el.disabled = false; return; }
        await Server.tasks.cancel(ta.dataset.task); toast("Stopped.");
      } else {
        const id = sa.dataset.scheduleId, k = sa.dataset.schedAct;
        if (k === "cancel") {
          const ended = App.schedules.find(x => x.id === sa.dataset.scheduleId)?.ended;
          if (!await ask(ended ? "Delete this schedule? It has ended, so nothing stops; its runs stay in Tasks." : "Delete this schedule? No more of its runs start, and a run still going stops. Its runs stay in Tasks.", "Delete schedule")) { el.disabled = false; return; }
          await Server.schedules.remove(id); toast("The schedule is deleted.");
        } else if (k === "stop") {
          if (!await ask("Stop this schedule? No more of its runs start, and a run still going stops. The schedule stays, ended, until you delete it.", "Stop schedule")) { el.disabled = false; return; }
          await Server.schedules.stop(id); toast("The schedule is stopped.");
        } else { await (k === "pause" ? Server.schedules.pause(id) : Server.schedules.resume(id)); toast(k === "pause" ? "Paused." : "Resumed."); }
      }
      await refresh();
    } catch (err) { toast(err.message); el.disabled = false; }
    return;
  }
  const b = e.target.closest("[data-decide]");
  if (!b) return;
  const card = b.closest("[data-card]").dataset.card, decision = b.dataset.decide;
  // Decline asks for the reason first; Magnus reads it before suggesting again.
  if (decision === "decline-open" || decision === "decline-cancel") { ui.declining = decision === "decline-open" ? card : null; return draw(); }
  const body = { decision };
  // A grant card accepted: the grant as the person set it in the form.
  const box = b.closest("[data-card]");
  if (box.matches(".grant") && decision === "confirm") {
    Object.assign(body, readGrantForm(box));
    if (!body.apps.length) return toast("Choose at least one app.");
  }
  const no = decision === "decline";
  if (no) {
    body.reason = ($(`#why-${CSS.escape(card)}`, main)?.value || "").trim();
    if (!body.reason) return toast("Say why, so Magnus knows.");
  }
  b.disabled = true;
  try {
    const r = await Server.chat.answerCard(card, body);
    const putOff = box.matches("[data-later]");
    toast(no ? (putOff ? "Deleted." : "Declined.") : decision === "later" ? "Not now. It stays here for later." : r.grant ? "Granted." : r.task ? "Started." : "Scheduled.");
    ui.declining = null; delete drafts[`why-${card}`];
    for (const k of Object.keys(drafts)) if (k.startsWith(`g-${card}-`)) delete drafts[k];
    await refresh();
  } catch (err) { toast(err.message); b.disabled = false; }
}
