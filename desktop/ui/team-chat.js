// A workspace channel: messages with threads, mentions, reactions, edits,
// pins, drafts, typing and presence; the team agent's work as live cards
// with the requester named; and a composer with no button to ask the
// agent: mention it or name it. Changes show at once and sync behind.

/**
 * @typedef {{ task: string, requester: string, status: "working"|"waiting"|"done"|"failed"|"stopped", steps: number, doing: ?string, title: string, long: boolean, thread: ?string, reason: ?string }} TeamWork
 * @typedef {{ id: string, sender: string, requester?: ?string, text: string, thread: ?string, at: string, edited?: boolean, pinned?: boolean, reactions?: Object<string, string[]>, replies?: number, last_reply?: string, work?: TeamWork, state?: "sending"|"failed", files?: string[], task?: string, private?: boolean, deleted?: boolean }} TeamMessage
 * @typedef {{ messages: TeamMessage[], typing: string[], names: Object<string, string> }} TeamView
 */

const TEAM_TYPING_PING_MS = 3000, TEAM_QUICK_REACTIONS = ["👍", "✅", "👀", "🎉"], TEAM_CATCH_UP_AFTER = 8, TEAM_PREVIEW_MAX_BYTES = 4 * 1024 * 1024;

/** A file name for a message: who wrote it and when, with nothing a name may not hold. */
function teamFileStem(who, at) {
  return `${who} ${String(at || "").slice(0, 16).replace("T", " ").replace(":", "-")}`.replace(/[\/\0]/g, "-").trim().slice(0, 150);
}

/** A dropped image as a data URL, for its preview. */
function dataUrl(file) {
  return new Promise(ok => { const r = new FileReader(); r.onload = () => ok(String(r.result)); r.onerror = () => ok(null); r.readAsDataURL(file); });
}

/** What a tool call means to a reader of the channel. */
function doingWords(tool) {
  if (!tool) return "Starting";
  if (tool === "task_complete") return "Finishing up";
  if (tool.startsWith("web_")) return "Searching the web";
  if (tool.startsWith("computer_")) return "Using the team computer";
  if (tool.startsWith("browser_")) return "Browsing";
  if (tool === "team_read_file") return "Reading a team file";
  if (tool === "team_write_file") return "Writing a file for the team";
  if (tool === "team_post") return "Posting an update";
  if (tool === "team_note") return "Taking notes";
  if (tool === "team_add_fact" || tool === "team_fix_entry") return "Updating the knowledge base";
  if (tool === "team_read_knowledge") return "Reading the knowledge base";
  if (tool.includes("__")) return `Using ${tool.split("__")[0]}`;
  return "Working";
}

/** A task's title as a card shows it: the ask, without the mention that made it. */
function workTitle(text) {
  const t = firstLine(text).replace(/^\s*@[\w.-]+[\s,:]*/, "").trim();
  return t ? t[0].toUpperCase() + t.slice(1) : firstLine(text);
}

/**
 * The agent's live card: what it is doing, for whom, and where the work is.
 * @param {TeamWork} w @param {(id: string) => string} name @param {string} ws @param {string} channel @param {boolean} [inThread] shown in its own thread already
 */
function workCard(w, name, ws, channel, inThread = false) {
  const who = name(w.requester), title = workTitle(w.title);
  const line = w.status === "working" ? esc(w.doing ? doingWords(w.doing) : "Thinking it through")
    : w.status === "waiting" ? `Waiting for ${esc(who)} to answer an ask`
    : w.status === "done" ? "Done" : w.status === "failed" ? `Could not finish${w.reason ? `: ${esc(w.reason)}` : ""}` : "Stopped";
  const thread = w.thread && !inThread ? `<a class="small" href="${teamHref(ws, { kind: "channel", id: channel, thread: w.thread })}" data-open-thread="${esc(w.thread)}">Open its thread</a>` : "";
  return `<div class="tm-work" data-status="${esc(w.status)}" data-task="${esc(w.task)}"><div class="tm-work-title">${w.status === "working" ? `<span class="team-spinner" aria-hidden="true"></span>` : ""}<strong>${esc(title)}</strong> <span class="small faint">for ${esc(who)}</span></div><div class="small">${line}</div>${thread}</div>`;
}

/** Text with mentions marked and files shown. People's words stay plain. */
function messageText(m, agent, files) {
  const lines = String(m.text || "").split("\n");
  const attached = lines.filter(l => l.startsWith("📎 ")).map(l => l.slice(2).trim());
  const text = lines.filter(l => !l.startsWith("📎 ")).join("\n");
  const body = agent ? Compose.markdown(text) : `<div class="pre">${Compose.maskedText(text)}</div>`;
  const marked = body.replace(/(^|[\s(>])@([A-Za-z][\w.-]*(?: [A-Z][\w-]*)?)/g, (all, pre, who) => `${pre}<span class="mention">@${who}</span>`);
  const chips = attached.map(f => `<div class="tm-file">${files?.[f] ? `<img src="${esc(files[f])}" alt="${esc(f)}">` : icon("file")}<span>${esc(f)}</span><button class="link small" data-team-download="${esc(f)}">Download</button></div>`).join("");
  return marked + chips;
}

/**
 * One message, with its actions. A pending one shows as sending.
 * @param {TeamMessage} m @param {{ me: string, ws: string, channel: string, name: (id: string) => string, guest: boolean, moderates?: boolean, files?: Object<string, string>, inThread?: boolean, editing?: string }} c
 */
function messageHtml(m, c) {
  if (m.deleted) return deletedHtml(m, c);
  const agent = m.sender === "agent", mine = m.sender === c.me;
  const who = agent ? "Magnus" : c.name(m.sender);
  const reactions = Object.entries(m.reactions || {}).map(([e, people]) => `<button class="tm-reaction${people.includes(c.me) ? " on" : ""}" data-react="${esc(e)}" aria-pressed="${people.includes(c.me)}" title="${esc(people.map(c.name).join(", "))}">${esc(e)} ${people.length}</button>`).join("");
  const replies = !c.inThread && m.replies ? `<a class="tm-replies small" href="${teamHref(c.ws, { kind: "channel", id: c.channel, thread: m.id })}" data-open-thread="${esc(m.id)}">${m.replies} ${m.replies === 1 ? "reply" : "replies"}${m.last_reply ? ` · last ${esc(when(m.last_reply))}` : ""}</a>` : "";
  const state = m.state === "sending" ? `<span class="small faint">Sending…</span>` : m.state === "failed" ? `<span class="small bad">Not sent</span> <button class="link small" data-retry="${esc(m.id)}">Retry</button>` : "";
  const editing = c.editing === m.id;
  const text = editing ? `<form class="tm-edit" data-edit-form><textarea aria-label="Edit message" rows="2">${esc(m.text)}</textarea><div class="row"><button class="primary">Save</button><button type="button" data-edit-cancel>Cancel</button></div></form>` : messageText(m, agent, c.files);
  const actions = m.state ? "" : teamMsgActions(m, c, agent, mine);
  const sharing = c.sharing === m.id ? MessageControls.shareRow(`tshare-${m.id}`, c.people) : "";
  return `<article class="tm${agent ? " agent" : ""}${mine ? " mine" : ""}${m.state ? ` ${m.state}` : ""}" data-msg="${esc(m.id)}">${avatar(who, agent)}<div class="tm-body">
    <div class="tm-head"><strong>${esc(who)}</strong>${agent ? ` <span class="chip run">team agent</span>${m.requester ? ` <span class="small faint">for ${esc(c.name(m.requester))}</span>` : ""}` : ""} <time class="small faint" datetime="${esc(m.at)}">${esc(when(m.at))}</time>${m.edited ? ` <span class="small faint">(edited)</span>` : ""}${m.pinned ? ` <span class="small faint">· pinned</span>` : ""} ${state}</div>
    ${text}${agent && m.private && m.requester === c.me && m.task ? `<div class="small faint">Only you can see this: it holds data only you may see. <button class="link small" data-share-answer="${esc(m.task)}">Post it to the thread</button></div>` : ""}${m.work ? workCard(m.work, c.name, c.ws, c.channel, c.inThread) : ""}${reactions ? `<div class="tm-reactions">${reactions}</div>` : ""}${replies}${actions}${sharing}</div></article>`;
}

/**
 * The personal chat's message controls, on a team message (message-controls.js),
 * after the team's own React and Reply in thread (owner, 2026-11-13: "the
 * same message controls as the other chat window"). Retry and Feedback are
 * for who asked the agent; what only they may see is not shared or saved
 * for the team, and the server holds to that too.
 * @param {TeamMessage} m @param {{ me: string, guest: boolean, moderates?: boolean, inThread?: boolean }} c @param {boolean} agent @param {boolean} mine
 */
function teamMsgActions(m, c, agent, mine) {
  const entry = `team:${m.id}`, asked = agent && m.requester === c.me && !!m.task;
  /** The team's own icon buttons: React's menu and Reply in thread. */
  const button = (a, label, attrs, cls = "icon") => `<button class="${cls}" ${attrs} aria-label="${label}" data-tooltip="${label}"><svg viewBox="0 0 24 24" aria-hidden="true">${ICONS[a]}</svg></button>`;
  const reactId = `team-react-${enc(m.id)}`;
  const react = `<div class="message-menu-wrap">${button("great", "Add a reaction", `data-menu="${esc(reactId)}" aria-haspopup="menu" aria-expanded="false" aria-controls="${esc(reactId)}"`, "icon menu-trigger")}
    <div id="${esc(reactId)}" class="action-menu reactions" role="menu" aria-label="Reactions" aria-orientation="horizontal" hidden>${TEAM_QUICK_REACTIONS.map(e => `<button role="menuitem" tabindex="-1" data-react="${e}" aria-label="React ${e}">${e}</button>`).join("")}</div></div>`;
  const reply = c.inThread ? "" : button("reply", "Reply in thread", "data-reply");
  // A post with no words (a work card) can still be reacted to and replied to.
  if (!m.text) return `<div class="msg-actions small" role="group" aria-label="Message actions">${react}${reply}</div>`;
  const deletes = (mine && !agent) || !!c.moderates || (agent && m.requester === c.me);
  const list = MessageControls.controls({ mine, agent, readable: agent, retry: asked, feedback: asked, private: !!m.private, pinned: !!m.pinned, deletes }, { guest: c.guest, pins: true, canReadAloud: App.canReadAloud, reading: voice.entry === entry });
  return MessageControls.html(teamMenuId(m.id), list, { voiceEntry: entry, lead: react + reply });
}

/** The id of a team message's "More message actions" menu. @param {string} id */
function teamMenuId(id) {
  return `team-menu-${enc(id)}`;
}

/** Where a deleted message stood: its replies still hang from it. @param {TeamMessage} m */
function deletedHtml(m, c) {
  const replies = !c.inThread && m.replies ? `<a class="tm-replies small" href="${teamHref(c.ws, { kind: "channel", id: c.channel, thread: m.id })}" data-open-thread="${esc(m.id)}">${m.replies} ${m.replies === 1 ? "reply" : "replies"}</a>` : "";
  return `<article class="tm deleted" data-msg="${esc(m.id)}"><div class="tm-body"><div class="small">This message was deleted.</div>${replies}</div></article>`;
}

/**
 * The server's messages with what is still on its way. A pending message
 * leaves once its real one arrives.
 * @param {TeamMessage[]} messages @param {Array<TeamMessage & { real?: string }>} pending
 */
function mergePending(messages, pending) {
  const have = new Set(messages.map(m => m.id));
  return [...messages, ...pending.filter(p => !(p.real && have.has(p.real)))];
}

/**
 * A change shown before the server confirms it.
 * @param {TeamView} view @param {{ op: string, id?: string, text?: string, emoji?: string, me?: string, on?: boolean, pinned?: boolean }} o
 */
function applyLocal(view, o) {
  const m = view.messages.find(x => x.id === o.id);
  if (!m) return view;
  if (o.op === "edit") { m.text = o.text; m.edited = true; }
  if (o.op === "pin") m.pinned = o.pinned;
  if (o.op === "delete") { if (m.replies) Object.assign(m, { deleted: true, text: "", reactions: {} }); else view.messages = view.messages.filter(x => x !== m); }
  if (o.op === "react") {
    const r = m.reactions = { ...(m.reactions || {}) };
    const people = new Set(r[o.emoji] || []);
    if (o.on) people.add(o.me); else people.delete(o.me);
    if (people.size) r[o.emoji] = [...people]; else delete r[o.emoji];
  }
  return view;
}

/** The @word being typed at the caret, if any. */
function mentionAt(text, caret) {
  const m = text.slice(0, caret).match(/(?:^|\s)@([\w.-]*)$/);
  return m ? m[1] : null;
}

/** Who a partial @mention could be: the team agent first, then people. */
function mentionChoices(home, partial) {
  const q = (partial || "").toLowerCase();
  const all = [{ id: home.agent.id, name: home.agent.name, agent: true }, ...home.members.filter(m => m.id !== home.me)];
  return all.filter(p => p.name.toLowerCase().startsWith(q) || p.id.toLowerCase().startsWith(q)).slice(0, 6);
}

/** How many people's faces a channel's header shows before "+N". */
const TEAM_HEAD_FACES = 4;

/**
 * Who is in a channel, at a glance: its people with who is here now, and
 * the team agent, which is in every channel.
 * @param {TeamHome} home @param {TeamChannel} c @param {?TeamProject} project
 */
function channelFaces(home, c, project) {
  const ids = c.direct ? c.direct.people : project ? [...new Set([project.owner, ...project.members])] : home.members.map(m => m.id);
  const people = ids.map(id => home.members.find(m => m.id === id)).filter(Boolean);
  const shown = people.slice(0, TEAM_HEAD_FACES), more = people.length - shown.length;
  const names = [...people.map(m => m.name), home.agent.name].join(", ");
  return `<span class="team-head-faces" title="${esc(names)}" aria-label="In this channel: ${esc(names)}">${shown.map(m => `<span class="team-face${m.online ? " on" : ""}">${avatar(m.name)}</span>`).join("")}${more > 0 ? `<span class="small faint">+${more}</span>` : ""}${avatar(home.agent.name, true)}</span>`;
}

/** Drafts survive a switch of channel and a restart, where storage allows. */
const TeamDrafts = {
  key: (ws, ch, th) => `team-draft:${ws}:${ch}:${th || ""}`,
  get(ws, ch, th) { try { return localStorage.getItem(this.key(ws, ch, th)) || ""; } catch { return ""; } },
  set(ws, ch, th, v) { try { if (v) localStorage.setItem(this.key(ws, ch, th), v); else localStorage.removeItem(this.key(ws, ch, th)); } catch { /* storage blocked: the draft lives in the box */ } },
};

const TeamChat = {
  s: null,

  async open(center, side, ws, place) {
    const s = this.s = { ws, channel: place.id, thread: place.thread, center, side, view: { messages: [], typing: [], names: {} }, pending: [], files: TeamChat.s?.files || {}, editing: null, pinsOnly: false, whispers: [], hidden: new Set(), tray: [], lastRead: "", typedAt: 0 };
    const c = Team.home.channels.find(x => x.id === s.channel);
    if (!c) { paint(center, `<p class="empty">This channel is gone, or not yours to see.</p>`); paint(side, ""); return; }
    s.unreadAtOpen = c.unread;
    center.closest(".team-shell")?.classList.toggle("threaded", !!s.thread);
    center.innerHTML = `<header class="team-head" id="tc-head"></header><div class="team-scroll" id="tc-list" aria-live="polite"></div><div class="team-typing small faint" id="tc-typing" aria-live="polite"></div><div id="tc-whispers"></div>${this.composer("tc", ws, s.channel, null)}`;
    this.bind(center, false);
    await this.refresh(true);
    $("#tc-text", center)?.focus({ preventScroll: true });
  },

  composer(prefix, ws, channel, thread) {
    const guest = Team.home.role === "guest";
    const c = Team.home.channels.find(x => x.id === channel);
    const where = thread ? "Reply in thread" : c ? `Message ${channelTitle(Team.home, c)}` : "Message";
    return `<form class="team-composer" id="${prefix}-form" data-thread="${esc(thread || "")}"><div class="team-tray" id="${prefix}-tray"></div>
      <div class="team-mentions" id="${prefix}-mentions" role="listbox" hidden></div>
      <textarea id="${prefix}-text" rows="2" placeholder="${esc(where)}. Mention @Magnus or name it to ask the team agent." aria-label="${esc(where)}">${esc(TeamDrafts.get(ws, channel, thread))}</textarea>
      <div class="row"><span class="attachment-picker"><button type="button" class="icon-btn" data-team-attach aria-label="Add files">${icon("attach")}</button><input type="file" multiple hidden data-team-files></span>
      ${guest ? "" : `<label class="small"><input type="checkbox" id="${prefix}-whisper"> Only my Magnus sees this</label>`}<button class="primary" aria-label="Send">${icon("send")}</button></div></form>`;
  },

  ctx(inThread) {
    const s = this.s, home = Team.home;
    const name = id => id === "agent" ? home.agent.name : s.view.names?.[id] || home.members.find(m => m.id === id)?.name || id;
    const direct = !!home.channels.find(x => x.id === s.channel)?.direct;
    return { me: home.me, ws: s.ws, channel: s.channel, name, guest: home.role === "guest", moderates: !direct && (home.role === "owner" || home.role === "admin"), files: s.files, inThread, editing: s.editing, sharing: s.sharing, people: home.members.filter(m => m.id !== home.me) };
  },

  async refresh(first = false) {
    const s = this.s;
    if (!s || !s.center.isConnected) return;
    const base = `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}`;
    const [view, whispers] = await Promise.all([api("GET", `${base}/view`), Team.home.role === "guest" ? [] : api("GET", `${base}/whispers`).catch(() => [])]);
    if (s !== this.s) return;
    s.view = view; s.whispers = whispers;
    this.draw(first);
    const newest = view.messages.at(-1)?.at || "";
    if (newest && newest !== s.lastRead) { s.lastRead = newest; api("POST", `${base}/read`, {}).catch(() => {}); }
  },

  draw(scroll = false) {
    const s = this.s, home = Team.home, c = home.channels.find(x => x.id === s.channel);
    if (!c) return;
    const project = home.projects.find(p => p.id === c.project);
    const helpful = c.mode === "when_helpful";
    paint($("#tc-head", s.center), `<div><h1>${esc(channelTitle(home, c))}</h1>${project ? `<a class="small faint" href="${teamHref(s.ws, { kind: "project", id: project.id })}">${esc(project.name)}</a>` : ""}</div>
      <div class="team-head-tools">${channelFaces(home, c, project)}${c.direct ? "" : `<label class="small" title="How the team agent takes part here"><span class="sr">Magnus here</span><select data-agent-mode aria-label="How Magnus takes part here" ${home.role === "guest" ? "disabled" : ""}><option value="mentions"${helpful ? "" : " selected"}>Magnus replies to mentions</option><option value="when_helpful"${helpful ? " selected" : ""}>Magnus joins when it can help</option></select></label>`}
      <select data-notify aria-label="Notifications for this channel">${[["all", "All messages"], ["mentions", "Mentions only"], ["off", "Off"]].map(([v, l]) => `<option value="${v}"${c.notify === v ? " selected" : ""}>${l}</option>`).join("")}</select>
      <button type="button" data-pins aria-pressed="${s.pinsOnly}">${s.pinsOnly ? "All messages" : "Pinned"}</button>
      <button type="button" data-archive="${c.archived ? "off" : "on"}" title="${c.archived ? "Back to your list" : "Off your list until someone writes here"}">${c.archived ? "Unarchive" : "Archive"}</button>${c.direct ? `<button type="button" class="danger" data-clear-conversation>Delete conversation</button>` : ""}</div>`);
    const all = mergePending(s.view.messages, s.pending);
    const roots = all.filter(m => !m.thread && (!s.pinsOnly || m.pinned));
    const catchUp = s.unreadAtOpen >= TEAM_CATCH_UP_AFTER && home.role !== "guest" ? `<button type="button" class="chip run" data-catch-up>Catch me up: ${s.unreadAtOpen} new</button>` : "";
    const list = $("#tc-list", s.center), atBottom = list.scrollHeight - list.scrollTop - list.clientHeight < 40;
    const changed = paint(list, `${catchUp}${roots.map(m => messageHtml(m, this.ctx(false))).join("") || `<p class="empty">${s.pinsOnly ? "Nothing pinned yet." : `No messages yet. ${esc(home.agent.name)} is here: mention it to ask for anything.`}</p>`}`);
    if (changed && (scroll || atBottom)) list.scrollTop = list.scrollHeight;
    const typing = s.view.typing || [];
    paint($("#tc-typing", s.center), typing.length ? `${esc(typing.join(", "))} ${typing.length === 1 ? "is" : "are"} ${typing.includes(home.agent.name) && typing.length === 1 ? "working" : "typing"}…` : "");
    const ws = s.whispers.filter(w => !w.shared && !s.hidden.has(w.task));
    paint($("#tc-whispers", s.center), ws.map(w => `<div class="tm-whisper" data-whisper="${esc(w.task)}"><div class="small"><strong>Your Magnus</strong> · only you can see this</div><div class="pre">${esc(w.text || "Working on it…")}</div>${w.done && w.text ? `<div class="row"><button class="primary" data-share="${esc(w.task)}">Share</button><button data-not-now="${esc(w.task)}">Not now</button></div>` : ""}</div>`).join(""));
    if (s.thread) this.drawThread(); else TeamPanel.side(s.side, s.ws, c.project, s.channel);
  },

  drawThread() {
    const s = this.s, root = mergePending(s.view.messages, s.pending).find(m => m.id === s.thread);
    let box = $("#tc-thread", s.side);
    if (!box) {
      s.side.innerHTML = `<header class="team-side-head"><h2>Thread</h2><a href="${teamHref(s.ws, { kind: "channel", id: s.channel })}" aria-label="Close thread">${icon("close")}</a></header><div class="team-scroll" id="tc-thread"></div>${this.composer("tt", s.ws, s.channel, s.thread)}`;
      this.bind(s.side, true);
      box = $("#tc-thread", s.side);
    }
    const replies = mergePending(s.view.messages, s.pending).filter(m => m.thread === s.thread);
    const changed = paint(box, root || replies.length ? `${root ? messageHtml(root, this.ctx(true)) : ""}<div class="tm-thread-count small faint">${replies.length} ${replies.length === 1 ? "reply" : "replies"}</div>${replies.map(m => messageHtml(m, this.ctx(true))).join("")}` : `<p class="empty">This thread is empty.</p>`);
    if (changed) box.scrollTop = box.scrollHeight;
  },

  /** Sends at once into the list, and syncs behind. */
  async send(text, thread, whisper, files) {
    const s = this.s, base = `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}`;
    if (whisper) {
      try { await api("POST", `${base}/messages`, { text, thread, whisper: true }); toast("Your Magnus is on it. Only you will see its answer."); await this.refresh(); } catch (err) { toast(err.message); }
      return;
    }
    const names = files.map(f => f.name);
    const full = [text, ...names.map(n => `📎 ${n}`)].filter(Boolean).join("\n");
    const p = { id: `tmp-${Date.now()}-${Math.random().toString(36).slice(2)}`, sender: Team.home.me, text: full, thread: thread || null, at: new Date().toISOString(), state: "sending" };
    s.pending.push(p);
    this.draw(true);
    try {
      const c = Team.home.channels.find(x => x.id === s.channel);
      for (const f of files) {
        s.files[f.name] = f.preview || s.files[f.name];
        await Magnus.putBytes(`/v1/workspaces/${enc(s.ws)}/files/${enc(f.name)}${c?.project ? `?project=${enc(c.project)}` : ""}`, new Uint8Array(await f.file.arrayBuffer()));
      }
      const r = await api("POST", `${base}/messages`, { text: full, thread: thread || undefined });
      p.real = r.id; delete p.state;
      if (r.agent) toast(`${Team.home.agent.name} is on it.`);
      if (r.limited) toast(r.limited, 8000);
      await this.refresh();
    } catch (err) { p.state = "failed"; p.error = err.message; this.draw(); toast(err.message); }
  },

  /** A change shown now; on failure the server's truth comes back. */
  async change(o, call) {
    applyLocal(this.s.view, o);
    this.draw();
    try { await call(); } catch (err) { toast(err.message); await this.refresh(); }
  },

  bind(root, inThread) {
    const prefix = inThread ? "tt" : "tc", form = $(`#${prefix}-form`, root), box = $(`#${prefix}-text`, root), s = this.s;
    const thread = () => form.dataset.thread || null;
    const tray = () => paint($(`#${prefix}-tray`, root), s.tray.filter(f => f.prefix === prefix).map((f, i) => `<span class="tm-file">${f.preview ? `<img src="${esc(f.preview)}" alt="">` : icon("file")}${esc(f.name)}<button type="button" class="link small" data-untray="${i}" aria-label="Remove ${esc(f.name)}">Remove</button></span>`).join(""));
    const addFiles = async list => {
      for (const file of list) s.tray.push({ prefix, file, name: file.name, preview: file.type?.startsWith("image/") && file.size <= TEAM_PREVIEW_MAX_BYTES ? await dataUrl(file) : null });
      tray();
    };
    const submit = () => {
      const text = box.value.trim(), files = s.tray.filter(f => f.prefix === prefix);
      if (!text && !files.length) return;
      const whisper = $(`#${prefix}-whisper`, root)?.checked;
      s.tray = s.tray.filter(f => f.prefix !== prefix); tray();
      box.value = ""; TeamDrafts.set(s.ws, s.channel, thread(), "");
      this.send(text, thread(), whisper, files);
    };
    form.onsubmit = e => { e.preventDefault(); submit(); };
    const menu = $(`#${prefix}-mentions`, root);
    const choose = name => { const at = box.selectionStart, before = box.value.slice(0, at).replace(/@[\w.-]*$/, `@${name} `); box.value = before + box.value.slice(at); box.setSelectionRange(before.length, before.length); menu.hidden = true; };
    box.onkeydown = e => {
      if (!menu.hidden && (e.key === "Enter" || e.key === "Tab")) { const first = $("[data-mention]", menu); if (first) { e.preventDefault(); choose(first.dataset.mention); return; } }
      if (e.key === "Escape") menu.hidden = true;
      if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); submit(); }
    };
    box.oninput = () => {
      TeamDrafts.set(s.ws, s.channel, thread(), box.value);
      const partial = mentionAt(box.value, box.selectionStart);
      const choices = partial === null ? [] : mentionChoices(Team.home, partial);
      menu.hidden = !choices.length;
      menu.innerHTML = choices.map(p => `<button type="button" role="option" data-mention="${esc(p.name)}">${avatar(p.name, p.agent)}${esc(p.name)}${p.agent ? ` <span class="small faint">team agent</span>` : ""}</button>`).join("");
      if (Date.now() - s.typedAt > TEAM_TYPING_PING_MS) { s.typedAt = Date.now(); api("POST", `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}/typing`, {}).catch(() => {}); }
    };
    menu.onclick = e => { const b = e.target.closest("[data-mention]"); if (b) { choose(b.dataset.mention); box.focus(); } };
    root.addEventListener("dragover", e => { if (e.dataTransfer?.types?.includes("Files")) { e.preventDefault(); root.classList.add("dropping"); } });
    root.addEventListener("dragleave", () => root.classList.remove("dropping"));
    root.addEventListener("drop", e => { if (!e.dataTransfer?.files?.length) return; e.preventDefault(); root.classList.remove("dropping"); addFiles(e.dataTransfer.files); });
    root.addEventListener("change", e => { if (e.target.matches("[data-team-files]")) { addFiles(e.target.files); e.target.value = ""; } });
    root.addEventListener("submit", e => {
      const f = e.target.closest("[data-edit-form]");
      if (!f) return;
      e.preventDefault();
      const id = f.closest("[data-msg]").dataset.msg, text = $("textarea", f).value.trim();
      s.editing = null;
      if (text) this.change({ op: "edit", id, text }, () => api("PUT", `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}/messages/${enc(id)}`, { text }));
    });
    root.addEventListener("change", e => {
      if (e.target.matches("[data-agent-mode]")) this.setting("agent", { mode: e.target.value }, c => { c.mode = e.target.value; });
      if (e.target.matches("[data-notify]")) this.setting("notify", { level: e.target.value }, c => { c.notify = e.target.value; });
    });
    root.addEventListener("click", e => this.click(e, root, tray));
  },

  /** Put this conversation away, or bring it back: the caller's list only. */
  async archive(on) {
    const s = this.s;
    try {
      await api("POST", `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}/archive`, { on });
      await loadHome(s.ws);
      if (on) { toast("Archived. It comes back when someone writes in it."); this.away(); } else { toast("Back on your list."); this.draw(); }
    } catch (err) { toast(err.message); }
  },

  /** Off this conversation, now it is off the list: to the workspace's
   * first channel still on it, else its activity. */
  away() {
    const s = this.s, next = firstChannel(Team.home);
    location.hash = next && next !== s.channel ? teamHref(s.ws, { kind: "channel", id: next }) : teamHref(s.ws, { kind: "activity" });
  },

  /** Delete a direct conversation from the caller's own list and view. */
  async clear() {
    const s = this.s;
    if (!(await ask("Delete this conversation from your list? What it holds now is gone for you. The others in it keep theirs.", "Delete"))) return;
    try {
      await api("POST", `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}/clear`, {});
      await loadHome(s.ws);
      toast("Deleted from your list.");
      this.away();
    } catch (err) { toast(err.message); }
  },

  async setting(what, body, local) {
    const s = this.s, c = Team.home.channels.find(x => x.id === s.channel);
    if (c) local(c);
    try { await api("POST", `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}/${what}`, body); } catch (err) { toast(err.message); await loadHome(s.ws); this.draw(); }
  },

  async click(e, root, tray) {
    const s = this.s, base = `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}`, t = e.target;
    const msg = t.closest("[data-msg]")?.dataset.msg, m = s.view.messages.find(x => x.id === msg);
    if (t.closest("[data-team-attach]")) { $("[data-team-files]", root).click(); return; }
    if (t.closest("[data-untray]")) { const i = Number(t.closest("[data-untray]").dataset.untray), mine = s.tray.filter(f => f.prefix === (root === s.side ? "tt" : "tc")); s.tray = s.tray.filter(f => f !== mine[i]); tray(); return; }
    if (t.closest("[data-pins]")) { s.pinsOnly = !s.pinsOnly; this.draw(); return; }
    if (t.closest("[data-archive]")) { await this.archive(t.closest("[data-archive]").dataset.archive === "on"); return; }
    if (t.closest("[data-clear-conversation]")) { await this.clear(); return; }
    if (t.closest("[data-catch-up]")) { s.unreadAtOpen = 0; this.send(`@${Team.home.agent.name} catch me up on what I missed here`, null, false, []); return; }
    if (t.closest("[data-team-download]")) { const n = t.closest("[data-team-download]").dataset.teamDownload; try { await Magnus.download(`/v1/workspaces/${enc(s.ws)}/files/${enc(n)}`, n); } catch (err) { toast(err.message); } return; }
    if (t.closest("[data-retry]")) { const p = s.pending.find(x => x.id === t.closest("[data-retry]").dataset.retry); if (p) { s.pending = s.pending.filter(x => x !== p); this.send(p.text, p.thread, false, []); } return; }
    if (t.closest("[data-share-answer]")) { try { await api("POST", `/v1/workspaces/${enc(s.ws)}/answers/${enc(t.closest("[data-share-answer]").dataset.shareAnswer)}/share`, {}); await this.refresh(); } catch (err) { toast(err.message); } return; }
    if (t.closest("[data-share]")) { try { await api("POST", `/v1/workspaces/${enc(s.ws)}/whispers/${enc(t.closest("[data-share]").dataset.share)}/share`, {}); await this.refresh(); } catch (err) { toast(err.message); } return; }
    if (t.closest("[data-not-now]")) { s.hidden.add(t.closest("[data-not-now]").dataset.notNow); this.draw(); return; }
    if (t.closest("[data-open-thread]")) { e.preventDefault(); location.hash = teamHref(s.ws, { kind: "channel", id: s.channel, thread: t.closest("[data-open-thread]").dataset.openThread }); return; }
    if (!m) return;
    // A choice from a menu closes it first, focus back on its button, so the
    // redraw does not reopen a menu that the document then shuts.
    if (t.closest('[role="menuitem"]')) Menus.close(true);
    if (t.closest("[data-react]")) {
      const emoji = t.closest("[data-react]").dataset.react, on = !(m.reactions?.[emoji] || []).includes(Team.home.me);
      this.change({ op: "react", id: msg, emoji, me: Team.home.me, on }, () => api("POST", `${base}/messages/${enc(msg)}/react`, { emoji, on }));
    }
    if (t.closest("[data-reply]")) location.hash = teamHref(s.ws, { kind: "channel", id: s.channel, thread: msg });
    if (t.closest("[data-edit-cancel]")) { s.editing = null; this.draw(); }
    const act = t.closest("[data-msg-act]");
    if (act) { try { await this.msgAct(act.dataset.msgAct, m, act); } catch (err) { toast(err.message); act.disabled = false; } }
  },

  /**
   * What a message's controls do, in the team's terms (message-controls.js
   * runs them). Share, Save to Files and Retry are the server's: it finds
   * the message as this person sees it and keeps the workspace's rules.
   */
  async msgAct(act, m, b) {
    const s = this.s, home = Team.home, base = `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}`, msg = m.id, at = `${base}/messages/${enc(msg)}`;
    const who = m.sender === "agent" ? home.agent.name : s.view.names?.[m.sender] || home.members.find(x => x.id === m.sender)?.name || m.sender;
    const text = String(m.text || "");
    return MessageControls.run(act, {
      copy: async () => { await copyText(text); toast("Copied."); },
      // No team route writes a spoken script: the answer is read as written.
      listen: () => readAloud(text, `team:${msg}`),
      hush: () => hush(),
      edit: () => { s.editing = msg; this.draw(); $(`[data-msg="${CSS.escape(msg)}"] [data-edit-form] textarea`)?.focus(); },
      retry: async () => {
        b.disabled = true;
        const r = await api("POST", `${at}/retry`, {});
        if (r.agent) toast(`${home.agent.name} is on it.`);
        if (r.limited) toast(r.limited, 8000);
        await this.refresh();
      },
      branch: () => { App.appSearch = `From the team chat, ${who} wrote:\n${text.split("\n").map(l => `> ${l}`).join("\n")}\n\n`; location.hash = "#/chat/new"; },
      share: () => { if (!home.members.some(x => x.id !== home.me)) return toast("Nobody else is in this workspace."); s.sharing = msg; this.draw(); },
      shareCancel: () => { s.sharing = null; this.draw(); },
      shareSend: async () => {
        const to = $(`[data-msg="${CSS.escape(msg)}"] [data-share-to]`)?.value;
        if (!to) return;
        b.disabled = true;
        await api("POST", `${at}/share`, { person: to });
        s.sharing = null; toast(`Shared with ${home.members.find(x => x.id === to)?.name || to}.`); this.draw();
      },
      export: async () => toast(`Saved ${await Magnus.download(`${at}/export`, `${teamFileStem(who, m.at)}.md`)}`),
      save: async () => { await api("POST", `${at}/save`, {}); toast("Saved to Files."); },
      task: async () => {
        const c = home.channels.find(x => x.id === s.channel);
        await api("POST", `/v1/workspaces/${enc(s.ws)}/tasks`, { title: firstLine(text).slice(0, 200), project: c?.project || undefined, from: { channel: s.channel, message: msg } });
        toast("Added to the board."); TeamPanel.refresh();
      },
      pin: () => this.change({ op: "pin", id: msg, pinned: !m.pinned }, () => api("POST", `${at}/pin`, { pinned: !m.pinned })),
      feedback: () => ({ task: m.task, verdict: null, focus: () => $(`[data-menu="${CSS.escape(teamMenuId(msg))}"]`) }),
      delete: async () => {
        if (!await ask("Delete this message for everyone? It cannot be brought back.", "Delete")) return;
        // What was sent from here goes too, or it would show again as on its way.
        s.pending = s.pending.filter(p => p.real !== msg);
        return this.change({ op: "delete", id: msg }, () => api("DELETE", at));
      },
    });
  },
};
