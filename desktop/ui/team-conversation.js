// A shared conversation, or one of its threads, as a source of the one
// conversation (chat-view.js) and the one composer (chat-composer.js). It says only
// where the items come from, what each control does there (the workspace's
// routes, which keep its rules), and what several people add: who sent each
// message, reactions, replies and threads, the requester named, an answer
// only its requester sees, who is typing, a catch-up and one's own Magnus
// whispering.

/** @param {any} s TeamChat's state @param {?string} thread @returns {TeamReader} */
function teamReader(s, thread) {
  return {
    me: Team.home.me, ws: s.ws, channel: s.channel, inThread: !!thread,
    name: id => id === "agent" ? Team.home.agent.name : s.view.names?.[id] || Team.home.members.find(m => m.id === id)?.name || id,
  };
}

/**
 * Where a shared file's preview is kept: the same name in another shared
 * project, or another project's channel, is another file.
 * @param {string} ws @param {?string|undefined} project @param {string} name
 */
function teamPreviewKey(ws, project, name) {
  return JSON.stringify([ws, project || "", name]);
}

/**
 * The work in a channel the viewer may stop: running, and asked by them.
 * Another person's request is theirs to stop.
 * @param {ChatEntry[]} entries @param {string} me
 */
function teamRunningIds(entries, me) {
  return [...new Set(entries.filter(e => e.work?.id && e.work.status === "running" && e.work.requester === me).map(e => e.work.id))];
}

/**
 * The composer's side in a channel. A file is held here until the message
 * goes: a message to the channel puts it in the workspace's files and names
 * it; a whisper takes it to one's own Magnus as a personal chat's file (the
 * person's own library), so it never reaches the shared Files
 * (decisions/2026-11-23-one-request-shape-for-shared-conversations.md).
 * @param {any} s @param {?string} thread @returns {ComposerConversation}
 */
function teamComposer(s, thread) {
  /** The files as the person chose them, or an error to attach one again. @param {Array<{ uid: string, name: string }>} files @returns {File[]} */
  const originals = files => files.map(f => {
    const file = MessageDrafts.originals.get(f.uid);
    if (!file) throw new Error(`Attach ${f.name} again.`);
    return file;
  });
  return {
    upload: async file => ({ id: file.name, name: file.name, media_type: file.type, size: file.size }),
    stopRun: ids => Conversation.stop(ids, () => TeamChat.refresh()),
    send: async ({ text, files, form, record }) => {
      const chosen = originals(files);
      if ($("[data-whisper]", form)?.checked) {
        const mine = await upload(chosen);
        await Server.channels.post(s.ws, s.channel, { text, thread: thread || undefined, whisper: true, files: mine.map(f => f.id), model: chosenModel(form) });
        return async () => { toast("Your Magnus is on it. Only you will see its answer."); await TeamChat.refresh(); };
      }
      const project = Team.home.channels.find(x => x.id === s.channel)?.project;
      for (const file of chosen) {
        await Magnus.putBytes(`/v1/workspaces/${enc(s.ws)}/files/${enc(file.name)}${project ? `?project=${enc(project)}` : ""}`, new Uint8Array(await file.arrayBuffer()));
        if (file.type?.startsWith("image/") && file.size <= TEAM_PREVIEW_MAX_BYTES) s.files[teamPreviewKey(s.ws, project, file.name)] = await teamPreviewUrl(file);
      }
      const full = [text, ...files.map(f => `📎 ${f.name}`)].join("\n");
      const r = await Server.channels.post(s.ws, s.channel, { text: full, thread: thread || undefined, model: Team.home.role === "guest" ? undefined : chosenModel(form) });
      if (!r?.agent && record) record.pendingRun = false;
      return async () => {
        if (r.agent) toast(`${Team.home.agent.name} is on it.`);
        if (r.limited) toast(r.limited, 8000);
        await TeamChat.refresh();
      };
    },
    refresh: () => TeamChat.refresh(),
    mentions: partial => mentionChoices(Team.home, partial),
    typed: () => { if (Date.now() - s.typedAt > TEAM_TYPING_PING_MS) { s.typedAt = Date.now(); Server.channels.typing(s.ws, s.channel).catch(() => {}); } },
  };
}

/**
 * What a channel has beside its items, as the server says (`caps` on its
 * view). An older server's view does not say, and a channel has all of it.
 * @param {any} s TeamChat's state @returns {ConversationCaps}
 */
function teamCaps(s) {
  return s.view?.caps || { several: true, reactions: true, pins: true, threads: true, whisper: Team.home.role !== "guest", typing: true };
}

/**
 * React and Reply in thread: a channel's own controls, first in the row,
 * each where the channel has it.
 * @param {ChatEntry} e @param {boolean} inThread @param {ConversationCaps} [caps]
 */
function teamLead(e, inThread, caps = { reactions: true, threads: true }) {
  const button = (a, label, attrs, cls = "icon") => `<button class="${cls}" ${attrs} aria-label="${label}" data-tooltip="${label}"><svg viewBox="0 0 24 24" aria-hidden="true">${ICONS[a]}</svg></button>`;
  const reactId = `team-react-${enc(e.id)}`;
  const react = `<div class="message-menu-wrap">${button("great", "Add a reaction", `data-menu="${esc(reactId)}" aria-haspopup="menu" aria-expanded="false" aria-controls="${esc(reactId)}"`, "icon menu-trigger")}
    <div id="${esc(reactId)}" class="action-menu reactions" role="menu" aria-label="Reactions" aria-orientation="horizontal" hidden>${TEAM_QUICK_REACTIONS.map(x => `<button role="menuitem" tabindex="-1" data-react="${x}" aria-label="React ${x}">${x}</button>`).join("")}</div></div>`;
  return (caps.reactions ? react : "") + (inThread || !caps.threads ? "" : button("reply", "Reply in thread", "data-reply"));
}

/**
 * Under a message: its reactions, and the replies that hang from it.
 * @param {ChatEntry} e @param {TeamReader} r @param {number} [count] in a thread, how many replies its first message has
 */
function teamExtras(e, r, count) {
  const reactions = Object.entries(e.reactions || {}).map(([x, people]) => `<button class="reaction${people.includes(r.me) ? " on" : ""}" data-react="${esc(x)}" aria-pressed="${people.includes(r.me)}" title="${esc(people.map(r.name).join(", "))}">${esc(x)} ${people.length}</button>`).join("");
  const replies = !r.inThread && e.replies ? `<a class="replies small" href="${teamHref(r.ws, { kind: "channel", id: r.channel, thread: e.id })}" data-open-thread="${esc(e.id)}">${e.replies} ${e.replies === 1 ? "reply" : "replies"}${e.last_reply ? ` · last ${esc(when(e.last_reply))}` : ""}</a>` : "";
  const counted = count === undefined ? "" : `<div class="small faint thread-count">${count} ${count === 1 ? "reply" : "replies"}</div>`;
  return `${reactions ? `<div class="reactions">${reactions}</div>` : ""}${replies}${counted}`;
}

/**
 * A channel (thread null) or one of its threads, as a conversation.
 * @param {any} s TeamChat's state @param {?string} thread @returns {ChatConversation}
 */
function teamConversation(s, thread) {
  const r = teamReader(s, thread), transport = teamTransport(s, r);
  const channel = () => Team.home.channels.find(x => x.id === s.channel);
  const replies = () => s.view.messages.filter(m => m.thread === thread);
  return {
    me: Team.home.me, agentName: Team.home.agent.name, name: r.name,
    names: () => [Team.home.agent.name, ...Team.home.members.map(m => m.name)],
    get hello() { return thread ? "This thread is empty." : `No messages yet. ${Team.home.agent.name} is here: mention @${Team.home.agent.name} to ask for anything.`; },
    voice: e => `team:${e.id}`,
    load: async () => {
      const all = s.view.messages;
      const list = thread ? [all.find(m => m.id === thread), ...replies()].filter(Boolean) : all.filter(m => !m.thread);
      return list.map(m => teamEntry(m, r));
    },
    prepare: async (entries, view) => {
      Conversation.track(view, teamRunningIds(entries, Team.home.me), entries.length > 0, () => TeamChat.refresh());
      const c = channel();
      if (!c) return;
      const project = Team.home.projects.find(p => p.id === c.project);
      paint($(".conv-head", view.section), thread
        ? ChatView.head({ title: "Thread", where: `<a href="${teamHref(s.ws, { kind: "channel", id: s.channel })}">← ${esc(channelTitle(Team.home, c))}</a>`, tools: channelTools(Team.home, c, project) })
        : ChatView.head({ title: channelTitle(Team.home, c), where: c.direct ? "" : `<a href="#/team/${enc(s.ws)}" data-open="shared:files">${esc(project ? project.name : Team.home.name)}</a>`, tools: channelTools(Team.home, c, project) }));
    },
    // Retry and Feedback are for who asked the agent; what only they may see
    // is not shared or saved for the team, and the server holds to that too.
    describe: e => {
      if (e.kind === "work" || e.kind === "deleted") return null;
      const agent = e.kind === "answer";
      return { mine: e.sender === Team.home.me, agent, readable: agent, pinned: !!e.pinned, may: e.may };
    },
    caps: () => teamCaps(s),
    file: f => {
      const img = s.files[teamPreviewKey(s.ws, channel()?.project, f.name)];
      return `<div class="fprev"><span class="glimpse" aria-hidden="true">${img ? `<img alt="" src="${esc(img)}">` : icon("file")}</span><span class="fchip"><span>${esc(f.name)}</span> <button class="link small" data-file-download="${esc(f.name)}" aria-label="Download ${esc(f.name)}">Download</button></span></div>`;
    },
    lead: e => e.kind === "deleted" ? "" : teamLead(e, !!thread, teamCaps(s)),
    extras: e => teamExtras(teamCaps(s).reactions ? e : { ...e, reactions: {} }, r, thread && e.id === thread ? replies().length : undefined),
    note: e => e.private && e.requester === Team.home.me && e.task_id ? `<div class="small faint private-note">Only you can see this: it holds data only you may see. <button class="link small" data-share-answer="${esc(e.task_id)}">Post it to the thread</button></div>` : "",
    before: () => !thread && s.unreadAtOpen >= TEAM_CATCH_UP_AFTER && Team.home.role !== "guest" ? `<button type="button" class="chip run" data-catch-up>Catch me up: ${s.unreadAtOpen} new</button>` : "",
    beforeItem: (_e, i, entries) => !thread && s.unreadAtOpen > 0 && i === Math.max(0, entries.length - s.unreadAtOpen)
      ? `<div class="team-unread-divider" role="separator"><span>New messages</span></div>` : "",
    after: () => thread ? "" : s.whispers.filter(w => !w.shared && !s.hidden.has(w.task)).map(w => `<div class="entry them" data-whisper="${esc(w.task)}"><div class="msg magnus whisper"><div class="small faint byline"><strong>Your Magnus</strong> · only you can see this</div>${w.held_because ? `<div class="small bad">Held for you to check: ${esc(w.held_because)}</div>` : ""}<div class="pre">${esc(w.text || "Working on it…")}</div>${w.done && w.text ? `<div class="row"><button class="primary" data-whisper-share="${esc(w.task)}">Share</button><button data-not-now="${esc(w.task)}">Not now</button></div>` : ""}</div></div>`).join(""),
    status: () => {
      const typing = thread || !teamCaps(s).typing ? [] : s.view.typing || [];
      return typing.length ? `${esc(typing.join(", "))} ${typing.length === 1 ? "is" : "are"} ${typing.includes(Team.home.agent.name) && typing.length === 1 ? "working" : "typing"}…` : "";
    },
    actions: (e, view, b, el) => MessageControls.adapter(e, view, b, el, transport),
    click: (ev, view) => teamClick(s, ev, view),
  };
}

/**
 * Where a channel's message actions go: the workspace's routes, which find
 * the message as this person sees it and keep the workspace's rules.
 * Delete takes a message away for everyone, and it cannot be brought back;
 * Edit changes it in place, marked edited; Share quotes it to another
 * member of the workspace; Branch quotes it into a new private chat, unsent;
 * Save puts it in the workspace's files; reactions are emoji everyone sees.
 * @param {any} s TeamChat's state @param {TeamReader} r @returns {ChatTransport}
 */
function teamTransport(s, r) {
  const home = () => Team.home, at = id => `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}/messages/${enc(id)}`;
  const who = e => e.sender === "agent" ? home().agent.name : r.name(e.sender);
  const asked = x => { if (x.agent) toast(`${home().agent.name} is on it.`); if (x.limited) toast(x.limited, 8000); };
  return {
    text: e => String(e.text || ""),
    resend: (e, text) => Server.messages.edit(s.ws, s.channel, e.id, [text, ...(e.files || []).map(f => `📎 ${f.name}`)].join("\n")),
    retry: async e => asked(await Server.messages.retry(s.ws, s.channel, e.id)),
    branch: e => { App.appSearch = `From the team chat, ${who(e)} wrote:\n${String(e.text || "").split("\n").map(l => `> ${l}`).join("\n")}\n\n`; location.hash = "#/chat/new"; },
    people: async () => home().members.filter(x => x.id !== home().me),
    nobody: "Nobody else is in this workspace.",
    share: (e, to) => Server.messages.share(s.ws, s.channel, e.id, to),
    exportPath: e => `${at(e.id)}/export`,
    exportName: e => `${teamFileStem(who(e), e.at)}.md`,
    save: e => Server.messages.save(s.ws, s.channel, e.id),
    // No team route writes a spoken script: the answer is read as written.
    script: e => String(e.text || ""),
    feedback: e => ({ task: e.task_id, verdict: null }),
    task: async e => {
      const c = home().channels.find(x => x.id === s.channel);
      await Server.workspaces.addTask(s.ws, { title: firstLine(String(e.text || "")).slice(0, 200), project: c?.project || undefined, from: { channel: s.channel, message: e.id } });
      toast("Added to the board."); TeamPanel.refresh();
    },
    pin: e => Server.messages.pin(s.ws, s.channel, e.id, !e.pinned),
    deleting: "Delete this message for everyone? It cannot be brought back.",
    remove: e => Server.messages.remove(s.ws, s.channel, e.id),
    after: () => TeamChat.refresh(),
  };
}

/** A channel's own buttons: reactions, threads, files, private answers, whispers, catch-up. */
async function teamClick(s, ev, view) {
  const t = ev.target, home = Team.home, base = `/v1/workspaces/${enc(s.ws)}`;
  const run = async f => { try { await f(); } catch (err) { toast(err.message); } };
  if (t.closest("[data-catch-up]")) { s.unreadAtOpen = 0; return run(() => TeamChat.send(`@${home.agent.name} catch me up on what I missed here`, null)); }
  if (t.closest("[data-file-download]")) { const n = t.closest("[data-file-download]").dataset.fileDownload; return run(() => Magnus.download(`${base}/files/${enc(n)}`, n)); }
  if (t.closest("[data-share-answer]")) return run(async () => { await Server.workspaces.shareAnswer(s.ws, t.closest("[data-share-answer]").dataset.shareAnswer); await TeamChat.refresh(); });
  if (t.closest("[data-whisper-share]")) return run(async () => { await Server.workspaces.shareWhisper(s.ws, t.closest("[data-whisper-share]").dataset.whisperShare); await TeamChat.refresh(); });
  if (t.closest("[data-not-now]")) { s.hidden.add(t.closest("[data-not-now]").dataset.notNow); return view.draw(); }
  if (t.closest("[data-open-thread]")) { ev.preventDefault(); location.hash = teamHref(s.ws, { kind: "channel", id: s.channel, thread: t.closest("[data-open-thread]").dataset.openThread }); return; }
  const msg = t.closest("[data-entry]")?.dataset.entry, e = view.entries.find(x => x.id === msg);
  if (!e) return;
  if (t.closest("[data-reply]")) { location.hash = teamHref(s.ws, { kind: "channel", id: s.channel, thread: msg }); return; }
  const r = t.closest("[data-react]");
  if (r) {
    const emoji = r.dataset.react, on = !(e.reactions?.[emoji] || []).includes(home.me);
    return run(async () => { await Server.channels.react(s.ws, s.channel, msg, emoji, on); await TeamChat.refresh(); });
  }
}
