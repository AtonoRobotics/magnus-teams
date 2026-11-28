// Universal Cmd+K Command Bar unifying personal navigation, files, schedules,
// computers, workspace actions (TeamCommand), and instant triage/co-edit actions.
// Replaces the duplicated inline palette that previously lived in app-shell.js.
"use strict";

const CommandBar = (() => {
  const MAX_ITEMS = 14;
  let paletteList = [];
  let paletteIdx = -1;

  /** Build unified command & jump items for the current query. @param {string} raw */
  function items(raw) {
    const q = (raw || "").trim().toLowerCase();
    const homes = (typeof Team !== "undefined" && Team.homes) || {};
    const sharedChats = [], people = [];
    for (const w of (App.workspaces || [])) {
      const h = homes[w.id];
      if (!h) continue;
      for (const c of (h.channels || []).filter(x => !x.direct && !x.archived && !x.deleted)) {
        sharedChats.push({ label: `${w.name} / ${c.name}`, kind: "Shared chat", run: () => { location.hash = `#/team/${enc(w.id)}/c/${enc(c.id)}`; } });
      }
      for (const m of (h.members || []).filter(x => x.id !== h.me)) {
        const dm = (h.channels || []).find(c => c.direct && !c.direct.agent && !c.deleted && c.direct.people.length === 2 && c.direct.people.includes(m.id));
        people.push({ label: m.name, kind: "Person", run: async () => {
          if (dm) { location.hash = `#/team/${enc(w.id)}/c/${enc(dm.id)}`; return; }
          try { const r = await Server.workspaces.direct(w.id, { people: [m.id] }); if (typeof loadHome === "function") await loadHome(w.id); location.hash = `#/team/${enc(w.id)}/c/${enc(r.id)}`; } catch (err) { toast(err.message); }
        } });
      }
    }
    const workspaceActions = (App.shared && typeof Team !== "undefined" && Team.home && typeof commandsFor === "function" && typeof TeamCommand !== "undefined")
      ? commandsFor(Team.home, Team.place || { kind: "home" }, raw || "").filter(c => c.group === "Create" || c.group === "Actions").slice(0, 6).map(c => ({ label: c.label, kind: c.group, run: () => TeamCommand.run(c.run) }))
      : [];
    const pending = typeof needsMe === "function" ? needsMe() : [];
    const batchable = typeof batchableApprovals === "function" ? batchableApprovals(pending) : [];
    const firstPending = pending[0];
    const list = [
      ...workspaceActions,
      ...(firstPending ? [{ label: `Approve next ask: ${(firstPending.ask?.sentence || firstPending.summary || firstPending.kind || "Waiting").slice(0, 56)}`, kind: "Action", run: async () => { try { await Server.inbox.answer(firstPending.id, { decision: "approve", scope: "once" }); toast("Allowed."); await refresh(); } catch (err) { toast(err.message); } } }] : []),
      ...(batchable.length >= 2 ? [{ label: `Approve all ${batchable.length} pending`, kind: "Approval", run: async () => { try { await Server.inbox.batch(batchable, { decision: "approve", scope: "once" }); toast(`Allowed ${batchable.length} requests.`); await refresh(); } catch (err) { toast(err.message); } } }] : []),
      { label: "New conversation", kind: "Action", run: () => { location.hash = "#/chat/new"; } },
      { label: "Open Split Co-Edit Canvas", kind: "Action", run: () => { if (typeof globalThis.SplitCoedit !== "undefined") globalThis.SplitCoedit.open({ title: "Scratchpad", content: "" }); } },
      { label: `Approvals & Inbox${pending.length ? ` (${pending.length} waiting)` : ""}`, kind: "Page", run: () => { location.hash = "#/inbox"; } },
      { label: "Morning Briefing & Routines", kind: "Page", run: () => { location.hash = "#/briefing"; } },
      { label: "Mobile Approvals", kind: "Page", run: () => { location.hash = "#/approve"; } },
      { label: "Projects", kind: "Page", run: () => { location.hash = "#/projects"; } },
      { label: "Explore playbooks", kind: "Page", run: () => { location.hash = "#/explore"; } },
      { label: "Apps & connectors", kind: "Page", run: () => { location.hash = "#/library"; } },
      { label: "Shared projects", kind: "Page", run: () => { location.hash = "#/teams"; } },
      { label: "Toggle Dark / Light mode", kind: "Action", run: () => { const cur = Theme.effective(Theme.prefs(), Theme.os()).mode; Theme.set({ mode: cur === "dark" ? "light" : "dark" }); } },
      { label: "Appearance & Companion", kind: "Page", run: () => { location.hash = "#/settings/appearance"; } },
      { label: "Trash", kind: "Page", run: () => { location.hash = "#/trash"; } },
      { label: "Settings", kind: "Page", run: () => { location.hash = "#/settings"; } },
      ...(typeof administers === "function" && administers() ? [{ label: "Admin & Operations", kind: "Page", run: () => { location.hash = "#/admin/operations"; } }] : []),
      { label: "Keyboard shortcuts", kind: "Action", run: () => Layout.openShortcutsModal() },
      { label: "Check for updates", kind: "Action", run: () => { refreshUpdate(true); toast("Checking for updates…"); } },
      ...(pending || []).slice(0, 8).map(a => ({
        label: `Approval: ${(a.ask?.sentence || a.summary || a.kind || "Waiting decision").slice(0, 72)}`,
        kind: "Approval",
        run: () => { if (typeof openPanel === "function") openPanel(`item:${a.id}`); else location.hash = `#/approve?ask=${enc(a.id)}`; },
      })),
      ...(App.tasks || []).slice(0, 12).map(t => ({
        label: displayTitle(t.title || t.text || "Task"),
        kind: t.status === "running" || t.status === "suspended" ? "Live Task" : "Task",
        run: () => { if (typeof openPanel === "function") openPanel(`task:${t.task_id}`); else location.hash = `#/task/${enc(t.task_id)}`; },
      })),
      ...(App.schedules || []).slice(0, 8).map(s => ({
        label: s.title || s.name || "Schedule",
        kind: s.paused ? "Schedule (paused)" : "Schedule",
        run: () => { location.hash = "#/schedules"; },
      })),
      ...(App.files || []).slice(0, 8).map(f => ({
        label: f.name || f.path || "File",
        kind: "File",
        run: () => { if (typeof openPanel === "function") openPanel("files"); else location.hash = "#/files"; },
      })),
      ...(App.threads || []).slice(0, 20).map(t => ({ label: displayTitle(t.title), kind: "Chat", run: () => { location.hash = `#/chat/${enc(t.thread)}`; } })),
      ...(App.projects || []).map(p => ({ label: p.name, kind: "Project", run: () => { location.hash = `#/project/${enc(p.id)}`; } })),
      ...(App.workspaces || []).map(w => ({ label: w.name, kind: "Shared project", run: () => { location.hash = `#/team/${enc(w.id)}`; } })),
      ...sharedChats,
      ...people,
    ];
    return (q ? list.filter(x => `${x.label} ${x.kind}`.toLowerCase().includes(q)) : list).slice(0, MAX_ITEMS);
  }

  function syncActive() {
    const box = $("#workspace-search .palette-list"), inp = $("#workspace-query");
    if (!box || !inp) return;
    const opts = [...box.querySelectorAll(".palette-item")];
    opts.forEach((el, i) => {
      const on = i === paletteIdx;
      el.classList.toggle("on", on);
      el.setAttribute("aria-selected", on ? "true" : "false");
    });
    const cur = opts[paletteIdx];
    if (cur) {
      inp.setAttribute("aria-activedescendant", cur.id);
      cur.scrollIntoView?.({ block: "nearest" });
    } else {
      inp.removeAttribute("aria-activedescendant");
    }
  }

  function draw() {
    const box = $("#workspace-search .palette-list"), inp = $("#workspace-query");
    if (!box || !inp) return;
    paletteList = items(inp.value);
    paletteIdx = paletteList.length ? 0 : -1;
    inp.setAttribute("aria-expanded", paletteList.length ? "true" : "false");
    box.innerHTML = paletteList.map((it, i) => `<button type="button" role="option" id="pal-opt-${i}" aria-selected="${i === paletteIdx}" class="palette-item${i === paletteIdx ? " on" : ""}" data-pal="${i}"><span>${esc(it.label)}</span><span class="palette-kind">${esc(it.kind)}</span></button>`).join("");
    syncActive();
  }

  function open() {
    const dialog = $("#workspace-search");
    if (!dialog || dialog.open) return;
    const inp = $("#workspace-query", dialog);
    inp.value = "";
    dialog.showModal();
    draw();
    inp.focus();
  }

  /** Bind DOM listeners on #workspace-search once per shell mount. */
  function install() {
    const wsDialog = $("#workspace-search");
    if (!wsDialog) return;
    const wsInput = $("#workspace-query", wsDialog);
    const wsList = $(".palette-list", wsDialog);
    wsDialog.addEventListener("click", e => { if (e.target === wsDialog) wsDialog.close(); });
    wsInput.oninput = draw;
    wsInput.onkeydown = e => {
      if (e.key === "Escape") { e.preventDefault(); wsDialog.close(); return; }
      if (!paletteList.length) return;
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        paletteIdx = (paletteIdx + (e.key === "ArrowDown" ? 1 : -1) + paletteList.length) % paletteList.length;
        syncActive();
      } else if (e.key === "Enter" && paletteIdx >= 0 && !e.shiftKey) {
        e.preventDefault();
        const item = paletteList[paletteIdx];
        wsDialog.close();
        item?.run();
      }
    };
    wsList.onclick = e => {
      const b = e.target.closest("[data-pal]");
      if (!b) return;
      const item = paletteList[Number(b.dataset.pal)];
      wsDialog.close();
      item?.run();
    };
    $("#workspace-search form").onsubmit = e => {
      e.preventDefault();
      const q = wsInput.value.trim();
      wsDialog.close();
      if (q) {
        if (App.workspace) openPanel(App.shared ? `shared:search:${q}` : `search:${q}`);
        else { App.openAfter = `search:${q}`; location.hash = "#/home"; }
      }
    };
    $("[data-close-search]").onclick = () => wsDialog.close();
  }

  return { items, open, draw, install };
})();
