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
    verify: f => api("GET", `/v1/files/${enc(f.id)}/info`),
    refresh: () => refresh(),
    send: async ({ text, files, form, record }) => {
      const body = { text, model: chosenModel(form), files: files.map(f => f.id) };
      if (record.thread) body.thread = record.thread; else if (record.project) body.project = record.project;
      const response = await api("POST", "/v1/chat", body);
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
  const path = (id, act) => `/v1/chat/${enc(thread)}/entries/${enc(id)}/${act}`;
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
  return {
    hello: "",
    stop: () => listen([]),
    load: async () => {
      const entries = await Resources.read("Conversation", `/v1/chat/${enc(thread)}`, "entries");
      return entries && App.thread === thread ? entries : null;
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
    },
    drawn: (el, entries, view) => {
      bindConnectorActs(el);
      keepHeights(el);
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
      const mine = e.kind === "message", magnus = e.kind === "answer" || e.kind === "turn";
      const said = ["answer", "turn", "result", "tasks", "grant_card", "suggestion_card", "schedule_card", "connector_card", "opener"].includes(e.kind);
      return { mine, agent: !mine, readable: e.kind === "answer", retry: magnus, feedback: said, deletes: true };
    },
    caps: () => ({}),
    actions: (e, view, b, el) => {
      const id = e.id, ui = view.ui, draw = view.draw, main = view.section;
      return {
        copy: async () => { await copyText(entryText(e)); toast("Copied."); },
        edit: () => { drafts[`edit-${id}`] = e.text; ui.editing = id; return draw(); },
        editCancel: () => { ui.editing = null; return draw(); },
        editSend: async () => {
          const text = $(`#edit-${CSS.escape(id)}`, main).value.trim();
          if (!text) return toast("Write the message first.");
          b.disabled = true; await api("POST", path(id, "resend"), { text }); ui.editing = null; delete drafts[`edit-${id}`]; return draw();
        },
        retry: async () => { b.disabled = true; await api("POST", path(id, "resend"), {}); return draw(); },
        branch: async () => { const r = await api("POST", path(id, "branch"), {}); toast("Branched into a new chat."); location.hash = `#/chat/${enc(r.thread)}`; },
        share: async () => { ui.people = (await api("GET", "/v1/directory")).people; if (!ui.people.length) return toast("Nobody else is on this server."); ui.sharing = id; return draw(); },
        shareCancel: () => { ui.sharing = null; return draw(); },
        shareSend: async () => {
          const to = $("[data-share-to]", el).value;
          b.disabled = true; await api("POST", path(id, "share"), { person: to }); ui.sharing = null;
          toast(`Shared with ${ui.people.find(p => p.id === to)?.name || to}.`); return draw();
        },
        export: async () => toast(`Saved ${await Magnus.download(path(id, "export"), safeName(e))}`),
        save: async () => { await api("POST", path(id, "save"), {}); toast("Saved to Files."); },
        listen: () => readAloud(api("POST", path(id, "spoken"), {}).then(r => r.script), id),
        hush: () => hush(),
        feedback: () => ({
          task: e.task_id, verdict: e.verdict, reaction: e.reaction,
          react: async a => { await api("POST", path(id, "reaction"), { reaction: e.reaction === a ? null : a }); await draw(); },
          focus: () => $(`[data-entry="${CSS.escape(id)}"] .menu-trigger`, main),
        }),
        task: async () => { const r = await api("POST", path(id, "task"), {}); toast("Started a task."); await refresh(); return r; },
        delete: async () => {
          if (!await ask("Delete this message? It leaves this chat and what Magnus reads next, and waits in the trash until it is gone for good.", "Delete")) return;
          await api("POST", path(id, "delete"), {}); await draw();
          const heading = $("#conv-head h1"); if (heading) { heading.tabIndex = -1; heading.focus({ preventScroll: true }); }
        },
      };
    },
    click: (e, view) => personalCardClick(e, view),
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
        await api("POST", `/v1/tasks/${enc(ta.dataset.task)}/cancel`, {}); toast("Stopped.");
      } else {
        const id = enc(sa.dataset.scheduleId), k = sa.dataset.schedAct;
        if (k === "cancel") {
          const ended = App.schedules.find(x => x.id === sa.dataset.scheduleId)?.ended;
          if (!await ask(ended ? "Delete this schedule? It has ended, so nothing stops; its runs stay in Tasks." : "Cancel this schedule? No more of its runs start, and a run still going stops.", ended ? "Delete schedule" : "Cancel schedule")) { el.disabled = false; return; }
          await api("DELETE", `/v1/schedules/${id}`); toast(ended ? "The schedule is deleted." : "The schedule is cancelled.");
        } else { await api("POST", `/v1/schedules/${id}/${k}`, {}); toast(k === "pause" ? "Paused." : "Resumed."); }
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
    const r = await api("POST", `/v1/chat/cards/${enc(card)}`, body);
    toast(no ? "Declined." : decision === "later" ? "Not now." : r.grant ? "Granted." : r.task ? "Started." : "Scheduled.");
    ui.declining = null; delete drafts[`why-${card}`];
    for (const k of Object.keys(drafts)) if (k.startsWith(`g-${card}-`)) delete drafts[k];
    await refresh();
  } catch (err) { toast(err.message); b.disabled = false; }
}
