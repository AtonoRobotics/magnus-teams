// The bento: in a style that has one (Theme.bento), what the companion
// panel would show beside the conversation is a row of tiles under it
// instead: what is running, what waits on the person, what runs next and
// what finished. A tile opens its view in the panel beside the
// conversation, which then slides in (decisions/2026-11-20-aurora-bento.md).
"use strict";

const Bento = (() => {
  const SHOWN_APPS = 3;
  const ENDED_STATES = ["completed", "failed", "cancelled"];
  const html = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  const helper = (name, ...args) => (typeof globalThis[name] === "function" ? globalThis[name](...args) : "");
  const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;
  const words = list => list.length < 2 ? list.join("") : `${list.slice(0, -1).join(", ")} and ${list[list.length - 1]}`;
  let drawn = "";

  /** Whether the shown style puts the companion in a bento. */
  function wanted() {
    return typeof Theme !== "undefined" && typeof Theme.bento === "function" && Theme.bento(document.documentElement.dataset.style);
  }

  /**
   * The tiles for a moment of the workspace.
   * @param {{ active: any[], needs: any[], done: any[], schedules: any[], apps: string[], kinds?: Record<string,string> }} s
   * @returns {{ key: string, label: string, tone: "idle"|"run"|"needs"|"done"|"stopped", title: string, desc: string, href: string, open: string }[]}
   */
  function tiles(s) {
    const out = [];
    const run = s.active[0];
    out.push(run
      ? { key: "live", label: "Live Workspace", tone: "run", title: run.title || "Running task",
          desc: `${s.active.length > 1 ? `${s.active.length} running · ` : ""}${run.status_line || (run.status === "suspended" ? "Waiting" : "In progress")}`,
          href: `#/task/${encodeURIComponent(run.task_id)}`, open: `task:${run.task_id}` }
      : { key: "live", label: "Live Workspace", tone: "idle", title: "Connected Apps",
          desc: s.apps.length ? `Standing by across ${words(s.apps.slice(0, SHOWN_APPS))}${s.apps.length > SHOWN_APPS ? ` and ${s.apps.length - SHOWN_APPS} more` : ""}.` : "No apps connected yet.",
          href: "#/apps", open: "apps" });
    const first = s.needs[0];
    out.push(first
      ? { key: "control", label: "Human Control", tone: "needs", title: `${plural(s.needs.length, "decision")} waiting`,
          desc: first.summary || first.ask?.sentence || s.kinds?.[first.kind] || "A decision is waiting for you.",
          href: "#/inbox", open: s.needs.length === 1 ? `item:${first.id}` : "needs" }
      : { key: "control", label: "Human Control", tone: "idle", title: "Nothing waiting on you",
          desc: "Magnus asks before external actions and payments.", href: "#/inbox", open: "needs" });
    const next = s.schedules[0];
    out.push(next
      ? { key: "next", label: "Up Next", tone: "idle", title: next.title || "Scheduled task",
          desc: [helper("until", next.next), s.schedules.length > 1 ? `${s.schedules.length - 1} more` : ""].filter(Boolean).join(" · ") || "Scheduled",
          href: "#/schedules", open: `schedule:${next.id}` }
      : { key: "next", label: "Up Next", tone: "idle", title: "No schedules yet",
          desc: "Ask Magnus to run something on a schedule.", href: "#/schedules", open: "schedules" });
    const last = s.done[0];
    out.push(last
      ? { key: "recent", label: "Recent Work", tone: last.status === "completed" ? "done" : "stopped", title: last.title || "Task",
          desc: `${last.status === "completed" ? "Finished" : "Stopped"}${helper("when", last.ended_at || last.created_at) ? ` ${helper("when", last.ended_at || last.created_at)}` : ""}`,
          href: `#/task/${encodeURIComponent(last.task_id)}`, open: `task:${last.task_id}` }
      : { key: "recent", label: "Recent Work", tone: "idle", title: "No finished work yet",
          desc: "Results land here when Magnus finishes.", href: "#/activity", open: "activity" });
    return out;
  }

  /** @param {ReturnType<typeof tiles>} list */
  function markup(list) {
    return `<div class="bento-tiles" role="group" aria-label="Workspace">${list.map(t => `<a class="bento-mini${t.tone === "run" ? " aurora-executing" : ""}" href="${html(t.href)}" data-open="${html(t.open)}" data-bento-tile="${t.key}" data-tone="${t.tone}"><span class="bento-label"><span>${html(t.label)}</span><span class="mini-dot" aria-hidden="true"></span></span><strong>${html(t.title)}</strong><span class="bento-desc">${html(t.desc)}</span></a>`).join("")}</div>`;
  }

  // The workspace as the app holds it now.
  function snapshot() {
    const app = typeof App !== "undefined" ? App : {};
    const tasks = app.tasks || [];
    const ended = typeof ENDED !== "undefined" ? ENDED : ENDED_STATES;
    const at = t => t.ended_at || t.created_at || "";
    const connectors = typeof Lib !== "undefined" && Array.isArray(Lib.all) ? Lib.all : app.connectors || [];
    const state = c => (typeof stateOf === "function" ? stateOf(c) : c.state);
    return {
      active: tasks.filter(t => !ended.includes(t.status)),
      needs: typeof needsMe === "function" ? needsMe() : [],
      done: tasks.filter(t => ended.includes(t.status)).sort((a, b) => at(b).localeCompare(at(a))),
      schedules: (app.schedules || []).filter(sc => !sc.ended && !sc.paused).sort((a, b) => String(a.next || "").localeCompare(String(b.next || ""))),
      apps: connectors.filter(c => state(c) === "connected").map(c => c.title || c.name),
      kinds: typeof KIND !== "undefined" ? KIND : {},
    };
  }

  // Only the personal conversation has one; a team brings its own panel.
  const onChat = () => /^(?:#\/?|#\/home|#\/chat(?:\/.*)?)?$/.test(location.hash);

  /** Draws the bento under the conversation (beside it in `main`, which the
   * app repaints as a whole), or takes it away. */
  function draw() {
    const conv = document.getElementById("thread")?.closest(".conv");
    if (!wanted() || !conv || !onChat()) return clear();
    if (typeof Panel !== "undefined" && Panel.autoOpened && typeof closePanel === "function") closePanel(false);
    const s = snapshot();
    const next = markup(tiles(s));
    const el = conv.nextElementSibling?.matches(".bento-tiles") ? conv.nextElementSibling : null;
    conv.classList.toggle("aurora-executing", s.active.length > 0 && !s.needs.length);
    if (el && drawn === next) return;
    const tpl = document.createElement("template");
    tpl.innerHTML = next;
    if (el) el.replaceWith(tpl.content); else conv.after(tpl.content);
    drawn = next;
  }

  function clear() {
    document.querySelectorAll(".conv + .bento-tiles").forEach(el => el.remove());
    document.querySelectorAll(".conv.aurora-executing").forEach(el => el.classList.remove("aurora-executing"));
    drawn = "";
  }

  return { wanted, tiles, markup, snapshot, draw, clear };
})();
globalThis.Bento = Bento;
