// Pop-ups in the lower corner for what needs the person, wherever they are
// in the app: a message to them, a mention, a message in a channel they
// follow in full, the team agent's answer to them, and anything new in
// Needs you. Clicking one opens it. When the app is not in front, the same
// goes to the computer's own notifications, if the person keeps them on for
// this device. Those carry who and where, never what a message says: they
// show on a locked screen and stay in the system's list after the app is
// closed.
"use strict";

/** How long a pop-up stays, how many show at once, how far back each look
 * overlaps the last so a message stamped just before it is not missed, and
 * how many shown ones are remembered so none shows twice. */
const NOTICE_SHOW_MS = 8000, NOTICE_MAX_SHOWN = 3, NOTICE_OVERLAP_MS = 10000, NOTICE_SEEN_MAX = 500;
/** Where this device keeps whether the system's notifications are on. */
const NOTICE_DEVICE_KEY = "magnus.system-notifications", NOTICE_OFFERED_KEY = "magnus.system-notifications-offered";

/**
 * @typedef {{ id: string, kind: "direct"|"mention"|"message"|"answer", workspace: string, workspace_name: string, channel: string, channel_name: string, direct: ?string[], thread: ?string, sender: string, at: string, preview: string }} TeamNotice
 * @typedef {{ key: string, title: string, body: string, system: string, href: string }} Shown
 */

const Notices = {
  /** The server's time of the last look; none until the first. */
  after: /** @type {?string} */ (null),
  seen: new Set(),

  reset() { this.after = null; this.seen = new Set(); $("#notices")?.remove(); },

  /** One look for new team messages to tell about. */
  async poll() {
    const q = this.after ? `?after=${encodeURIComponent(new Date(Date.parse(this.after) - NOTICE_OVERLAP_MS).toISOString())}` : "";
    const r = await api("GET", `/v1/team/notices${q}`).catch(() => null);
    if (!r) return;
    const first = this.after === null;
    this.after = r.now;
    if (first) return;
    for (const n of r.notices || []) this.show(noticeWords(n, r.names || {}));
  },

  /** Tell the person, unless they are already looking at it. @param {Shown} s */
  show(s) {
    if (this.seen.has(s.key)) return;
    this.seen.add(s.key);
    if (this.seen.size > NOTICE_SEEN_MAX) this.seen.delete(this.seen.values().next().value);
    const away = document.hidden || !document.hasFocus();
    if (!away && s.href && location.hash.startsWith(s.href.split("/t/")[0])) return;
    this.popup(s);
    if (away && this.systemOn()) Magnus.notify?.(s.title, s.system, () => { if (s.href) location.hash = s.href; });
  },

  /** A pop-up in the lower corner: the newest at the bottom, a few at most. @param {Shown} s */
  popup(s) {
    let box = $("#notices");
    if (!box) {
      box = document.createElement("section");
      box.id = "notices"; box.setAttribute("aria-label", "Notifications");
      document.body.append(box);
      box.addEventListener("click", e => this.click(e));
    }
    while (box.children.length >= NOTICE_MAX_SHOWN) box.firstElementChild?.remove();
    const el = document.createElement("div");
    el.className = "notice"; el.setAttribute("role", "status"); el.dataset.notice = s.key;
    const body = typeof Compose === "object" ? Compose.maskedText(s.body) : esc(s.body);
    el.innerHTML = `<button type="button" class="notice-open" data-open="${esc(s.href)}"><strong>${esc(s.title)}</strong>${s.body ? `<span>${body}</span>` : ""}</button>
      <button type="button" class="notice-close icon-btn" data-dismiss aria-label="Dismiss">${typeof icon === "function" ? icon("close") : "×"}</button>${this.offer() ? `<button type="button" class="link small" data-system-on>Show these when Magnus is in the background</button>` : ""}`;
    box.append(el);
    // It stays while the pointer or focus is on it.
    let timer = setTimeout(() => el.remove(), NOTICE_SHOW_MS);
    const hold = () => clearTimeout(timer), go = () => { clearTimeout(timer); timer = setTimeout(() => el.remove(), NOTICE_SHOW_MS); };
    el.addEventListener("pointerenter", hold); el.addEventListener("focusin", hold);
    el.addEventListener("pointerleave", go); el.addEventListener("focusout", go);
  },

  async click(e) {
    const t = /** @type {HTMLElement} */ (e.target), el = t.closest(".notice");
    if (!el) return;
    if (t.closest("[data-system-on]")) { await this.turnOn(); el.remove(); return; }
    const open = t.closest("[data-open]");
    el.remove();
    const href = open?.getAttribute("data-open");
    if (href) location.hash = href;
  },

  /** Whether this device shows the system's notifications. The desktop app
   * does until turned off; a browser only once the person allowed it. */
  systemOn() {
    let v = null;
    try { v = localStorage.getItem(NOTICE_DEVICE_KEY); } catch { /* private window */ }
    return v !== "off" && Magnus.notifyPermission?.() === "granted";
  },

  /** Whether a pop-up offers to turn them on: a browser that has not been asked. */
  offer() {
    let offered = null;
    try { offered = localStorage.getItem(NOTICE_OFFERED_KEY); } catch { /* private window */ }
    return Magnus.kind === "web" && Magnus.notifyPermission?.() === "default" && !offered;
  },

  /** Turn them on for this device, asking the browser from the person's own click. */
  async turnOn() {
    try { localStorage.setItem(NOTICE_OFFERED_KEY, "1"); } catch { /* private window */ }
    const got = await Magnus.askToNotify?.();
    this.setSystem(got === "granted");
    if (got === "denied") toast("Your browser blocks notifications from this site. Allow them in its site settings.");
    return got === "granted";
  },

  setSystem(on) { try { localStorage.setItem(NOTICE_DEVICE_KEY, on ? "on" : "off"); } catch { /* private window */ } },
};

/** The channel a notice is in, as the rail names it. @param {TeamNotice} n @param {Object<string, string>} names */
function noticePlace(n, names) {
  if (n.direct) return n.direct.map(id => names[id] || id).join(", ") || "yourself";
  return `#${n.channel_name}`;
}

/**
 * What a notice says: in the pop-up, who and where and the start of the
 * message; to the system, who and where only.
 * @param {TeamNotice} n @param {Object<string, string>} names @returns {Shown}
 */
function noticeWords(n, names) {
  const who = names[n.sender] || n.sender, where = noticePlace(n, names);
  const href = teamHref(n.workspace, { kind: "channel", id: n.channel, thread: n.thread || undefined });
  const words = {
    direct: [who, "sent you a message"],
    mention: [`${who} mentioned you`, `in ${where}, ${n.workspace_name}`],
    message: [`${who} in ${where}`, `New message in ${n.workspace_name}`],
    answer: [`${who} answered you`, `in ${where}, ${n.workspace_name}`],
  }[n.kind] || [who, n.workspace_name];
  return { key: `team:${n.workspace}:${n.id}`, title: words[0], body: n.preview, system: words[1], href };
}
