// Which of Aurora's boxes show, and for how long
// (decisions/2026-11-21-aurora-boxes-come-and-go.md):
// - A box that wants the person (a decision, a teammate's message, a
//   decision just answered) stays until it is dealt with or closed.
// - Any other box (work running or just finished, Up Next, new files, the
//   computer, apps, Recent Work) slides in when it appears or changes, and
//   leaves after SHOWN_FOR_MS. Work that runs for hours does not keep a box.
//   It stays while the pointer or the focus is on the boxes, and for
//   SHOWN_FOR_MS after they leave.
// - Close (×) puts a box away until it changes: until it is another task,
//   another decision, or the same one in another state (running, needs you,
//   done). A new status line is not a change. Closed boxes are remembered in
//   this browser for the person signed in, and for no one else: what a box
//   says names their work (scope).
"use strict";

const AuroraBoxes = (() => {
  const SHOWN_FOR_MS = 90_000;
  /** How many closed boxes this browser remembers, newest kept. */
  const CLOSED_KEPT = 200;
  const STORE = "magnus-boxes-closed";
  /** When each box, as it is now, first showed. @type {Map<string, number>} */
  const since = new Map();
  /** Where the signed-in person's closed boxes are kept; null: for this page only. @type {?string} */
  let key = null;
  /** Last seen content signature per box key, so live updates record recency. @type {Map<string, string>} */
  const lastContent = new Map();
  /** When each box key last appeared or changed content. @type {Map<string, number>} */
  const touched = new Map();
  /** @type {string[]} */
  let closed = [];

  function load() {
    if (!key) return [];
    try {
      const v = JSON.parse(localStorage.getItem(key) || "[]");
      return Array.isArray(v) ? v.filter(s => typeof s === "string") : [];
    } catch { return []; }
  }
  function save() {
    if (!key) return;
    try { localStorage.setItem(key, JSON.stringify(closed)); } catch { /* Kept for this page only. */ }
  }

  /**
   * Whose closed boxes these are: the person signed in at `appliance`, or
   * no one (null, signed out). The list an older app kept for everyone on
   * this browser is dropped, not used: nothing says whose it was.
   * @param {?string} appliance @param {?string} person
   */
  function scope(appliance, person) {
    try { localStorage.removeItem(STORE); } catch { /* nothing kept to drop */ }
    key = person ? `${STORE}.${appliance}.${person}` : null;
    closed = load();
    since.clear();
    lastContent.clear();
    touched.clear();
  }

  /**
   * Identity + state signature used for close persistence. A decision's box
   * is known by its decision alone: how many others wait is not part of it,
   * or a closed one would come back when that changed.
   * @param {{ key: string, tone: string, title: string, count?: number }} c
   */
  const sign = c => `${c.key}|${c.tone}|${c.title}${c.count && !c.key.startsWith("approval:") ? `|${c.count}` : ""}`;
  /** Full content signature including live description, used for recency tracking and resurfacing. @param {{ key: string, tone: string, title: string, desc?: string, count?: number }} c */
  const contentSign = c => `${sign(c)}|${c.desc || ""}`;
  /** Whether a box wants attention or is actively running, and so stays visible. @param {{ tone: string, said?: string }} c */
  const stays = c => c.tone === "needs" || c.tone === "talk" || c.tone === "run" || !!c.said;

  /**
   * Records content changes for recency ordering and returns the last touched timestamp for a key.
   * @param {Array<{ key: string, tone: string, title: string, desc?: string, count?: number }>} list
   * @param {number} [now]
   */
  function noteChanges(list, now = Date.now()) {
    const keys = new Set(list.map(c => c.key));
    for (const k of [...lastContent.keys()]) if (!keys.has(k)) { lastContent.delete(k); touched.delete(k); }
    for (const c of list) {
      const cs = contentSign(c);
      if (lastContent.get(c.key) !== cs) {
        lastContent.set(c.key, cs);
        touched.set(c.key, now);
        // A passive box whose content actually changed gets a fresh display window.
        if (!stays(c)) since.set(sign(c), now);
      }
    }
  }

  /** Timestamp when a box key last appeared or updated, or 0. @param {string} key */
  const touchedAt = key => touched.get(key) || 0;

  /**
   * The boxes to show now, of those relevant, in their order.
   * @template {{ key: string, tone: string, title: string, desc?: string, count?: number, said?: string }} T
   * @param {T[]} list @param {number} now @param {Set<string>} held boxes the pointer or focus is on
   * @returns {T[]}
   */
  function shown(list, now = Date.now(), held = new Set()) {
    noteChanges(list, now);
    const here = new Set(list.map(sign));
    // A box that went and comes back is new again.
    for (const s of [...since.keys()]) if (!here.has(s)) since.delete(s);
    const out = [];
    for (const c of list) {
      const s = sign(c);
      if (closed.includes(s)) continue;
      if (!since.has(s) || held.has(s)) since.set(s, now);
      if (stays(c) || now - (since.get(s) ?? now) < SHOWN_FOR_MS) out.push(c);
    }
    return out;
  }

  /** How long until one of these boxes leaves on its own, or null. @param {Array<{ key: string, tone: string, title: string, count?: number, said?: string }>} list @param {number} now */
  function next(list, now = Date.now()) {
    const left = list.filter(c => !stays(c)).map(c => SHOWN_FOR_MS - (now - (since.get(sign(c)) ?? now)));
    return left.length ? Math.max(0, Math.min(...left)) : null;
  }

  /** The boxes the person just left: their time starts again. @param {string[]} signs @param {number} now */
  function left(signs, now = Date.now()) {
    for (const s of signs) if (since.has(s)) since.set(s, now);
  }

  /** Put a box away until it changes. @param {string} s its sign */
  function close(s) {
    if (!s || closed.includes(s)) return;
    closed.push(s);
    closed = closed.slice(-CLOSED_KEPT);
    save();
  }

  /** Whether a box, as it is, was closed. @param {{ key: string, tone: string, title: string, count?: number }} c */
  const isClosed = c => closed.includes(sign(c));

  return { SHOWN_FOR_MS, sign, contentSign, stays, noteChanges, touchedAt, shown, next, left, close, isClosed, scope };
})();
