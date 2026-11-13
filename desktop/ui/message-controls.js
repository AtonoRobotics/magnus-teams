// One set of message controls for both chats, the personal one and a team
// channel (owner, 2026-11-13: "the same message controls as the other chat
// window"). Which controls a message has is decided here once, from what it
// is and who is looking; the icons and the menu are drawn here once; and a
// chosen control is run here against the surface's own adapter, which does
// the work its own way (the personal chat's routes, or the team's). The
// Feedback dialog lives here too, so both open and close it alike.

/**
 * What a message is, to its controls.
 * @typedef {{ mine?: boolean, agent?: boolean, readable?: boolean, retry?: boolean, feedback?: boolean, private?: boolean, pinned?: boolean, deletes?: boolean }} ControlledMessage
 * mine: the viewer wrote it. agent: Magnus did. readable: it can be read
 * aloud. retry, feedback: offered to whoever asked for it. private: only
 * its requester may see it, so it is not shared or saved for others.
 * deletes: the viewer may delete it.
 * What the viewer may do here.
 * @typedef {{ guest?: boolean, pins?: boolean, canReadAloud?: boolean, reading?: boolean }} ControlCaps
 * One control: its action, its name, and whether it sits at hand or in the menu.
 * @typedef {{ act: string, label: string, primary: boolean }} Control
 * What the Feedback dialog needs: the task to judge, the verdict so far,
 * and, where a surface has them, quick reactions.
 * @typedef {{ task?: ?string, verdict?: any, reaction?: ?string, react?: (a: string) => Promise<any>, focus?: () => any }} FeedbackOptions
 */
const MessageControls = (() => {
  /** Actions whose adapter method is named otherwise. */
  const METHODS = { "share-send": "shareSend", "share-cancel": "shareCancel", "edit-send": "editSend", "edit-cancel": "editCancel" };
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
    const own = m.mine && !m.agent;
    if (own) add("edit", "Edit", true);
    else {
      add("copy", m.agent ? "Copy response" : "Copy message", true);
      if (m.readable && caps.canReadAloud) add(caps.reading ? "hush" : "listen", caps.reading ? "Stop reading" : "Read aloud", true);
    }
    if (own) add("copy", "Copy message");
    if (m.retry) add("retry", "Retry");
    add("branch", "Branch");
    if (!m.private) add("share", "Share");
    add("export", "Export");
    if (!caps.guest && !m.private) add("save", "Save to Files");
    if (!caps.guest) add("task", "Make a task");
    if (caps.pins && !caps.guest) add("pin", m.pinned ? "Unpin" : "Pin");
    if (m.feedback) add("feedback", "Feedback");
    if (m.deletes) add("delete", "Delete message");
    return out;
  }

  /**
   * The controls as a row of icons under the message, the rest in a menu.
   * @param {string} menuId @param {Control[]} list @param {{ voiceEntry?: string, lead?: string }} [o] lead: the surface's own controls, first in the row
   */
  function html(menuId, list, o = {}) {
    const button = c => `<button class="icon" data-msg-act="${c.act}"${c.act === "listen" || c.act === "hush" ? ` data-voice-entry="${escape(o.voiceEntry)}"` : ""} aria-label="${c.label}" data-tooltip="${c.label}"><svg viewBox="0 0 24 24" aria-hidden="true">${iconOf(c.act)}</svg></button>`;
    const menu = list.filter(c => !c.primary).map(c => `<button role="menuitem" tabindex="-1" data-msg-act="${c.act}">${c.label}</button>`).join("");
    return `<div class="msg-actions small" role="group" aria-label="Message actions">${o.lead || ""}${list.filter(c => c.primary).map(button).join("")}<div class="message-menu-wrap"><button class="icon menu-trigger" data-menu="${escape(menuId)}" aria-label="More message actions" data-tooltip="More message actions" aria-haspopup="menu" aria-expanded="false" aria-controls="${escape(menuId)}"><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/></svg></button>
    <div id="${escape(menuId)}" class="action-menu" role="menu" aria-label="More message actions" hidden>${menu}</div></div></div>`;
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

  return { controls, html, shareRow, run, feedback, method };
})();
if (typeof module !== "undefined") module.exports = MessageControls;
