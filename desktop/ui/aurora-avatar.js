// Magnus as an avatar, not an image (decisions/2026-11-21-aurora-avatar.md).
//
// An avatar is anything that meets one small contract:
//   AuroraAvatar.define(name, (host, { size }) => ({ set(state), destroy() }))
// It is mounted into a host element it draws in, told each state Magnus is
// in, and taken down. Each style names its avatar (Theme.avatar in theme.js),
// so a theme can put another avatar in place of the orb; this script mounts
// whichever the style names, where the app shows Magnus: at the top of the
// navigation, and in the middle of a new conversation.
//
// The states follow the line above the composer and the answer's reasoning
// pill (aurora-reasoning.js): ready, listening, thinking, planning, working,
// waiting (on the person), in control (the person's), done and failed. Done
// and failed show for a moment after a run ends, then the avatar is ready.
//
// The avatar is a button: its name says the state in words, so the state is
// never only a colour or a shape, and clicking it shows what Magnus is doing
// (the decision waiting on the person if one is, else Activity). The orb leans
// toward the pointer. With reduced motion or none, it is drawn once for each
// state, still. It does not draw while the window is hidden.
"use strict";

/** How often the state is read from the page. */
const AVATAR_POLL_MS = 400;
/** How long done shows after a run ends well. */
const AVATAR_DONE_MS = 4000;
/** How long failed shows after a run ends badly. */
const AVATAR_FAILED_MS = 8000;

const AuroraAvatar = (() => {
  const on = () => typeof Theme !== "undefined" && typeof Theme.stage === "function" && Theme.stage(document.documentElement.dataset.style);
  const moving = () => document.documentElement.dataset.motion === "full";

  /** What each state is called, in the avatar's name and title. */
  const WORDS = {
    ready: "ready",
    listening: "listening",
    thinking: "thinking",
    planning: "planning",
    working: "working",
    waiting: "waiting on you",
    control: "letting you take control",
    done: "done",
    failed: "stopped with a problem",
  };

  /**
   * An avatar: draws in its host, follows `set`, and is gone after `destroy`.
   * @typedef {{ set: (state: string) => void, destroy: () => void, look?: (x: number, y: number) => void }} Avatar
   * @typedef {(host: HTMLElement, o: { size: number }) => Avatar} Maker
   * @type {Map<string, Maker>}
   */
  const makers = new Map();

  /** @type {Array<{ slot: HTMLElement, button: HTMLButtonElement, size: number, name: string, avatar: Avatar, fixedState?: string }>} */
  const mounted = [];

  let state = "ready";
  /** A run's end, shown for a moment: its state and until when. */
  let ending = { state: "", until: 0 };
  /** The phase each answer's pill was last seen in. @type {WeakMap<Element, string>} */
  const seen = new WeakMap();
  let timer = 0;
  /** The new conversation last opened at its top, by its address. */
  let opened = "";

  /** @param {string} name @param {Maker} make */
  function define(name, make) {
    makers.set(name, make);
    // A style already showing this avatar's name gets it drawn now.
    for (const m of mounted) if (m.name === name) remount(m);
  }

  /** The state Magnus is in, from AuroraCore when present else the page. */
  function read() {
    if (typeof AuroraCore !== "undefined") return AuroraCore.state;
    const html = document.documentElement.dataset.auroraState || "idle";
    if (html === "listening") return "listening";
    if (html === "takeover") return "control";
    if (html === "needs") return "waiting";
    const pills = [...document.querySelectorAll(".aurora-reason")].filter(p => /** @type {HTMLElement} */ (p).isConnected);
    const last = /** @type {HTMLElement|undefined} */ (pills[pills.length - 1]);
    const phase = last?.dataset.phase || "";
    const now = Date.now();
    // A run seen ending shows its end for a moment; one already ended when
    // first seen (an earlier answer, another conversation) does not.
    if (last) {
      const before = seen.get(last);
      seen.set(last, phase);
      const bad = phase === "stopped" && last.dataset.outcome === "failed";
      if (before && before !== phase && (phase === "done" || bad)) ending = { state: bad ? "failed" : "done", until: now + (bad ? AVATAR_FAILED_MS : AVATAR_DONE_MS) };
    }
    // A run under way is what shows; the last one's end is over.
    if (["executing", "thinking"].includes(html) || ["working", "thinking", "writing"].includes(phase)) ending = { state: "", until: 0 };
    if (html === "executing" || phase === "working") return last?.hasAttribute("data-planned") ? "planning" : "working";
    if (html === "thinking" || phase === "thinking" || phase === "writing") return "thinking";
    if (ending.until > now) return ending.state;
    return "ready";
  }

  /** @param {{ button: HTMLButtonElement, fixedState?: string }} m */
  function name(m) {
    const s = m.fixedState || state;
    const words = WORDS[/** @type {keyof typeof WORDS} */ (s)] || WORDS.ready;
    m.button.setAttribute("aria-label", `Magnus is ${words}. Show what Magnus is doing`);
    m.button.title = `Magnus is ${words}`;
    m.button.dataset.state = s;
  }

  function tell() {
    const next = read();
    if (next !== state) {
      state = next;
      for (const m of mounted) { name(m); m.avatar.set(m.fixedState || state); }
    }
  }

  /** @param {typeof mounted[number]} m */
  function remount(m) {
    m.avatar.destroy();
    m.button.replaceChildren();
    const make = makers.get(m.name);
    m.avatar = make ? make(m.button, { size: m.size }) : { set() {}, destroy() {} };
    m.avatar.set(m.fixedState || state);
  }

  /** Explicit class names for each mounted placement (keeps static CSS checks accurate). */
  const PLACE_CLASS = {
    nav: "aurora-avatar-nav",
    bar: "aurora-avatar-bar",
    side: "aurora-avatar-side",
    hero: "aurora-avatar-hero",
  };

  /**
   * Shows Magnus in `slot`, as the style's avatar, `size` pixels across.
   * @param {HTMLElement} slot @param {number} size @param {string} place @param {string} [fixedState]
   */
  function mount(slot, size, place, fixedState) {
    const avatarName = Theme.avatar(document.documentElement.dataset.style || "", document.documentElement.dataset.avatar || "") || "";
    const button = document.createElement("button");
    button.type = "button";
    const placeClass = PLACE_CLASS[/** @type {keyof typeof PLACE_CLASS} */ (place)] || `aurora-avatar-${place}`;
    button.className = `aurora-avatar ${placeClass}`;
    button.style.setProperty("--avatar-size", `${size}px`);
    slot.append(button);
    const make = makers.get(avatarName);
    const m = { slot, button, size, name: avatarName, avatar: make ? make(button, { size }) : { set() {}, destroy() {} }, fixedState };
    name(m);
    m.avatar.set(fixedState || state);
    mounted.push(m);
    return m;
  }

  function unmountAll() {
    for (const m of mounted.splice(0)) { m.avatar.destroy(); m.button.remove(); }
  }

  /** Each place Magnus is shown gets an avatar; places gone take theirs down. */
  function place() {
    for (let i = mounted.length - 1; i >= 0; i--) {
      const m = mounted[i];
      if (!m.button.isConnected) { m.avatar.destroy(); mounted.splice(i, 1); }
    }
    if (!on()) { unmountAll(); return; }
    const want = Theme.avatar(document.documentElement.dataset.style || "", document.documentElement.dataset.avatar || "") || "";
    for (const m of mounted) if (m.name !== want) { m.name = want; remount(m); }
    document.querySelectorAll("nav.side .brand .logo").forEach(slot => {
      if (slot.querySelector(":scope > .aurora-avatar")) return;
      // The mark's slot is hidden from assistive technology; the avatar in
      // it is a control, so the slot is not, and the mark it covers is.
      slot.removeAttribute("aria-hidden");
      slot.querySelector(":scope > svg")?.setAttribute("aria-hidden", "true");
      mount(/** @type {HTMLElement} */ (slot), 28, "nav");
    });
    document.querySelectorAll(".workspace-bar").forEach(bar => {
      const heading = bar.querySelector(".workspace-heading");
      if (!heading || bar.querySelector(":scope > .aurora-avatar-bar")) return;
      const m = mount(/** @type {HTMLElement} */ (bar), 22, "bar");
      bar.insertBefore(m.button, heading);
    });
    document.querySelectorAll(".companion-avatar").forEach(slot => {
      if (slot.querySelector(":scope > .aurora-avatar")) return;
      mount(/** @type {HTMLElement} */ (slot), 30, "side");
    });
    const heads = document.querySelectorAll(".hello-head");
    heads.forEach(slot => {
      if (!slot.querySelector(":scope > .aurora-avatar")) {
        const m = mount(/** @type {HTMLElement} */ (slot), 84, "hero");
        slot.prepend(m.button);
      }
      // A new conversation opens at its top, where Magnus and the question
      // are, once each time it is opened; the input stays at the bottom.
      if (opened !== location.hash) {
        opened = location.hash;
        const scroller = slot.closest(".thread");
        if (scroller) scroller.scrollTop = 0;
      }
    });
    if (!heads.length) opened = "";
  }

  let placeQueued = 0;
  function queuePlace() {
    if (placeQueued) return;
    placeQueued = requestAnimationFrame(() => { placeQueued = 0; if (on()) { place(); tell(); } });
  }

  function run() {
    if (typeof AuroraCore !== "undefined") {
      place();
      tell();
      return;
    }
    if (timer || document.hidden) return;
    timer = window.setInterval(() => { if (on()) { place(); tell(); } }, AVATAR_POLL_MS);
  }
  function stop() { if (timer) { clearInterval(timer); timer = 0; } }

  if (typeof AuroraCore !== "undefined") {
    AuroraCore.onState(() => tell());
    AuroraCore.onDom(records => { if (records.some(r => r.addedNodes.length || r.removedNodes.length)) queuePlace(); });
  }

  document.addEventListener("visibilitychange", () => { if (document.hidden) stop(); else { run(); tell(); } });
  window.addEventListener("magnus-theme", () => { place(); tell(); for (const m of mounted) m.avatar.set(m.fixedState || state); });

  // Clicking the avatar shows what Magnus is doing: what waits on the
  // person, if anything does, else Activity.
  document.addEventListener("click", e => {
    const b = /** @type {HTMLElement} */ (e.target).closest?.(".aurora-avatar");
    if (!b) return;
    e.preventDefault();
    e.stopPropagation();
    // The decision waiting longest opens on the canvas; with none, Activity
    // (which Aurora shows as a page, not on the canvas).
    const first = typeof needsMe === "function" ? needsMe()[0] : null;
    if (first?.id) openPanel(`item:${first.id}`, { history: true, focus: true, opener: b });
    else location.hash = "#/activity";
  }, true);

  // The avatar leans toward the pointer near it, coalesced to one layout read per frame.
  let lookRaf = 0, lastPointerX = 0, lastPointerY = 0;
  document.addEventListener("pointermove", e => {
    if (!moving() || !mounted.length) return;
    lastPointerX = e.clientX;
    lastPointerY = e.clientY;
    if (lookRaf) return;
    lookRaf = requestAnimationFrame(() => {
      lookRaf = 0;
      if (!moving()) return;
      for (const m of mounted) {
        if (!m.avatar.look) continue;
        const r = m.button.getBoundingClientRect();
        if (!r.width) continue;
        const dx = (lastPointerX - (r.left + r.width / 2)) / Math.max(r.width * 4, 120);
        const dy = (lastPointerY - (r.top + r.height / 2)) / Math.max(r.height * 4, 120);
        m.avatar.look(Math.max(-1, Math.min(1, dx)), Math.max(-1, Math.min(1, dy)));
      }
    });
  }, { passive: true });

  const first = new MutationObserver(() => { if (document.body) { first.disconnect(); run(); } });
  if (document.body) run(); else first.observe(document.documentElement, { childList: true });

  return { define, mount, get state() { return state; }, get mounted() { return mounted.map(m => ({ place: m.button.className, name: m.name })); }, now: () => { place(); tell(); } };
})();
