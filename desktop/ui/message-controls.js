// One set of message controls for both chats, the personal one and a team
// channel (owner, 2026-11-13: "the same message controls as the other chat
// window"). Which controls a message has is decided here once, from what it
// is and who is looking; the icons and the menu are drawn here once; and a
// chosen control is run here against the surface's own adapter, which does
// the work its own way (the personal chat's routes, or the team's). The
// Feedback dialog lives here too, so both open and close it alike.

/**
 * What a message is, to its controls.
 * What the server says the viewer may do with an entry (crates/magnus/src/chat_entry.rs, `May`):
 * the one rule both chats' controls follow, decided in the team by the member view.
 * @typedef {{ edit: boolean, retry: boolean, feedback: boolean, share: boolean, save: boolean, task: boolean, pin: boolean, delete: boolean }} May
 * @typedef {{ mine?: boolean, agent?: boolean, readable?: boolean, pinned?: boolean, may: May }} ControlledMessage
 * mine: the viewer wrote it. agent: Magnus did. readable: it can be read aloud.
 * How the viewer is reading.
 * @typedef {{ canReadAloud?: boolean, reading?: boolean }} ControlCaps
 * One control: its action, its name, and whether it sits at hand or in the menu.
 * @typedef {{ act: string, label: string, primary: boolean }} Control
 * What the Feedback dialog needs: the task to judge, the verdict so far,
 * and, where a surface has them, quick reactions.
 * @typedef {{ task?: ?string, verdict?: any, reaction?: ?string, react?: (a: string) => Promise<any>, focus?: () => any }} FeedbackOptions
 * Where a conversation's message actions go: the personal chat's routes or
 * the team's. Each conversation gives every one of these, so the two tables
 * have the same keys; what an action means there is its own (decisions/2026-11-14-one-conversation.md).
 * @typedef {Object} ChatTransport
 * @property {(e: any) => string} text the words Copy takes
 * @property {(e: any, text: string) => Promise<any>} resend Edit's send
 * @property {(e: any) => Promise<any>} retry
 * @property {(e: any) => any} branch
 * @property {() => Promise<Array<{ id: string, name: string }>>} people whom it may be shared with
 * @property {string} nobody what Share says when there is no one
 * @property {(e: any, to: string) => Promise<any>} share
 * @property {(e: any) => string} exportPath
 * @property {(e: any) => string} exportName
 * @property {(e: any) => Promise<any>} save
 * @property {(e: any) => Promise<string>|string} script what Read aloud reads
 * @property {(e: any, view: any) => FeedbackOptions} feedback
 * @property {(e: any) => Promise<any>} task
 * @property {(e: any) => Promise<any>} pin
 * @property {string} deleting what Delete asks first
 * @property {(e: any) => Promise<any>} remove
 * @property {(view: any, act?: string) => Promise<any>} after draws again once an action is done
 */
const MessageControls = (() => {
  /** Actions whose adapter method is named otherwise. */
  const METHODS = {
    "share-send": "shareSend", "share-cancel": "shareCancel", "edit-send": "editSend", "edit-cancel": "editCancel",
    "refine-shorter": "refineShorter", "refine-direct": "refineDirect", "refine-table": "refineTable",
    "refine-schedule": "refineSchedule", "refine-split": "refineSplit",
  };
  const REFINE_ACTIONS = [
    ["refine-shorter", "Shorter"],
    ["refine-direct", "More direct"],
    ["refine-table", "Turn into table"],
    ["refine-schedule", "Schedule"],
    ["refine-split", "Open in split view"],
  ];
  const QUICK_REACTIONS = [["great", "That was great"], ["annoying", "That was annoying"]];
  const escape = s => (typeof esc === "function" ? esc(s) : String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`));
  const iconOf = a => (typeof ICONS !== "undefined" ? ICONS[a] || "" : "");

  /**
   * The controls a message has, in order: those at hand first.
   * @param {ControlledMessage} m @param {ControlCaps} [caps] @returns {Control[]}
   */
  function controls(m, caps = {}) {
    /** @type {Control[]} */
    const out = [];
    const add = (act, label, primary = false) => out.push({ act, label, primary });
    const may = m.may || {};
    const own = m.mine && !m.agent && may.edit;
    if (own) add("edit", "Edit", true);
    else {
      add("copy", m.agent ? "Copy response" : "Copy message", true);
      if (m.readable && caps.canReadAloud) add(caps.reading ? "hush" : "listen", caps.reading ? "Stop reading" : "Read aloud", true);
    }
    if (own) add("copy", "Copy message");
    if (may.retry) add("retry", "Retry");
    add("branch", "Branch");
    if (may.share) add("share", "Share");
    add("export", "Export");
    if (may.save) add("save", "Save to Files");
    if (may.task) add("task", "Make a task");
    if (may.pin) add("pin", m.pinned ? "Unpin" : "Pin");
    if (may.feedback) add("feedback", "Feedback");
    if (may.delete) add("delete", "Delete message");
    return out;
  }

  /**
   * The controls as a row of icons under the message, the rest in a menu.
   * @param {string} menuId @param {Control[]} list @param {{ voiceEntry?: string, lead?: string, agent?: boolean }} [o] lead: the surface's own controls, first in the row
   */
  function html(menuId, list, o = {}) {
    const button = c => `<button class="icon" data-msg-act="${c.act}"${c.act === "listen" || c.act === "hush" ? ` data-voice-entry="${escape(o.voiceEntry)}"` : ""} aria-label="${c.label}" data-tooltip="${c.label}"><svg viewBox="0 0 24 24" aria-hidden="true">${iconOf(c.act)}</svg></button>`;
    const menu = list.filter(c => !c.primary).map(c => `<button role="menuitem" tabindex="-1" data-msg-act="${c.act}">${c.label}</button>`).join("");
    const refine = o.agent ? `<div class="msg-refine-bar" role="group" aria-label="Quick refinements">${REFINE_ACTIONS.map(([act, label]) => `<button type="button" class="msg-refine-btn" data-msg-act="${act}">${label}</button>`).join("")}</div>` : "";
    return `<div class="msg-actions small" role="group" aria-label="Message actions">${o.lead || ""}${list.filter(c => c.primary).map(button).join("")}<div class="message-menu-wrap"><button class="icon menu-trigger" data-menu="${escape(menuId)}" aria-label="More message actions" data-tooltip="More message actions" aria-haspopup="menu" aria-expanded="false" aria-controls="${escape(menuId)}"><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/></svg></button>
    <div id="${escape(menuId)}" class="action-menu" role="menu" aria-label="More message actions" hidden>${menu}</div></div>${refine}</div>`;
  }

  /**
   * Whom to share with, under the message.
   * @param {string} domId @param {Array<{ id: string, name: string }>} people
   */
  function shareRow(domId, people) {
    return `<div class="row small share-to"><label class="sr" for="${escape(domId)}">Share with</label>
    <select id="${escape(domId)}" data-share-to>${(people || []).map(p => `<option value="${escape(p.id)}">${escape(p.name)}</option>`).join("")}</select>
    <button class="primary" data-msg-act="share-send">Share</button><button class="link" data-msg-act="share-cancel">Cancel</button></div>`;
  }

  /** The adapter method an action runs. @param {string} act */
  const method = act => METHODS[act] || act;

  /**
   * Run a chosen control on the surface's adapter: each action is the
   * adapter's method of its name (`share-send` is `shareSend`). Feedback
   * opens the one dialog, from what the adapter's `feedback` gives.
   * @param {string} act @param {Object<string, Function>} adapter
   */
  async function run(act, adapter) {
    if (act === "feedback") return typeof adapter.feedback === "function" ? feedback(await adapter.feedback()) : undefined;
    const f = adapter[method(act)];
    return typeof f === "function" ? f.call(adapter) : undefined;
  }

  /**
   * The Feedback dialog: the task's verdict and, where the surface has
   * them, quick reactions. Closing it gives focus back where it came from.
   * @param {FeedbackOptions} o
   */
  function feedback(o) {
    const d = document.createElement("dialog"); d.className = "feedback-dialog"; d.setAttribute("aria-label", "Feedback");
    const verdict = o.task && typeof verdictBar === "function" ? verdictBar(o.task, o.verdict ?? null) : "";
    const quick = o.react ? `<div class="row">${QUICK_REACTIONS.map(([a, label]) => `<button data-feedback-react="${a}" aria-pressed="${o.reaction === a}">${label}</button>`).join("")}</div>` : "";
    d.innerHTML = `<h2>Feedback</h2>${verdict}${quick}<button data-close-feedback>Close feedback</button>`;
    d.onclick = async event => {
      if (event.target.closest("[data-close-feedback]")) { d.close(); return; }
      const pick = event.target.closest("[data-feedback-react]");
      if (!pick || !o.react) return;
      const status = document.createElement("p"); status.className = "small"; status.setAttribute("role", "status"); status.textContent = "Saving feedback…"; d.append(status);
      pick.disabled = true;
      try { await o.react(pick.dataset.feedbackReact); d.close(); }
      catch (err) { status.textContent = "Feedback was not confirmed. Try again when connected."; if (typeof toast === "function") toast(err.message); pick.disabled = false; }
    };
    d.onclose = () => { d.remove(); o.focus?.()?.focus?.({ preventScroll: true }); };
    document.body.append(d); d.showModal();
    return d;
  }

  /**
   * The one adapter both conversations' controls run on: what the view keeps
   * (the edit box, whom to share with), the words it says, and a transport
   * for where each action goes.
   * @param {any} e the entry @param {any} view the chat view @param {HTMLElement} b the control @param {HTMLElement} el the entry's element @param {ChatTransport} t
   * @returns {Object<string, Function>}
   */
  function adapter(e, view, b, el, t) {
    const id = e.id, ui = view.ui, find = q => $(q, view.section);
    return {
      copy: async () => { await copyText(t.text(e)); toast("Copied."); },
      listen: () => readAloud(t.script(e), view.conv.voice ? view.conv.voice(e) : id),
      hush: () => hush(),
      edit: () => { drafts[`edit-${id}`] = e.text || ""; ui.editing = id; return view.draw(); },
      editCancel: () => { ui.editing = null; return view.draw(); },
      editSend: async () => {
        const typed = (find(`#edit-${CSS.escape(id)}`)?.value || "").trim();
        if (!typed) return toast("Write the message first.");
        b.disabled = true;
        const sent = await t.resend(e, typed);
        if (sent === false) { b.disabled = false; return; }
        ui.editing = null; delete drafts[`edit-${id}`]; return t.after(view, "edit");
      },
      retry: async () => { b.disabled = true; await t.retry(e); return t.after(view, "retry"); },
      branch: () => t.branch(e),
      share: async () => { ui.people = await t.people(); if (!ui.people.length) return toast(t.nobody); ui.sharing = id; return view.draw(); },
      shareCancel: () => { ui.sharing = null; return view.draw(); },
      shareSend: async () => {
        const to = $("[data-share-to]", el)?.value;
        if (!to) return;
        b.disabled = true; await t.share(e, to); ui.sharing = null;
        toast(`Shared with ${ui.people.find(p => p.id === to)?.name || to}.`); return view.draw();
      },
      export: async () => toast(`Saved ${await Magnus.download(t.exportPath(e), t.exportName(e))}`),
      save: async () => { await t.save(e); toast("Saved to Files."); },
      task: () => t.task(e),
      pin: async () => { await t.pin(e); return t.after(view, "pin"); },
      feedback: () => ({ ...t.feedback(e, view), focus: () => find(`[data-entry="${CSS.escape(id)}"] .menu-trigger`) }),
      delete: async () => {
        if (!await ask(t.deleting, "Delete")) return;
        await t.remove(e); return t.after(view, "delete");
      },
      refineShorter: () => queueRefine(view, "Make your previous response shorter and tighter."),
      refineDirect: () => queueRefine(view, "Rewrite your previous response to be more direct and action-oriented."),
      refineTable: () => queueRefine(view, "Turn your previous response into a clean Markdown table."),
      refineSchedule: () => queueRefine(view, "/schedule Turn your previous response into a recurring schedule and confirm the cadence."),
      refineSplit: () => {
        const content = String(t.text(e) || "").trim();
        const title = content.split("\n")[0].replace(/^#+\s*/, "").slice(0, 60) || "Draft";
        if (typeof globalThis.SplitCoedit !== "undefined") globalThis.SplitCoedit.open({ title, content, onSend: msg => queueRefine(view, msg) });
        else if (typeof toast === "function") toast("Split Co-Edit is not ready yet.");
      },
    };
  }

  /** Fill the active conversation composer with a one-click refinement prompt. @param {any} view @param {string} prompt */
  function queueRefine(view, prompt) {
    const box = /** @type {?HTMLTextAreaElement} */ (view?.section?.querySelector("form.say textarea") || document.querySelector("#msg"));
    if (!box) { if (typeof toast === "function") toast(prompt); return; }
    box.value = prompt;
    box.dispatchEvent(new Event("input", { bubbles: true }));
    box.focus();
  }

  return { controls, html, shareRow, run, feedback, method, adapter };
})();
if (typeof module !== "undefined") module.exports = MessageControls;
