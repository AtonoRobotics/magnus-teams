// The personal chat, as a conversation of the one chat view (chat-view.js):
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
    send: async ({ text, files, form, record }) => {
      const body = { text, model: chosenModel(form), files: files.map(f => f.id) };
      if (record.thread) body.thread = record.thread; else if (record.project) body.project = record.project;
      const response = await Server.chat.send(body);
      return async () => {
        // A secret typed into chat was hidden by the server: say so, and where a sign-in belongs.
        if (response?.hidden?.length) toast(`Magnus hid the ${[...new Set(response.hidden)].join(" and ")} in your message: it is not kept, and Magnus never sees it. When Magnus needs a sign-in, it asks for it through a secure form.`, 8000);
        if (!record.thread && response?.thread) {
          MessageDrafts.discard(record.id);
          const next = MessageDrafts.forThread(response.thread); next.text = record.text; next.files = record.files; next.model = record.model; next.modelUnder = record.modelUnder;
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
  /** @param {ChatEntry[]} entries @param {ChatViewState} [view] */
  const listen = (entries, view) => {
    const running = new Set(entries.filter(e => e.kind === "turn" && e.status === "running").map(e => e.task_id));
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
      if (!e.id || e.kind === "sub_agents") return null;
      const mine = e.kind === "message";
      return { mine, agent: !mine, readable: e.kind === "answer", may: e.may };
    },
    caps: () => caps,
    actions: (e, view, b, el) => MessageControls.adapter(e, view, b, el, transport),
    click: (e, view) => personalCardClick(e, view),
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
    resend: (e, text) => entry(e.id, "resend", { text }),
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

/** The live cards' buttons act directly, not through Magnus. @param {MouseEvent} e @param {ChatViewState} view */
async function personalCardClick(e, view) {
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
          if (!await ask(ended ? "Delete this schedule? It has ended, so nothing stops; its runs stay in Tasks." : "Cancel this schedule? No more of its runs start, and a run still going stops.", ended ? "Delete schedule" : "Cancel schedule")) { el.disabled = false; return; }
          await Server.schedules.remove(id); toast(ended ? "The schedule is deleted." : "The schedule is cancelled.");
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
    toast(no ? "Declined." : decision === "later" ? "Not now." : r.grant ? "Granted." : r.task ? "Started." : "Scheduled.");
    ui.declining = null; delete drafts[`why-${card}`];
    for (const k of Object.keys(drafts)) if (k.startsWith(`g-${card}-`)) delete drafts[k];
    await refresh();
  } catch (err) { toast(err.message); b.disabled = false; }
}
