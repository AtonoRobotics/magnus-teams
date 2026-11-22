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
//   this browser.
"use strict";

const AuroraBoxes = (() => {
  const SHOWN_FOR_MS = 15_000;
  /** How many closed boxes this browser remembers, newest kept. */
  const CLOSED_KEPT = 200;
  const STORE = "magnus-boxes-closed";
  /** When each box, as it is now, first showed. @type {Map<string, number>} */
  const since = new Map();
  /** @type {string[]} */
  let closed = load();

  function load() {
    try {
      const v = JSON.parse(localStorage.getItem(STORE) || "[]");
      return Array.isArray(v) ? v.filter(s => typeof s === "string") : [];
    } catch { return []; }
  }
  function save() {
    try { localStorage.setItem(STORE, JSON.stringify(closed)); } catch { /* Kept for this page only. */ }
  }

  /** What a box is, so a change to it shows it again: not what it says. @param {{ key: string, tone: string, title: string }} c */
  const sign = c => `${c.key}|${c.tone}|${c.title}`;
  /** Whether a box wants the person, and so stays. @param {{ tone: string, said?: string }} c */
  const stays = c => c.tone === "needs" || c.tone === "talk" || !!c.said;

  /**
   * The boxes to show now, of those relevant, in their order.
   * @template {{ key: string, tone: string, title: string, said?: string }} T
   * @param {T[]} list @param {number} now @param {Set<string>} held boxes the pointer or focus is on
   * @returns {T[]}
   */
  function shown(list, now = Date.now(), held = new Set()) {
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

  /** How long until one of these boxes leaves on its own, or null. @param {Array<{ key: string, tone: string, title: string, said?: string }>} list @param {number} now */
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

  /** Whether a box, as it is, was closed. @param {{ key: string, tone: string, title: string }} c */
  const isClosed = c => closed.includes(sign(c));

  return { SHOWN_FOR_MS, sign, stays, shown, next, left, close, isClosed };
})();
