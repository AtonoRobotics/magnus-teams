// A workspace channel is the one chat view (chat-view.js) with a
// conversation of several people: this file is only where a channel's items
// come from, what each control does there, and what only several people
// have (who sent each message, mentions, reactions, threads, the requester
// named on the agent's work, an answer only its requester sees, who is
// typing, a catch-up, and one's own Magnus whispering). A thread opens in the
// same view beside its channel. The composer is the personal chat's
// (chat-composer.js); there is no button to ask the agent: mention it or
// name it.

/**
 * A channel's items as the server sends them (crates/magnus/src/team/items.rs).
 * @typedef {{ task: string, requester: string, status: "running"|"suspended"|"completed"|"failed"|"cancelled", steps: number, doing: ?string, title: string, long: boolean, thread: ?string, reason: ?string }} TeamWork
 * @typedef {{ id: string, kind: "message"|"answer"|"work"|"deleted", files?: string[], sender: string, requester?: ?string, text: string, thread: ?string, at: string, edited?: boolean, pinned?: boolean, reactions?: Object<string, string[]>, replies?: number, last_reply?: string, work?: TeamWork, task?: string, private?: boolean, deleted?: boolean }} TeamMessage
 * @typedef {{ messages: TeamMessage[], typing: string[], names: Object<string, string> }} TeamView
 * Who is reading, to read a channel's items into the chat view's kinds.
 * @typedef {{ me: string, ws: string, channel: string, name: (id: string) => string, inThread?: boolean }} TeamReader
 */

const TEAM_TYPING_PING_MS = 3000, TEAM_QUICK_REACTIONS = ["👍", "✅", "👀", "🎉"], TEAM_CATCH_UP_AFTER = 8, TEAM_PREVIEW_MAX_BYTES = 4 * 1024 * 1024;

/** A file name for a message: who wrote it and when, with nothing a name may not hold. */
function teamFileStem(who, at) {
  return `${who} ${String(at || "").slice(0, 16).replace("T", " ").replace(":", "-")}`.replace(/[\/\0]/g, "-").trim().slice(0, 150);
}

/** An image as a data URL, for its preview. @param {File} file */
function teamPreviewUrl(file) {
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
 * The agent's work on a request as the chat view's work row: the personal
 * chat's task row, with the requester named and its thread to open. Its
 * status is the task's, as the personal chat's tasks have.
 * @param {TeamWork} w @param {TeamReader} r @returns {ChatWork}
 */
function teamWork(w, r) {
  const who = r.name(w.requester), status = w.status;
  const line = status === "running" ? (w.doing ? doingWords(w.doing) : "Thinking it through")
    : status === "suspended" ? `Waiting for ${who} to answer an ask`
    : status === "completed" ? "Done" : status === "failed" ? `Could not finish${w.reason ? `: ${w.reason}` : ""}` : "Stopped";
  const needs = status === "suspended" && w.requester === r.me;
  const icon = needs ? "need" : { running: "working", suspended: "working", completed: "done" }[status] || "stopped";
  const sub = status === "running" ? statusLineSlot({ task_id: w.task, status_line: line }, "sub why") : `<span class="sub why">${esc(line)}</span>`;
  const thread = w.thread && !r.inThread ? teamHref(r.ws, { kind: "channel", id: r.channel, thread: w.thread }) : undefined;
  return { id: w.task, status, icon, pill: needs ? `<span class="chip warn">Needs you</span>` : chip(status), title: workTitle(w.title), tip: thread ? "Open its thread" : workTitle(w.title), sub, by: who, href: thread };
}

/**
 * A channel's item as the chat view's entry. The server says which kind it
 * is (a person's message, the agent's answer, its work with no words yet, or
 * a deleted message) and which files it carries (crates/magnus/src/team/items.rs);
 * here only the words lose their 📎 lines and names are put to ids.
 * @param {TeamMessage} m @param {TeamReader} r @returns {ChatEntry}
 */
function teamEntry(m, r) {
  if (m.kind === "deleted") return { id: m.id, kind: "deleted", at: m.at, sender: m.sender, replies: m.replies, thread: m.thread };
  const text = String(m.text || "").split("\n").filter(l => !l.startsWith("📎 ")).join("\n");
  return { id: m.id, kind: m.kind, at: m.at, sender: m.sender, text, files: (m.files || []).map(name => ({ name })), edited: m.edited, pinned: m.pinned, reactions: m.reactions, replies: m.replies, last_reply: m.last_reply,
    thread: m.thread, task_id: m.task, private: m.private, requester: m.requester, for: m.kind !== "message" && m.requester ? r.name(m.requester) : undefined, work: m.work ? teamWork(m.work, r) : undefined };
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

/** What only a channel's head has: who is in it, how the agent takes part, notifications, pins, archive. */
function channelTools(home, c, project, pinsOnly) {
  const helpful = c.mode === "when_helpful";
  return `${channelFaces(home, c, project)}${c.direct ? "" : `<label class="small" title="How the team agent takes part here"><span class="sr">Magnus here</span><select data-agent-mode aria-label="How Magnus takes part here" ${home.role === "guest" ? "disabled" : ""}><option value="mentions"${helpful ? "" : " selected"}>Magnus replies to mentions</option><option value="when_helpful"${helpful ? " selected" : ""}>Magnus joins when it can help</option></select></label>`}
      <select data-notify aria-label="Notifications for this channel">${[["all", "All messages"], ["mentions", "Mentions only"], ["off", "Off"]].map(([v, l]) => `<option value="${v}"${c.notify === v ? " selected" : ""}>${l}</option>`).join("")}</select>
      <button type="button" data-pins aria-pressed="${pinsOnly}">${pinsOnly ? "All messages" : "Pinned"}</button>
      <button type="button" data-archive="${c.archived ? "off" : "on"}" title="${c.archived ? "Back to your list" : "Off your list until someone writes here"}">${c.archived ? "Unarchive" : "Archive"}</button>${c.direct ? `<button type="button" class="danger" data-clear-conversation>Delete conversation</button>` : ""}`;
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
    const s = this.s = { ws, channel: place.id, thread: place.thread, center, side, view: { messages: [], typing: [], names: {} }, files: TeamChat.s?.files || {}, pinsOnly: false, whispers: [], hidden: new Set(), lastRead: "", typedAt: 0, views: [] };
    const c = Team.home.channels.find(x => x.id === s.channel);
    if (!c) { paint(center, `<p class="empty">This channel is gone, or not yours to see.</p>`); paint(side, ""); return; }
    s.unreadAtOpen = c.unread;
    center.closest(".team-shell")?.classList.toggle("threaded", !!s.thread);
    s.views = [this.mount(center, null)];
    if (s.thread) s.views.push(this.mount(side, s.thread));
    await this.refresh(true);
    $("textarea", center)?.focus({ preventScroll: true });
  },

  /** The chat view of the channel, or of one of its threads, with its composer. */
  mount(root, thread) {
    const s = this.s, home = Team.home, c = home.channels.find(x => x.id === s.channel);
    const record = MessageDrafts.forThread(`team:${s.ws}:${s.channel}:${thread || ""}`);
    if (!record.text && !record.files.length) record.text = TeamDrafts.get(s.ws, s.channel, thread);
    const view = ChatView.mount(root, teamConversation(s, thread), { side: !!thread, record });
    const where = thread ? "Reply in thread" : c ? `Message ${channelTitle(home, c)}` : "Message";
    const bar = home.role === "guest" ? "" : `<label class="small whisper"><input type="checkbox" data-whisper> Only my Magnus sees this</label>`;
    const slot = $(".composer-slot", view.section);
    paint(slot, composer(`${where}. Mention @${home.agent.name} or name it to ask the team agent.`, bar, { side: !!thread, label: where, mentions: true }));
    wireComposer($("form.say", slot), view.record, teamComposer(s, thread));
    view.section.addEventListener("change", e => {
      if (e.target.matches("[data-agent-mode]")) this.setting("agent", { mode: e.target.value }, x => { x.mode = e.target.value; });
      if (e.target.matches("[data-notify]")) this.setting("notify", { level: e.target.value }, x => { x.notify = e.target.value; });
    });
    $(".conv-head", view.section).onclick = e => this.headClick(e);
    return view;
  },

  async refresh(first = false) {
    const s = this.s;
    if (!s || !s.center.isConnected) return;
    const base = `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}`;
    const [view, whispers] = await Promise.all([api("GET", `${base}/view`), Team.home.role === "guest" ? [] : api("GET", `${base}/whispers`).catch(() => [])]);
    if (s !== this.s) return;
    s.view = view; s.whispers = whispers;
    await this.draw(first);
    const newest = view.messages.at(-1)?.at || "";
    if (newest && newest !== s.lastRead) { s.lastRead = newest; api("POST", `${base}/read`, {}).catch(() => {}); }
  },

  /** Draw the channel (and its open thread) again from what is held. */
  async draw() {
    const s = this.s, c = s && Team.home.channels.find(x => x.id === s.channel);
    if (!c) return;
    await Promise.all(s.views.map(v => v.draw()));
    if (!s.thread) TeamPanel.side(s.side, s.ws, c.project, s.channel);
  },

  /** Ask in this channel, from outside its composer (the command menu, Catch me up). */
  async send(text, thread) {
    const s = this.s, r = await api("POST", `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}/messages`, { text, thread: thread || undefined });
    if (r.agent) toast(`${Team.home.agent.name} is on it.`);
    if (r.limited) toast(r.limited, 8000);
    await this.refresh();
  },

  async headClick(e) {
    const s = this.s, t = e.target;
    if (t.closest("[data-pins]")) { s.pinsOnly = !s.pinsOnly; await this.draw(); return; }
    if (t.closest("[data-archive]")) { await this.archive(t.closest("[data-archive]").dataset.archive === "on"); return; }
    if (t.closest("[data-clear-conversation]")) await this.clear();
  },

  /** Put this conversation away, or bring it back: the caller's list only. */
  async archive(on) {
    const s = this.s;
    try {
      await api("POST", `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}/archive`, { on });
      await loadHome(s.ws);
      if (on) { toast("Archived. It comes back when someone writes in it."); this.away(); } else { toast("Back on your list."); await this.draw(); }
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
    try { await api("POST", `/v1/workspaces/${enc(s.ws)}/channels/${enc(s.channel)}/${what}`, body); } catch (err) { toast(err.message); await loadHome(s.ws); await this.draw(); }
  },
};
