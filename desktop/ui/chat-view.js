// The one conversation (owner, 2026-11-13: "Teams is a multi user version of
// the single chat interface"; 2026-11-22: a shared project's chat is the
// personal chat with many users, one UI, the same backend unless truly
// needed). Every conversation opens here (Conversation.open): the personal
// chat, a shared conversation and its threads. A conversation brings only
// its source: where its items come from and what each action does there (a
// ChatConversation: personal-chat.js, or a shared project's,
// team-conversation.js), and where a message and a file go (a
// ComposerConversation). The layout, the messages, the work cards, the
// controls, scrolling, the status line, the composer (chat-composer.js), the
// draft (MessageDrafts, in memory only: #544) and the Stop of running work
// are this one code. What only a conversation of several people has (who
// sent each message, mentions, reactions, threads, the requester named, an
// answer only its requester sees) is drawn when the conversation names its
// people, and is simply absent when it does not.

/**
 * One item of a conversation, as the view draws it. The personal chat's
 * entries come as the server sends them; a team channel's items are read
 * into the same kinds (team-conversation.js). Multi-user fields are absent in the
 * personal chat.
 * @typedef {{ id: string, kind: string, at?: string, text?: string, files?: any[], task_id?: string, model?: { model: string }, view?: any,
 *   sender?: string, for?: string, edited?: boolean, pinned?: boolean, private?: boolean, replies?: number, work?: ChatWork, [k: string]: any }} ChatEntry
 * A piece of work as a row: the personal chat's task, or a team request.
 * @typedef {{ id: string, status: string, icon: string, pill: string, title: string, tip?: string, sub: string, href?: string, open?: string, opened?: boolean, stop?: boolean, by?: string, requester?: string, after?: string }} ChatWork
 * What the view keeps between draws.
 * @typedef {{ editing: ?string, sharing: ?string, declining: ?string, people: Array<{ id: string, name: string }>, live: Object<string, string>, [k: string]: any }} ChatUi
 * A mounted view.
 * @typedef {{ conv: ChatConversation, section: HTMLElement, list: HTMLElement, ui: ChatUi, entries: ChatEntry[], revision: number, record?: any, draw: () => Promise<void> }} ChatViewState
 * What a conversation has beside its items, as the server sends it on both
 * views (crates/magnus/src/conversation/mod.rs, `Caps`): the view draws from
 * these, not from which hooks a conversation happens to have.
 * @typedef {{ several?: boolean, reactions?: boolean, pins?: boolean, threads?: boolean, whisper?: boolean, typing?: boolean }} ConversationCaps
 * A conversation, to the view. `caps().several` makes it a conversation of
 * several people, with `name` to say who an id is; the rest of the
 * multi-user hooks are optional.
 * @typedef {Object} ChatConversation
 * @property {(view: ChatViewState) => Promise<?ChatEntry[]>} load
 * @property {(e: ChatEntry, view: ChatViewState) => ?ControlledMessage} describe what a message is, to its controls
 * @property {() => ConversationCaps & ControlCaps} caps what the conversation has, and what the viewer may do here
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
  const several = conv => !!conv.caps?.().several && typeof conv.name === "function";

  /**
   * The conversation's frame: its head, the items, Jump to latest, the
   * status line and the composer. The main one keeps the ids the app and
   * its tests know; a second one beside it (a thread) has none.
   * @param {{ side?: boolean, banners?: boolean }} [o]
   */
  function layout(o = {}) {
    const id = n => o.side ? "" : ` id="${n}"`;
    return `<section class="conv${o.side ? " side" : ""}"><header class="conv-head"${id("conv-head")}></header>${o.banners ? `<div id="banners"></div>` : ""}<div${id("thread")} class="thread" aria-live="off"></div><button type="button" class="jump-latest" data-jump-latest hidden>New activity · Jump to latest</button><div class="chat-status small faint" aria-live="polite"></div><div class="sr" data-chat-announce aria-live="polite" aria-atomic="true"></div><div${id("composer")} class="composer-slot"></div></section>`;
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

  /** @param {string} text @param {ChatConversation} conv */
  function answerBody(text, conv) {
    if (typeof UiSpec === "undefined") return marked(Compose.markdown(text), conv);
    return UiSpec.split(text).map(p => p.kind === "text"
      ? marked(Compose.markdown(p.text), conv)
      : `<div class="ui-block" data-ui-kind="${esc(p.kind)}" data-ui-id="${esc(p.id || "")}" data-ui-body="${esc(p.body)}" data-ui-closed="${p.closed ? "1" : "0"}"></div>`).join("");
  }

  /** The agent's answer. @param {ChatEntry} e @param {ChatUi} ui @param {ChatConversation} conv @param {ChatViewState} view */
  function answer(e, ui, conv, view) {
    const extra = `${e.model ? ` · ${esc(e.model.model)}` : ""}${e.for ? ` · for ${esc(e.for)}` : ""}`;
    return `<div class="msg magnus" data-answer data-task="${esc(e.task_id)}">${byline(e, conv, extra)}<div class="answer-body">${answerBody(e.text || "", conv)}</div>${e.view ? shownFrame(e.id, "What Magnus showed") : ""}${conv.note?.(e, view) || ""}</div>`;
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
    const conv = view.conv, items = entries.map((e, i) => `${conv.beforeItem?.(e, i, entries, view) || ""}${item(e, view)}`).join("");
    const customHero = conv.hero && typeof conv.hero.render === "function" ? conv.hero.render() : null;
    const empty = conv.hero ? (customHero || `<div class="hello-hero"><h1 class="hello-title">${esc(conv.hero.title())}</h1><p class="hello-sub">${esc(conv.hero.subtitle)}</p><div class="hello-chips">${conv.hero.starters.map(s => `<button type="button" class="hello-chip">${esc(s)}</button>`).join("")}</div></div>`)
      : conv.hello ? `<p class="muted hello">${esc(conv.hello)}</p>` : "";
    return `${conv.before?.(view) || ""}${items || empty}${conv.after?.(view) || ""}`;
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
    const newEntries = ui.entryIds ? entries.filter(e => e.id && !ui.entryIds.has(e.id)) : [];
    const newActivity = newEntries.length > 0;
    const newAnswer = newEntries.some(e => e.kind === "answer" || (e.sender === "agent" && e.text));
    ui.entryIds = new Set(entries.map(e => e.id)); ui.drawn = true;
    const painted = paint(el, listHtml(entries, view));
    if (painted) {
      liveStatusLines(el);
      ReadingAnchors.restore(el, anchor); ReadingAnchors.remember(el, anchor);
      if (newActivity && !anchor.follow) $("[data-jump-latest]", view.section).hidden = false;
      motion(el, ui);
      conv.drawn?.(el, entries, view);
    }
    if (newAnswer) {
      const announce = $("[data-chat-announce]", view.section);
      if (announce) {
        const who = conv.agentName || "Magnus";
        announce.textContent = "";
        announce.textContent = `${who} replied.`;
      }
    }
    paint($(".chat-status", view.section), conv.status?.(view) || "");
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
      const starter = ev.target.closest(".hello-chip");
      if (starter) {
        const box = $("textarea", view.section);
        if (box) { box.value = starter.dataset.starterPrompt || starter.textContent; box.dispatchEvent(new Event("input", { bubbles: true })); box.focus(); }
        return;
      }
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

// Unsent messages, kept in memory for this session: what was typed and the
// files attached survive redraws and moving between views. One per
// conversation, personal or shared, and one for a new conversation in each
// project. Nothing is written to the device; signing out forgets them, and
// another person signing in on this computer starts with none (#544).
const MessageDrafts = (() => {
  let person = null, rows = {}, epoch = 0;
  const listeners = new Set(), originals = new Map();
  const emit = () => { for (const f of listeners) f(); };
  const row = (id, thread = null, project = null) => rows[id] ||= {id, thread, project, text:"", files:[], selection:[0,0], anchor:null};
  const forThread = (thread, project = null) => row(thread ? `thread:${thread}` : `new:${project || ""}`, thread, project);
  // An earlier version kept a shared project's unsent message on the device
  // (`team-draft:` keys), for whoever signed in next: those go.
  function forgetDevice() {
    try {
      const old = [];
      for (let i = 0; i < localStorage.length; i++) { const k = localStorage.key(i); if (k?.startsWith("team-draft:")) old.push(k); }
      for (const k of old) localStorage.removeItem(k);
    } catch { /* storage blocked: nothing was kept there either */ }
  }
  function clear() { rows = {}; originals.clear(); epoch++; forgetDevice(); emit(); }
  // Another person signing in on this computer starts with nothing.
  function open(appliance, who) { const next = `${appliance}/${who}`; forgetDevice(); if (next !== person) { person = next; clear(); } }
  function discard(id) { if (!rows[id]) return; for (const f of rows[id].files) originals.delete(f.uid); delete rows[id]; emit(); }
  return {forThread, forKey:id=>row(id), open, clear, discard, changed:emit, uid:()=>crypto.randomUUID(),
    get:id=>rows[id], all:()=>Object.values(rows), originals, epoch:()=>epoch,
    subscribe:f=>{listeners.add(f);return()=>listeners.delete(f);}};
})();

/**
 * Where a conversation is, to find its draft: a personal thread (or a
 * project's new conversation), or a shared project's conversation or thread.
 * @typedef {{ thread?: ?string, project?: ?string, ws?: string, channel?: string }} ConversationPlace
 * What a conversation opens with.
 * @typedef {Object} ConversationSpec
 * @property {ChatConversation} conv where its items come from, and what each control does there
 * @property {ComposerConversation} side where its messages and files go
 * @property {any} record its draft (Conversation.draft)
 * @property {string} placeholder what the empty box says
 * @property {string} [label] the box's name, where it differs
 * @property {string} [bar] what follows the model picker (Only my Magnus sees this)
 * @property {boolean} [mentions] typing @ offers who to mention
 * @property {string} [noModel] no model runs for this viewer here, and why: the picker shows, disabled
 * @property {boolean} [framed] the frame is drawn already
 * @property {() => boolean} [stale] the person moved on while the picker loaded
 */

const Conversation = {
  /** The conversation's one draft, in memory only. @param {ConversationPlace} place */
  draft(place) {
    return place.ws ? MessageDrafts.forKey(`shared:${place.ws}:${place.channel}:${place.thread || ""}`) : MessageDrafts.forThread(place.thread || null, place.project || null);
  },

  /**
   * Show a conversation in `root`: the chat view, then the composer with the
   * model picker, on its draft. Null when the person moved on meanwhile.
   * @param {HTMLElement} root @param {ConversationSpec} o @returns {Promise<?ChatViewState>}
   */
  async open(root, o) {
    const picker = await modelPicker();
    if (o.stale?.()) return null;
    const view = ChatView.mount(root, o.conv, { framed: o.framed, record: o.record });
    const slot = $(".composer-slot", view.section);
    paint(slot, composer(o.placeholder, picker + (o.bar || ""), { label: o.label, mentions: o.mentions }));
    const sel = $("select[data-model]", slot);
    if (o.noModel && sel) { sel.disabled = true; sel.title = o.noModel; }
    wireComposer($("form.say", slot), o.record, o.side);
    return view;
  },

  /**
   * What is running here now, after a draw, for the composer's Stop and
   * Steer. `settled`: the conversation has answered since the last send. A
   * Stop pressed before the work had an id stops it now that it has one.
   * @param {ChatViewState} view @param {string[]} running @param {boolean} settled @param {() => Promise<any>} refresh
   */
  track(view, running, settled, refresh) {
    const record = view.record;
    if (!record) return;
    record.runningIds = running;
    if (running.length || settled) record.pendingRun = false;
    if (record.cancelRequested && running.length) {
      record.cancelRequested = false;
      Conversation.stop(running, refresh).catch(() => {});
    }
    MessageDrafts.changed();
  },

  /** Stop the work running here. @param {string[]} ids @param {() => Promise<any>} refresh */
  async stop(ids, refresh) {
    await Promise.all(ids.map(id => Server.tasks.cancel(id)));
    toast("Stopped.");
    await refresh();
  },
};
if (typeof module !== "undefined") module.exports = ChatView;
