// A team channel, or one of its threads, as a conversation of the one chat
// view (chat-view.js) and the one composer (chat-composer.js). It says only
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
 * The composer's side in a channel: files go to the workspace's files and
 * are named in the message; a message goes to the channel, or only to one's
 * own Magnus.
 * @param {any} s @param {?string} thread @returns {ComposerConversation}
 */
function teamComposer(s, thread) {
  const base = () => `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}`;
  return {
    upload: async file => {
      const c = Team.home.channels.find(x => x.id === s.channel);
      await Magnus.putBytes(`/v1/workspaces/${enc(s.ws)}/files/${enc(file.name)}${c?.project ? `?project=${enc(c.project)}` : ""}`, new Uint8Array(await file.arrayBuffer()));
      if (file.type?.startsWith("image/") && file.size <= TEAM_PREVIEW_MAX_BYTES) s.files[file.name] = await teamPreviewUrl(file);
      return { id: file.name, name: file.name, media_type: file.type, size: file.size };
    },
    send: async ({ text, files, form }) => {
      if ($("[data-whisper]", form)?.checked) {
        await api("POST", `${base()}/messages`, { text, thread: thread || undefined, whisper: true });
        return async () => { toast("Your Magnus is on it. Only you will see its answer."); await TeamChat.refresh(); };
      }
      const full = [text, ...files.map(f => `📎 ${f.name}`)].join("\n");
      const r = await api("POST", `${base()}/messages`, { text: full, thread: thread || undefined });
      return async () => {
        if (r.agent) toast(`${Team.home.agent.name} is on it.`);
        if (r.limited) toast(r.limited, 8000);
        await TeamChat.refresh();
      };
    },
    refresh: () => TeamChat.refresh(),
    mentions: partial => mentionChoices(Team.home, partial),
    typed: () => { if (Date.now() - s.typedAt > TEAM_TYPING_PING_MS) { s.typedAt = Date.now(); api("POST", `${base()}/typing`, {}).catch(() => {}); } },
    keep: text => TeamDrafts.set(s.ws, s.channel, thread, text),
  };
}

/**
 * React and Reply in thread: a channel's own controls, first in the row.
 * @param {ChatEntry} e @param {boolean} inThread
 */
function teamLead(e, inThread) {
  const button = (a, label, attrs, cls = "icon") => `<button class="${cls}" ${attrs} aria-label="${label}" data-tooltip="${label}"><svg viewBox="0 0 24 24" aria-hidden="true">${ICONS[a]}</svg></button>`;
  const reactId = `team-react-${enc(e.id)}`;
  const react = `<div class="message-menu-wrap">${button("great", "Add a reaction", `data-menu="${esc(reactId)}" aria-haspopup="menu" aria-expanded="false" aria-controls="${esc(reactId)}"`, "icon menu-trigger")}
    <div id="${esc(reactId)}" class="action-menu reactions" role="menu" aria-label="Reactions" aria-orientation="horizontal" hidden>${TEAM_QUICK_REACTIONS.map(x => `<button role="menuitem" tabindex="-1" data-react="${x}" aria-label="React ${x}">${x}</button>`).join("")}</div></div>`;
  return react + (inThread ? "" : button("reply", "Reply in thread", "data-reply"));
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
  const r = teamReader(s, thread), base = () => `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}`;
  const channel = () => Team.home.channels.find(x => x.id === s.channel);
  const moderates = () => !channel()?.direct && (Team.home.role === "owner" || Team.home.role === "admin");
  const replies = () => s.view.messages.filter(m => m.thread === thread);
  return {
    me: Team.home.me, agentName: Team.home.agent.name, name: r.name,
    names: () => [Team.home.agent.name, ...Team.home.members.map(m => m.name)],
    get hello() { return thread ? "This thread is empty." : s.pinsOnly ? "Nothing pinned yet." : `No messages yet. ${Team.home.agent.name} is here: mention it to ask for anything.`; },
    voice: e => `team:${e.id}`,
    load: async () => {
      const all = s.view.messages;
      const list = thread ? [all.find(m => m.id === thread), ...replies()].filter(Boolean) : all.filter(m => !m.thread && (!s.pinsOnly || m.pinned));
      return list.map(m => teamEntry(m, r));
    },
    prepare: async (entries, view) => {
      const c = channel();
      if (!c) return;
      const project = Team.home.projects.find(p => p.id === c.project);
      paint($(".conv-head", view.section), thread
        ? ChatView.head({ title: "Thread", where: esc(channelTitle(Team.home, c)), tools: `<a class="icon-btn" href="${teamHref(s.ws, { kind: "channel", id: s.channel })}" aria-label="Close thread" title="Close thread">${icon("close")}</a>` })
        : ChatView.head({ title: channelTitle(Team.home, c), where: project ? `<a href="${teamHref(s.ws, { kind: "project", id: project.id })}">${esc(project.name)}</a>` : "", tools: channelTools(Team.home, c, project, s.pinsOnly) }));
    },
    // Retry and Feedback are for who asked the agent; what only they may see
    // is not shared or saved for the team, and the server holds to that too.
    describe: e => {
      if (e.kind === "work" || e.kind === "deleted") return null;
      const agent = e.kind === "answer", mine = e.sender === Team.home.me, asked = agent && e.requester === Team.home.me && !!e.task_id;
      const deletes = (mine && !agent) || moderates() || (agent && e.requester === Team.home.me);
      return { mine, agent, readable: agent, retry: asked, feedback: asked, private: !!e.private, pinned: !!e.pinned, deletes };
    },
    caps: () => ({ guest: Team.home.role === "guest", pins: true }),
    file: f => {
      const img = s.files[f.name];
      return `<div class="fprev"><span class="glimpse" aria-hidden="true">${img ? `<img alt="" src="${esc(img)}">` : icon("file")}</span><span class="fchip"><span>${esc(f.name)}</span> <button class="link small" data-team-download="${esc(f.name)}" aria-label="Download ${esc(f.name)}">Download</button></span></div>`;
    },
    lead: e => e.kind === "deleted" ? "" : teamLead(e, !!thread),
    extras: e => teamExtras(e, r, thread && e.id === thread ? replies().length : undefined),
    note: e => e.private && e.requester === Team.home.me && e.task_id ? `<div class="small faint private-note">Only you can see this: it holds data only you may see. <button class="link small" data-share-answer="${esc(e.task_id)}">Post it to the thread</button></div>` : "",
    before: () => !thread && s.unreadAtOpen >= TEAM_CATCH_UP_AFTER && Team.home.role !== "guest" ? `<button type="button" class="chip run" data-catch-up>Catch me up: ${s.unreadAtOpen} new</button>` : "",
    after: () => thread ? "" : s.whispers.filter(w => !w.shared && !s.hidden.has(w.task)).map(w => `<div class="entry them" data-whisper="${esc(w.task)}"><div class="msg magnus whisper"><div class="small faint byline"><strong>Your Magnus</strong> · only you can see this</div><div class="pre">${esc(w.text || "Working on it…")}</div>${w.done && w.text ? `<div class="row"><button class="primary" data-share="${esc(w.task)}">Share</button><button data-not-now="${esc(w.task)}">Not now</button></div>` : ""}</div></div>`).join(""),
    status: () => {
      const typing = thread ? [] : s.view.typing || [];
      return typing.length ? `${esc(typing.join(", "))} ${typing.length === 1 ? "is" : "are"} ${typing.includes(Team.home.agent.name) && typing.length === 1 ? "working" : "typing"}…` : "";
    },
    actions: (e, view, b, el) => teamActions(s, r, e, view, b, el),
    click: (ev, view) => teamClick(s, ev, view),
  };
}

/**
 * What a message's controls do, in the team's terms (message-controls.js
 * runs them). Share, Save to Files and Retry are the server's: it finds the
 * message as this person sees it and keeps the workspace's rules.
 */
function teamActions(s, r, e, view, b, el) {
  const home = Team.home, ui = view.ui, id = e.id, at = `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}/messages/${enc(id)}`;
  const who = e.sender === "agent" ? home.agent.name : r.name(e.sender), text = String(e.text || "");
  return {
    copy: async () => { await copyText(text); toast("Copied."); },
    // No team route writes a spoken script: the answer is read as written.
    listen: () => readAloud(text, `team:${id}`),
    hush: () => hush(),
    edit: () => { drafts[`edit-${id}`] = text; ui.editing = id; return view.draw(); },
    editCancel: () => { ui.editing = null; return view.draw(); },
    editSend: async () => {
      const typed = $(`#edit-${CSS.escape(id)}`, view.section).value.trim();
      if (!typed) return toast("Write the message first.");
      b.disabled = true;
      await api("PUT", at, { text: [typed, ...(e.files || []).map(f => `📎 ${f.name}`)].join("\n") });
      ui.editing = null; delete drafts[`edit-${id}`]; await TeamChat.refresh();
    },
    retry: async () => {
      b.disabled = true;
      const x = await api("POST", `${at}/retry`, {});
      if (x.agent) toast(`${home.agent.name} is on it.`);
      if (x.limited) toast(x.limited, 8000);
      await TeamChat.refresh();
    },
    branch: () => { App.appSearch = `From the team chat, ${who} wrote:\n${text.split("\n").map(l => `> ${l}`).join("\n")}\n\n`; location.hash = "#/chat/new"; },
    share: () => { ui.people = home.members.filter(x => x.id !== home.me); if (!ui.people.length) return toast("Nobody else is in this workspace."); ui.sharing = id; return view.draw(); },
    shareCancel: () => { ui.sharing = null; return view.draw(); },
    shareSend: async () => {
      const to = $("[data-share-to]", el)?.value;
      if (!to) return;
      b.disabled = true;
      await api("POST", `${at}/share`, { person: to });
      ui.sharing = null; toast(`Shared with ${home.members.find(x => x.id === to)?.name || to}.`); return view.draw();
    },
    export: async () => toast(`Saved ${await Magnus.download(`${at}/export`, `${teamFileStem(who, e.at)}.md`)}`),
    save: async () => { await api("POST", `${at}/save`, {}); toast("Saved to Files."); },
    task: async () => {
      const c = home.channels.find(x => x.id === s.channel);
      await api("POST", `/v1/workspaces/${enc(s.ws)}/tasks`, { title: firstLine(text).slice(0, 200), project: c?.project || undefined, from: { channel: s.channel, message: id } });
      toast("Added to the board."); TeamPanel.refresh();
    },
    pin: async () => { await api("POST", `${at}/pin`, { pinned: !e.pinned }); await TeamChat.refresh(); },
    feedback: () => ({ task: e.task_id, verdict: null, focus: () => $(`[data-entry="${CSS.escape(id)}"] .menu-trigger`, view.section) }),
    delete: async () => {
      if (!await ask("Delete this message for everyone? It cannot be brought back.", "Delete")) return;
      await api("DELETE", at); await TeamChat.refresh();
    },
  };
}

/** A channel's own buttons: reactions, threads, files, private answers, whispers, catch-up. */
async function teamClick(s, ev, view) {
  const t = ev.target, home = Team.home, base = `/v1/workspaces/${enc(s.ws)}`;
  const run = async f => { try { await f(); } catch (err) { toast(err.message); } };
  if (t.closest("[data-catch-up]")) { s.unreadAtOpen = 0; return run(() => TeamChat.send(`@${home.agent.name} catch me up on what I missed here`, null)); }
  if (t.closest("[data-team-download]")) { const n = t.closest("[data-team-download]").dataset.teamDownload; return run(() => Magnus.download(`${base}/files/${enc(n)}`, n)); }
  if (t.closest("[data-share-answer]")) return run(async () => { await api("POST", `${base}/answers/${enc(t.closest("[data-share-answer]").dataset.shareAnswer)}/share`, {}); await TeamChat.refresh(); });
  if (t.closest("[data-share]")) return run(async () => { await api("POST", `${base}/whispers/${enc(t.closest("[data-share]").dataset.share)}/share`, {}); await TeamChat.refresh(); });
  if (t.closest("[data-not-now]")) { s.hidden.add(t.closest("[data-not-now]").dataset.notNow); return view.draw(); }
  if (t.closest("[data-open-thread]")) { ev.preventDefault(); location.hash = teamHref(s.ws, { kind: "channel", id: s.channel, thread: t.closest("[data-open-thread]").dataset.openThread }); return; }
  const msg = t.closest("[data-entry]")?.dataset.entry, e = view.entries.find(x => x.id === msg);
  if (!e) return;
  if (t.closest("[data-reply]")) { location.hash = teamHref(s.ws, { kind: "channel", id: s.channel, thread: msg }); return; }
  const r = t.closest("[data-react]");
  if (r) {
    const emoji = r.dataset.react, on = !(e.reactions?.[emoji] || []).includes(home.me);
    return run(async () => { await api("POST", `${base}/channels/${enc(s.channel)}/messages/${enc(msg)}/react`, { emoji, on }); await TeamChat.refresh(); });
  }
}
