// The one chat view (owner, 2026-11-13: "Teams is a multi user version of
// the single chat interface"; "Not the same system"). Every conversation is
// drawn here: the personal chat, a team channel and a team thread. A
// conversation says only where its items come from and what each action does
// there (a ChatConversation); the layout, the messages, the work cards, the
// controls, scrolling, the status line and the composer (chat-composer.js)
// are this one code. What only a conversation of several people has (who
// sent each message, mentions, reactions, threads, the requester named, an
// answer only its requester sees) is drawn when the conversation names its
// people, and is simply absent when it does not.

/**
 * One item of a conversation, as the view draws it. The personal chat's
 * entries come as the server sends them; a team channel's items are read
 * into the same kinds (team-chat.js). Multi-user fields are absent in the
 * personal chat.
 * @typedef {{ id: string, kind: string, at?: string, text?: string, files?: any[], task_id?: string, model?: { model: string }, view?: any,
 *   sender?: string, for?: string, edited?: boolean, pinned?: boolean, private?: boolean, replies?: number, work?: ChatWork, [k: string]: any }} ChatEntry
 * A piece of work as a row: the personal chat's task, or a team request.
 * @typedef {{ id: string, status: string, icon: string, pill: string, title: string, tip?: string, sub: string, href?: string, open?: string, opened?: boolean, stop?: boolean, by?: string, after?: string }} ChatWork
 * What the view keeps between draws.
 * @typedef {{ editing: ?string, sharing: ?string, declining: ?string, people: Array<{ id: string, name: string }>, live: Object<string, string>, [k: string]: any }} ChatUi
 * A mounted view.
 * @typedef {{ conv: ChatConversation, section: HTMLElement, list: HTMLElement, ui: ChatUi, entries: ChatEntry[], revision: number, record?: any, draw: () => Promise<void> }} ChatViewState
 * A conversation, to the view. `people` and `name` make it a conversation
 * of several people; the rest of the multi-user hooks are optional.
 * @typedef {Object} ChatConversation
 * @property {(view: ChatViewState) => Promise<?ChatEntry[]>} load
 * @property {(e: ChatEntry, view: ChatViewState) => ?ControlledMessage} describe what a message is, to its controls
 * @property {() => ControlCaps} caps what the viewer may do here
 * @property {(e: ChatEntry, view: ChatViewState, b: HTMLElement, el: HTMLElement) => Object<string, Function>} actions the controls' adapter
 * @property {string} [hello] what an empty conversation says
 * @property {string} [me] the viewer, in a conversation of several people
 * @property {string} [agentName] the agent's name there
 * @property {(id: string) => string} [name] who an id is: present only with several people
 * @property {() => string[]} [names] everyone who may be mentioned here
 * @property {(e: ChatEntry) => string} [voice] the read-aloud key of an answer
 * @property {(f: any) => string} [file] an attached file as it shows
 * @property {(e: ChatEntry, view: ChatViewState) => string} [lead] its own controls, first in the row (React, Reply in thread)
 * @property {(e: ChatEntry, view: ChatViewState) => string} [extras] under the message (reactions, replies)
 * @property {(e: ChatEntry, view: ChatViewState) => string} [note] inside an answer (only you can see this)
 * @property {(view: ChatViewState) => string} [before] above the items
 * @property {(view: ChatViewState) => string} [after] below the items
 * @property {(view: ChatViewState) => string} [status] the line under the conversation (who is typing)
 * @property {(entries: ChatEntry[], view: ChatViewState) => Promise<void>} [prepare] before a draw
 * @property {(el: HTMLElement, entries: ChatEntry[], view: ChatViewState) => void} [drawn] after a draw
 * @property {(ev: MouseEvent, view: ChatViewState) => any} [click] its own buttons
 */

const ChatView = (() => {
  const several = conv => typeof conv.name === "function";

  /**
   * The conversation's frame: its head, the items, Jump to latest, the
   * status line and the composer. The main one keeps the ids the app and
   * its tests know; a second one beside it (a thread) has none.
   * @param {{ side?: boolean, banners?: boolean }} [o]
   */
  function layout(o = {}) {
    const id = n => o.side ? "" : ` id="${n}"`;
    return `<section class="conv${o.side ? " side" : ""}"><header class="conv-head"${id("conv-head")}></header>${o.banners ? `<div id="banners"></div>` : ""}<div${id("thread")} class="thread" aria-live="off"></div><button type="button" class="jump-latest" data-jump-latest hidden>New activity · Jump to latest</button><div class="chat-status small faint" aria-live="polite"></div><div${id("composer")} class="composer-slot"></div></section>`;
  }

  /**
   * Above the conversation: where it belongs (a link), its title, and what
   * can be done to the whole of it.
   * @param {{ where?: string, title: string, tip?: string, tools?: string }} h
   */
  function head(h) {
    return `<div class="conv-title"><div class="small muted">${h.where || ""}</div>
    <h1 title="${esc(h.tip ?? h.title)}">${esc(h.title)}</h1></div>
    <div class="row">${h.tools || ""}</div>`;
  }

  /** @param {ChatEntry} e @param {ChatConversation} conv */
  function mine(e, conv) { return !several(conv) || e.sender === conv.me; }

  /** Who wrote it and when, over the message. @param {ChatEntry} e @param {ChatConversation} conv @param {string} [extra] */
  function byline(e, conv, extra = "") {
    const agent = e.kind !== "message";
    const who = agent ? (several(conv) ? conv.agentName || "Magnus" : "Magnus") : mine(e, conv) ? "You" : conv.name(e.sender);
    const face = several(conv) && typeof avatar === "function" ? avatar(who === "You" ? conv.name(conv.me) : who, agent) : "";
    return `<div class="small faint byline">${face}${esc(who)}${extra} · ${esc(when(e.at))}${e.edited ? " · edited" : ""}${e.pinned ? " · pinned" : ""}</div>`;
  }

  /** Mentions marked, in a conversation of several people. @param {string} html @param {ChatConversation} conv */
  function marked(html, conv) {
    if (!several(conv)) return html;
    // A name of two words is one mention only when someone here has it:
    // "@Dana Smith" is Dana Smith, "@Magnus Plan the launch" is Magnus.
    const names = new Set((conv.names?.() || []).map(n => n.toLowerCase()));
    return html.replace(/(^|[\s(>])@([A-Za-z][\w.-]*)( [A-Z][\w-]*)?/g, (all, pre, first, second = "") =>
      second && names.has(`${first}${second}`.toLowerCase()) ? `${pre}<span class="mention">@${first}${second}</span>` : `${pre}<span class="mention">@${first}</span>${second}`);
  }

  /** @param {ChatEntry} e @param {ChatConversation} conv */
  function files(e, conv) {
    return e.files?.length ? `<div class="previews">${e.files.map(conv.file || filePreview).join("")}</div>` : "";
  }

  /** A person's message, or its edit box. @param {ChatEntry} e @param {ChatUi} ui @param {ChatConversation} conv */
  function message(e, ui, conv) {
    if (ui.editing === e.id) return `<div class="msg me editing"><label class="sr" for="edit-${esc(e.id)}">Edit and resend</label><textarea id="edit-${esc(e.id)}" rows="3" data-draft="edit-${esc(e.id)}"></textarea>
        <div class="row small"><button class="primary" data-msg-act="edit-send">Send</button><button class="link" data-msg-act="edit-cancel">Cancel</button></div></div>`;
    return `<div class="msg ${mine(e, conv) ? "me" : "person"}">${byline(e, conv)}<div>${marked(Compose.maskedText(e.text), conv)}</div>${files(e, conv)}</div>`;
  }

  /** The agent's answer. @param {ChatEntry} e @param {ChatUi} ui @param {ChatConversation} conv @param {ChatViewState} view */
  function answer(e, ui, conv, view) {
    const extra = `${e.model ? ` · ${esc(e.model.model)}` : ""}${e.for ? ` · for ${esc(e.for)}` : ""}`;
    return `<div class="msg magnus" data-answer data-task="${esc(e.task_id)}">${byline(e, conv, extra)}<div class="answer-body">${marked(Compose.markdown(e.text || ""), conv)}</div>${e.view ? shownFrame(e.id, "What Magnus showed") : ""}${conv.note?.(e, view) || ""}</div>`;
  }

  /**
   * A piece of work as a row that opens it, its status live; Stop while it
   * runs where the viewer may stop it.
   * @param {ChatWork} w
   */
  function workRow(w) {
    const main = `<span class="st ${w.icon}">${icon(w.icon)}</span><span class="rg-main"><span class="ttl">${esc(w.title)}</span>${w.by ? ` <span class="sub by">for ${esc(w.by)}</span>` : ""}${w.sub}</span>${w.pill}`;
    const row = w.href ? `<a class="rg-row${w.opened ? " open" : ""}" href="${esc(w.href)}"${w.open ? ` data-open="${esc(w.open)}"` : ""} title="${esc(w.tip ?? w.title)}">${main}${icon("chev", "chev")}</a>` : `<div class="rg-row" title="${esc(w.tip ?? w.title)}">${main}</div>`;
    return `<li class="tcard" data-chat-task="${esc(w.id)}" data-status="${esc(w.status)}">${row}
    ${w.stop ? `<div class="card-acts"><button class="link small" data-task-act="cancel" data-task="${esc(w.id)}">Stop</button></div>` : ""}${w.after || ""}</li>`;
  }

  /** One request's work, as the personal chat's "From this request" card. @param {ChatWork} w */
  function workCard(w) {
    return `<div class="reqgroup chatcard" data-chat-work="${esc(w.id)}"><ul class="rg-rows">${workRow(w)}</ul></div>`;
  }

  /** Where a deleted message stood, when replies still hang from it. */
  function deleted() {
    return `<div class="msg person deleted"><div class="small">This message was deleted.</div></div>`;
  }

  /** @type {Object<string, (e: ChatEntry, ui: ChatUi, conv: ChatConversation, view: ChatViewState) => string>} */
  const KINDS = { message, answer, deleted, work: () => "" };

  /** The controls under an item. @param {ChatEntry} e @param {ChatViewState} view */
  function controls(e, view) {
    const conv = view.conv, m = conv.describe(e, view), lead = conv.lead?.(e, view) || "";
    if (!m) return lead ? `<div class="msg-actions small" role="group" aria-label="Message actions">${lead}</div>` : "";
    const key = conv.voice ? conv.voice(e) : e.id, reading = typeof voice !== "undefined" && voice.entry === key;
    const list = MessageControls.controls(m, { ...conv.caps(), canReadAloud: App.canReadAloud, reading });
    return MessageControls.html(menuId(e.id), list, { voiceEntry: key, lead }) + (view.ui.sharing === e.id ? MessageControls.shareRow(`share-${e.id}`, view.ui.people) : "");
  }

  /** The id of an item's "More message actions" menu. @param {string} id */
  function menuId(id) { return `message-menu-${enc(id)}`; }

  /** One item, whatever the conversation. @param {ChatEntry} e @param {ChatViewState} view */
  function item(e, view) {
    const conv = view.conv, ui = view.ui;
    const body = KINDS[e.kind] ? KINDS[e.kind](e, ui, conv, view) : entry(e, ui);
    const side = e.kind === "message" && mine(e, conv) ? "me" : "them";
    return `<div class="entry ${side}" data-entry="${esc(e.id || "")}" data-kind="${esc(e.kind)}">${body}${e.work ? workCard(e.work) : ""}${conv.extras?.(e, view) || ""}${e.kind === "deleted" ? "" : controls(e, view)}</div>`;
  }

  /** The whole list, as it is drawn. @param {ChatEntry[]} entries @param {ChatViewState} view */
  function listHtml(entries, view) {
    const conv = view.conv, items = entries.map(e => item(e, view)).join("");
    return `${conv.before?.(view) || ""}${items || (conv.hello ? `<p class="muted hello">${esc(conv.hello)}</p>` : "")}${conv.after?.(view) || ""}`;
  }

  /** Draw the conversation again, keeping the reader's place. @param {ChatViewState} view */
  async function draw(view) {
    const revision = ++view.revision, conv = view.conv;
    const entries = await conv.load(view);
    if (!entries || revision !== view.revision || !view.list.isConnected) return;
    view.entries = entries;
    await conv.prepare?.(entries, view);
    const el = view.list, ui = view.ui;
    const anchor = ui.drawn ? ReadingAnchors.capture(el) : view.record?.anchor || { follow: true };
    const newActivity = ui.entryIds && entries.some(e => e.id && !ui.entryIds.has(e.id));
    ui.entryIds = new Set(entries.map(e => e.id)); ui.drawn = true;
    paint(el, listHtml(entries, view)); liveStatusLines(el);
    ReadingAnchors.restore(el, anchor); ReadingAnchors.remember(el, anchor);
    if (newActivity && !anchor.follow) $("[data-jump-latest]", view.section).hidden = false;
    motion(el, ui);
    paint($(".chat-status", view.section), conv.status?.(view) || "");
    conv.drawn?.(el, entries, view);
  }

  /**
   * Show a conversation in `root`: its frame, its items kept in place as
   * they change, and its controls run on the conversation's adapter.
   * @param {HTMLElement} root @param {ChatConversation} conv @param {{ side?: boolean, banners?: boolean, record?: any, framed?: boolean }} [o] framed: the frame is drawn already
   * @returns {ChatViewState}
   */
  function mount(root, conv, o = {}) {
    if (!o.framed) paint(root, layout(o));
    const section = root.matches?.("section.conv") ? root : $("section.conv", root);
    /** @type {ChatViewState} */
    const view = { conv, section, list: $(".thread", section), ui: { editing: null, sharing: null, declining: null, people: [], live: {} }, entries: [], revision: 0, record: o.record, draw: () => draw(view) };
    $("[data-jump-latest]", section).onclick = () => {
      const list = view.list; list.scrollTop = list.scrollHeight; ReadingAnchors.remember(list);
      if (view.record) view.record.anchor = { follow: true };
      $("[data-jump-latest]", section).hidden = true;
    };
    onLeave(ReadingAnchors.watch(view.list, view.record));
    view.list.onclick = async ev => {
      const b = ev.target.closest("[data-msg-act]");
      if (b) {
        const el = b.closest("[data-entry]"), e = view.entries.find(x => x.id === el?.dataset.entry);
        if (!e) return;
        try { await MessageControls.run(b.dataset.msgAct, conv.actions(e, view, b, el)); } catch (err) { toast(err.message); b.disabled = false; }
        return;
      }
      return conv.click?.(ev, view);
    };
    return view;
  }

  return { layout, head, mount, draw, item, listHtml, workRow, workCard, menuId, several };
})();
if (typeof module !== "undefined") module.exports = ChatView;
