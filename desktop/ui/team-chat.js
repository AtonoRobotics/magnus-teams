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
 * A channel's items as the server sends them: the personal chat's entries
 * (crates/magnus/src/chat_entry.rs, built in crates/magnus/src/team/items.rs).
 * A request's work is a personal task row, with whom it is for and where it shows.
 * @typedef {{ task_id: string, title: string, text: string, status: "running"|"suspended"|"completed"|"failed"|"cancelled", waiting_on: ?string, status_line: ?string, requester: string, steps: number, doing: ?string, long: boolean, thread: ?string, started: string }} TeamWork
 * @typedef {{ id: string, name: string, media_type?: string, size?: number }} TeamFile
 * @typedef {{ id: string, kind: "message"|"answer"|"tasks"|"deleted", files?: TeamFile[], sender: string, requester?: ?string, text: string, thread: ?string, at: string, edited?: boolean, pinned?: boolean, reactions?: Object<string, string[]>, replies?: number, last_reply?: string, work?: TeamWork, tasks?: TeamWork[], from_run?: string, task?: string, task_id?: string, private?: boolean, deleted?: boolean, may?: May, for?: string }} TeamMessage
 * @typedef {{ messages: TeamMessage[], typing: string[], names: Object<string, string>, caps?: ConversationCaps }} TeamView
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
  // What it is doing comes in the server's words (crates/magnus/src/tool_registry), as a personal task's card has them.
  const line = status === "running" ? (w.status_line || (w.doing ? "Working" : "Thinking it through"))
    : status === "suspended" ? `Waiting for ${who} to answer an ask`
    : status === "completed" ? "Done" : status === "failed" ? `Could not finish${w.waiting_on ? `: ${w.waiting_on}` : ""}` : "Stopped";
  const needs = status === "suspended" && w.requester === r.me;
  const icon = needs ? "need" : { running: "working", suspended: "working", completed: "done" }[status] || "stopped";
  const sub = status === "running" ? statusLineSlot({ task_id: w.task_id, status_line: line }, "sub why") : `<span class="sub why">${esc(line)}</span>`;
  const thread = w.thread && !r.inThread ? teamHref(r.ws, { kind: "channel", id: r.channel, thread: w.thread }) : undefined;
  return { id: w.task_id, status, icon, pill: needs ? `<span class="chip warn">Needs you</span>` : chip(status), title: workTitle(w.title), tip: thread ? "Open its thread" : workTitle(w.title), sub, by: who, href: thread };
}

/**
 * A channel's item as the chat view's entry. The server sends the personal
 * chat's entry (a person's message, the agent's answer, the tasks it began
 * with no words yet, or a deleted message's place): its words without the
 * files' lines, its files, and whom an answer is for by name (`for`). Here
 * only work is drawn as the chat view's work card.
 * @param {TeamMessage} m @param {TeamReader} r @returns {ChatEntry}
 */
function teamEntry(m, r) {
  if (m.kind === "deleted") return { id: m.id, kind: "deleted", at: m.at, sender: m.sender, replies: m.replies, thread: m.thread };
  const work = m.work || (m.kind === "tasks" ? m.tasks?.[0] : undefined);
  return { ...m, kind: m.kind === "tasks" ? "work" : m.kind, files: m.files || [], work: work ? teamWork(work, r) : undefined };
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

/** Minimal work-focused shared conversation header: faces, user's personal computer chip, Members chip, and quiet Auto-join toggle. */
function channelTools(home, c, project) {
  const helpful = c.mode === "when_helpful";
  const ext = typeof TeamRooms !== "undefined" ? TeamRooms.renderSlot("headerActions", { ws: home.id, channel: c.id }) : "";
  const comp = typeof computerChip === "function" ? computerChip(App.computers) : "";
  const autoJoin = c.direct ? "" : `<button type="button" class="shared-autojoin-btn${helpful ? " on" : ""}" data-agent-toggle aria-pressed="${helpful}" ${home.role === "guest" ? "disabled" : ""} title="${helpful ? "Magnus auto-joins when helpful (click for @Magnus mentions only)" : "Magnus replies to @Magnus mentions (click to auto-join when helpful)"}">${helpful ? "Auto-join: On" : "Auto-join"}</button>`;
  const membersBtn = `<button type="button" class="shared-members-btn" data-open="shared:settings" aria-label="Members and project settings" title="Members and project settings">${icon("team")}<span>${home.members.length}</span></button>`;
  const clearDm = c.direct ? `<button type="button" class="icon-btn" data-clear-conversation title="Delete conversation" aria-label="Delete conversation">${icon("trash")}</button>` : "";
  return `${channelFaces(home, c, project)}${ext}${comp}${autoJoin}${membersBtn}${clearDm}`;
}

/** Drafts survive a switch of conversation and a restart, where storage allows. */
const TeamDrafts = {
  key: (ws, ch, th) => `team-draft:${ws}:${ch}:${th || ""}`,
  get(ws, ch, th) { try { return localStorage.getItem(this.key(ws, ch, th)) || ""; } catch { return ""; } },
  set(ws, ch, th, v) { try { if (v) localStorage.setItem(this.key(ws, ch, th), v); else localStorage.removeItem(this.key(ws, ch, th)); } catch { /* storage blocked: the draft lives in the box */ } },
};

const TeamChat = {
  s: null,

  async open(root, ws, place) {
    const s = this.s = { ws, channel: place.id, thread: place.thread || null, root, view: { messages: [], typing: [], names: {} }, files: TeamChat.s?.files || {}, whispers: [], hidden: new Set(), lastRead: "", typedAt: 0, views: [] };
    const c = Team.home.channels.find(x => x.id === s.channel);
    if (!c) { paint(root, `<p class="empty">This conversation is gone, or not yours to see.</p>`); return; }
    s.unreadAtOpen = c.unread;
    s.views = [this.mount(root, s.thread)];
    await this.refresh(true);
    $("textarea", root)?.focus({ preventScroll: true });
  },

  /** The unified chat view of a shared conversation (or focused thread) with its composer. */
  mount(root, thread) {
    const s = this.s, home = Team.home, c = home.channels.find(x => x.id === s.channel);
    const record = MessageDrafts.forThread(`team:${s.ws}:${s.channel}:${thread || ""}`);
    if (!record.text && !record.files.length) record.text = TeamDrafts.get(s.ws, s.channel, thread);
    const view = ChatView.mount(root, teamConversation(s, thread), { record });
    const where = thread ? "Reply in thread" : c ? `Message ${channelTitle(home, c)}` : "Message";
    const guest = home.role === "guest";
    // The personal composer's model picker, first in the bar as there: a
    // member's request runs on their pick (decisions/2026-11-13-team-composer-model.md).
    const bar = `<span data-picker></span>${guest ? "" : `<label class="small whisper"><input type="checkbox" data-whisper> Only my Magnus sees this</label>`}`;
    const slot = $(".composer-slot", view.section);
    paint(slot, composer(`${where}. Mention @${home.agent.name} to have your Magnus help and share its output.`, bar, { label: where, mentions: true }));
    modelPicker().then(html => {
      const at = $("[data-picker]", slot);
      if (!at) return;
      at.outerHTML = html;
      // A guest's messages never start the agent, so there is no model to pick.
      const sel = $("select[data-model]", slot);
      if (guest && sel) { sel.disabled = true; sel.title = "Guests chat here; only members can ask Magnus, so no model runs for you."; }
    });
    wireComposer($("form.say", slot), view.record, teamComposer(s, thread));
    $(".conv-head", view.section).onclick = e => this.headClick(e);
    return view;
  },

  async refresh(first = false) {
    const s = this.s;
    if (!s || !s.root?.isConnected) return;
    const [view, whispers] = await Promise.all([Server.channels.view(s.ws, s.channel), Team.home.role === "guest" ? [] : Server.channels.whispers(s.ws, s.channel).catch(() => [])]);
    if (s !== this.s) return;
    s.view = view; s.whispers = whispers;
    await this.draw(first);
    const newest = view.messages.at(-1)?.at || "";
    if (newest && newest !== s.lastRead) { s.lastRead = newest; Server.channels.read(s.ws, s.channel).catch(() => {}); }
  },

  /** Draw the shared conversation again from what is held. */
  async draw() {
    const s = this.s, c = s && Team.home.channels.find(x => x.id === s.channel);
    if (!c) return;
    await Promise.all(s.views.map(v => v.draw()));
  },

  /** Ask in this conversation, from outside its composer (Catch me up). */
  async send(text, thread) {
    const s = this.s, r = await Server.channels.post(s.ws, s.channel, { text, thread: thread || undefined });
    if (r.agent) toast(`${Team.home.agent.name} is on it.`);
    if (r.limited) toast(r.limited, 8000);
    await this.refresh();
  },

  async headClick(e) {
    const s = this.s, t = e.target;
    if (t.closest("[data-agent-toggle]")) {
      const c = Team.home.channels.find(x => x.id === s.channel);
      if (!c || Team.home.role === "guest") return;
      const next = c.mode === "when_helpful" ? "mentions" : "when_helpful";
      await this.setting("agent", { mode: next }, x => { x.mode = next; });
      await this.draw();
      toast(next === "when_helpful" ? "Magnus will auto-join when helpful." : "Magnus will reply to @Magnus mentions.");
      return;
    }
    if (t.closest("[data-clear-conversation]")) await this.clear();
  },

  /** Off this conversation, now it is off the list: to the workspace's
   * first conversation still on it, else Shared projects. */
  away() {
    const s = this.s, next = firstChannel(Team.home);
    location.hash = next && next !== s.channel ? teamHref(s.ws, { kind: "channel", id: next }) : "#/teams";
  },

  /** Delete a direct conversation from the caller's own list and view. */
  async clear() {
    const s = this.s;
    if (!(await ask("Delete this conversation from your list? What it holds now is gone for you. The others in it keep theirs.", "Delete"))) return;
    try {
      await Server.channels.clear(s.ws, s.channel);
      await loadHome(s.ws);
      toast("Deleted from your list.");
      this.away();
    } catch (err) { toast(err.message); }
  },

  async setting(what, body, local) {
    const s = this.s, c = Team.home.channels.find(x => x.id === s.channel);
    if (c) local(c);
    try { await Server.channels.act(s.ws, s.channel, what, body); } catch (err) { toast(err.message); await loadHome(s.ws); await this.draw(); }
  },
};
