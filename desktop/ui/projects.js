// Unified, calm Project views: one-step creation (zero tasks started),
// full-stage Project home in #main (never trapped in the right inspector
// panel), and direct, no-ceremony deletion from sidebar, list, or header.
"use strict";

const Projects = (() => {
  /** Whether the sidebar inline "New project" input is currently open. */
  let addingInline = false;

  /** Inline sidebar creation form shown when clicking `+` next to Projects. */
  function inlineNewHtml() {
    if (!addingInline) return "";
    return `<form class="nav-project-new" data-project-inline-form><input type="text" data-project-inline-input placeholder="Project name…" aria-label="New project name" autocomplete="off"><button type="submit" class="icon-btn" title="Create project" aria-label="Create project">${icon("plus")}</button></form>`;
  }

  /** Open and focus the inline sidebar project input. */
  function openInlineNew() {
    addingInline = true;
    if (typeof sidebar === "function") sidebar();
    requestAnimationFrame(() => {
      const inp = /** @type {?HTMLInputElement} */ (document.querySelector("[data-project-inline-input]"));
      inp?.focus();
    });
  }

  /** Close the inline sidebar project input. */
  function closeInlineNew() {
    if (!addingInline) return;
    addingInline = false;
    if (typeof sidebar === "function") sidebar();
  }

  /** Delete a personal project with one confirmation and an Undo toast. */
  async function deletePersonal(id, name = "this project") {
    if (!(await ask(`Delete “${name}”? It moves to Trash, where you can restore it.`, "Delete project"))) return false;
    try {
      await Server.trash.add(id);
      App.projects = (App.projects || []).filter(p => p.id !== id);
      toast(`Deleted “${name}”.`, 6000, {
        label: "Undo",
        onClick: async () => {
          try { await Server.trash.restore(id); toast("Restored."); await refresh(); }
          catch (err) { toast(err.message); }
        },
      });
      if (location.hash === `#/project/${enc(id)}`) location.hash = "#/projects";
      if (typeof Panel !== "undefined" && Panel.what === `project:${id}`) closePanel(false);
      await refresh();
      return true;
    } catch (err) {
      toast(err.message);
      return false;
    }
  }

  /**
   * Delete a shared project (workspace) with minimal ceremony:
   * - Solo project (1 member): single confirm dialog.
   * - Active shared project (2+ members): typed-name confirmation.
   */
  async function deleteShared(wsId, name = "") {
    const home = (typeof Team !== "undefined" && (Team.homes?.[wsId] || (Team.ws === wsId ? Team.home : null)))
      || await Server.workspaces.home(wsId).catch(() => null);
    const title = home?.name || name || "this project";
    const memberCount = home?.members?.length || 1;
    const confirmed = memberCount > 1
      ? await askTyped(`Delete “${title}” for all ${memberCount} people? Shared conversations, files, and tasks will be permanently deleted. Personal spaces are untouched.`, title, "Delete project")
      : await ask(`Delete “${title}”? Its conversations and files will be permanently deleted. Your personal space is untouched.`, "Delete project");
    if (!confirmed) return false;
    try {
      await Server.workspaces.remove(wsId);
      App.workspaces = (App.workspaces || []).filter(w => w.id !== wsId);
      if (typeof Team !== "undefined") {
        delete Team.homes[wsId];
        if (Team.ws === wsId) { Team.ws = null; Team.home = null; }
      }
      if (location.hash.startsWith(`#/team/${enc(wsId)}`)) location.hash = "#/projects";
      toast(`Deleted “${title}”.`);
      await refresh();
      return true;
    } catch (err) {
      if (!err.cancelled) toast(err.body?.message || err.message);
      return false;
    }
  }

  /** Unified Projects list page (`#/projects`): personal + shared in one place. */
  function listHtml(personal, shared) {
    const pRows = (personal || []).map(p => {
      const counts = [
        p.counts?.files ? `${p.counts.files} ${p.counts.files === 1 ? "file" : "files"}` : "",
        p.counts?.tasks ? `${p.counts.tasks} ${p.counts.tasks === 1 ? "task" : "tasks"}` : "",
        p.counts?.schedules ? `${p.counts.schedules} ${p.counts.schedules === 1 ? "routine" : "routines"}` : "",
      ].filter(Boolean).join(" · ") || "Private";
      return `<li class="project-row" data-project-row="${esc(p.id)}">
        <a class="project-row-link" href="#/project/${enc(p.id)}">${icon("folder")}<span class="project-row-main"><strong>${esc(p.name)}</strong><span class="small muted">${esc(counts)}</span></span></a>
        <button type="button" class="icon-btn project-row-del" data-del-personal="${esc(p.id)}" data-del-name="${esc(p.name)}" title="Delete ${esc(p.name)}" aria-label="Delete ${esc(p.name)}">${icon("trash")}</button>
      </li>`;
    }).join("");
    const sRows = (shared || []).map(w => {
      const canDel = w.role === "owner" || w.role === "admin";
      return `<li class="project-row" data-workspace-row="${esc(w.id)}">
        <a class="project-row-link" href="#/team/${enc(w.id)}">${icon("team")}<span class="project-row-main"><strong>${esc(w.name)}</strong><span class="small muted">Shared</span></span></a>
        ${canDel ? `<button type="button" class="icon-btn project-row-del" data-del-shared="${esc(w.id)}" data-del-name="${esc(w.name)}" title="Delete ${esc(w.name)}" aria-label="Delete ${esc(w.name)}">${icon("trash")}</button>` : ""}
      </li>`;
    }).join("");
    const any = pRows || sRows;
    return `<div class="project-page">
      <header class="project-page-head"><h1>Projects</h1></header>
      <form class="project-create-bar" data-project-create>
        <input type="text" name="name" placeholder="New project name…" aria-label="New project name" autocomplete="off" required>
        <button type="submit" class="primary">Create</button>
      </form>
      ${any ? `<ul class="project-list">${pRows}${sRows}</ul>` : `<p class="empty">No projects yet. Type a name above to create one — nothing runs until you ask Magnus.</p>`}
    </div>`;
  }

  /** Render the unified Projects list in `#main`. */
  async function showList(main) {
    if (typeof closePanel === "function" && Panel?.what) closePanel(false);
    App.thread = null;
    sidebar();
    paint(main, listHtml(App.projects || [], App.workspaces || []));
    main.onsubmit = async e => {
      const f = /** @type {HTMLElement} */ (e.target).closest?.("[data-project-create]");
      if (!f) return;
      e.preventDefault();
      const inp = /** @type {?HTMLInputElement} */ ($('input[name="name"]', f));
      const name = (inp?.value || "").trim();
      if (!name) return;
      try {
        const r = await Server.projects.create({ name, instructions: "" });
        await refresh();
        location.hash = `#/project/${enc(r.id)}`;
      } catch (err) { toast(err.message); }
    };
    main.onclick = async e => {
      const dp = /** @type {HTMLElement} */ (e.target).closest?.("[data-del-personal]");
      if (dp) {
        e.preventDefault();
        if (await deletePersonal(dp.dataset.delPersonal, dp.dataset.delName)) paint(main, listHtml(App.projects || [], App.workspaces || []));
        return;
      }
      const ds = /** @type {HTMLElement} */ (e.target).closest?.("[data-del-shared]");
      if (ds) {
        e.preventDefault();
        if (await deleteShared(ds.dataset.delShared, ds.dataset.delName)) paint(main, listHtml(App.projects || [], App.workspaces || []));
      }
    };
  }

  /** Render a single project's clean full-stage HTML. */
  function pageHtml(p) {
    const hasIns = Boolean((p.instructions || "").trim());
    const hasTasks = Boolean(p.tasks?.length);
    const hasScheds = Boolean(p.schedules?.length);
    return `<div class="project-page" data-project-page="${esc(p.id)}">
      <header class="project-page-head">
        <div class="project-title-wrap">
          <a class="small muted project-crumb" href="#/projects">Projects</a>
          <form class="project-rename-form" data-project-rename="${esc(p.id)}">
            <input type="text" class="project-title-input" name="name" value="${esc(p.name)}" aria-label="Project name" autocomplete="off">
          </form>
        </div>
        <div class="row project-head-acts">
          <a class="button primary" href="#/chat/new/${enc(p.id)}">New conversation</a>
          <button type="button" class="shared-promote-btn" data-project-share="${esc(p.id)}" data-project-name="${esc(p.name)}" title="Add people to collaborate">${icon("team")}<span>Add people</span></button>
          <button type="button" class="icon-btn" data-project-delete="${esc(p.id)}" data-project-name="${esc(p.name)}" title="Delete project" aria-label="Delete project">${icon("trash")}</button>
        </div>
      </header>
      <form class="project-hero-composer" data-project-ask="${esc(p.id)}">
        <input type="text" name="prompt" placeholder="Ask Magnus anything in ${esc(p.name)}… (nothing runs until you send)" aria-label="Ask Magnus in ${esc(p.name)}" autocomplete="off">
        <button type="submit" class="primary">Ask Magnus</button>
      </form>
      <div class="project-sections project-hub-grid">
        <div class="project-hub-main">
          <section class="project-sec">
            <div class="spread"><h2>Conversations</h2></div>
            ${p.threads?.length ? `<ul class="tlist">${p.threads.map(t => `<li class="trow"><a href="#/chat/${enc(t.thread)}"><span class="ttl">${esc(t.title)}</span><span class="small faint when">${esc(when(t.updated_at))}</span></a></li>`).join("")}</ul>` : `<p class="empty">No conversations yet. Start one when you're ready — Magnus waits until you ask.</p>`}
          </section>
          <section class="project-sec">
            <div class="spread"><h2>Files</h2>${attachButton("Add files")}</div>
            ${p.files?.length ? `<ul class="flist">${p.files.map(f => `<li data-project-file="${esc(f.id)}">${fileChip(f)} <span class="small faint">${f.source === "delivered" ? "delivered" : "uploaded"} ${esc(when(f.at))}</span></li>`).join("")}</ul>` : `<p class="small muted">No files yet.</p>`}
          </section>
        </div>
        <div class="project-hub-side">
          <details class="project-sec project-instructions"${hasIns ? " open" : ""}>
            <summary><span>Instructions</span><span class="small muted">${hasIns ? "Active" : "Optional"}</span></summary>
            <div class="project-ins-body">
              <textarea id="pin" rows="3" data-draft="pin-${esc(p.id)}" placeholder="Rules or context Magnus should follow in this project…">${esc(p.instructions || "")}</textarea>
              <div class="row"><button type="button" data-project-save-ins="${esc(p.id)}">Save</button></div>
            </div>
          </details>
          ${hasTasks ? `<section class="project-sec"><h2>Tasks</h2><ul class="tlist">${p.tasks.map(taskRow).join("")}</ul></section>` : ""}
          ${hasScheds ? `<section class="project-sec"><h2>Routines</h2><ul class="tlist">${p.schedules.map(x => `<li class="trow"><a href="#/schedules" data-open="schedule:${esc(x.id)}"><span class="ttl">${esc(x.title)}</span><span class="small faint when">Next ${esc(until(x.next))}</span></a></li>`).join("")}</ul></section>` : ""}
        </div>
      </div>
    </div>`;
  }

  /** Render a single project in `#main` (`#/project/:id`). */
  async function show(main, id) {
    if (typeof closePanel === "function" && Panel?.what && (Panel.what === `project:${id}` || Panel.what === "projects")) closePanel(false);
    App.thread = null;
    sidebar();
    const draw = async () => {
      try {
        const p = await Server.projects.get(id);
        if (location.hash !== `#/project/${enc(id)}`) return;
        paint(main, pageHtml(p));
      } catch (err) {
        paint(main, `<div class="project-page"><div class="banner bad">${esc(err.status === 404 ? "This project was deleted or does not exist." : err.message)}</div><p><a href="#/projects">Back to Projects</a></p></div>`);
      }
    };
    await draw();
    main.onsubmit = async e => {
      const askForm = /** @type {HTMLElement} */ (e.target).closest?.("[data-project-ask]");
      if (askForm) {
        e.preventDefault();
        const inp = /** @type {?HTMLInputElement} */ ($('input[name="prompt"]', askForm));
        const text = (inp?.value || "").trim();
        if (!text) return;
        App.appSearch = text;
        location.hash = `#/chat/new/${enc(id)}`;
        return;
      }
      const f = /** @type {HTMLElement} */ (e.target).closest?.("[data-project-rename]");
      if (!f) return;
      e.preventDefault();
      const inp = /** @type {?HTMLInputElement} */ ($('input[name="name"]', f));
      const name = (inp?.value || "").trim();
      if (!name) return;
      try {
        await Server.projects.update(id, { name });
        toast("Renamed.");
        await refresh();
      } catch (err) { toast(err.message); }
    };
    main.onchange = async e => {
      const t = /** @type {HTMLInputElement} */ (e.target);
      if (t.matches?.("[data-attach]") && t.files?.length) {
        try {
          const up = await upload(t.files, { project: id });
          toast(`Uploaded ${up.map(f => f.name).join(", ")}.`);
          t.value = "";
          await refresh();
          await draw();
        } catch (err) { toast(err.message); }
      }
    };
    main.onclick = async e => {
      const t = /** @type {HTMLElement} */ (e.target);
      if (t.closest?.("[data-project-save-ins]")) {
        try {
          const val = /** @type {?HTMLTextAreaElement} */ ($("#pin", main))?.value || "";
          await Server.projects.update(id, { instructions: val });
          delete drafts[`pin-${id}`];
          toast("Instructions saved.");
          await refresh();
        } catch (err) { toast(err.message); }
        return;
      }
      const del = t.closest?.("[data-project-delete]");
      if (del) {
        await deletePersonal(del.dataset.projectDelete, del.dataset.projectName);
      }
    };
  }

  /** Render a shared project's unified Project Home in `#main` (`#/team/:ws`). */
  function sharedPageHtml(home, panel) {
    const ws = home.id;
    const edit = home.role !== "guest";
    const canDel = home.role === "owner" || home.role === "admin";
    const chats = (home.channels || []).filter(c => !c.direct && !c.archived && !c.deleted);
    const files = panel?.files || [];
    const members = home.members || [];
    return `<div class="project-page" data-shared-project-page="${esc(ws)}">
      <header class="project-page-head">
        <div class="project-title-wrap">
          <a class="small muted project-crumb" href="#/projects">Projects</a>
          <h1>${esc(home.name)} <span class="nav-shared-badge">Shared</span></h1>
        </div>
        <div class="row project-head-acts">
          ${chats[0] ? `<a class="button primary" href="#/team/${enc(ws)}/c/${enc(chats[0].id)}">Open chat</a>` : ""}
          <button type="button" class="shared-members-btn" data-open="shared:settings" title="People and project settings">${icon("team")}<span>${members.length} ${members.length === 1 ? "person" : "people"}</span></button>
          ${canDel ? `<button type="button" class="icon-btn" data-shared-project-delete="${esc(ws)}" data-shared-project-name="${esc(home.name)}" title="Delete project" aria-label="Delete project">${icon("trash")}</button>` : ""}
        </div>
      </header>
      ${edit && chats[0] ? `<form class="project-hero-composer" data-shared-project-ask="${esc(ws)}" data-shared-default-ch="${esc(chats[0].id)}">
        <input type="text" name="prompt" placeholder="Ask @${esc(home.agent?.name || "Magnus")} anything in ${esc(home.name)}… (nothing runs until you send)" aria-label="Ask Magnus in ${esc(home.name)}" autocomplete="off">
        <button type="submit" class="primary">Ask ${esc(home.agent?.name || "Magnus")}</button>
      </form>` : ""}
      <div class="project-sections project-hub-grid">
        <div class="project-hub-main">
          <section class="project-sec">
            <div class="spread"><h2>Shared chats</h2></div>
            ${chats.length ? `<ul class="tlist">${chats.map(c => `<li class="trow"><a href="#/team/${enc(ws)}/c/${enc(c.id)}"><span class="ttl">${esc(c.name)}</span>${c.unread ? `<span class="small faint when">${c.unread} new</span>` : ""}</a></li>`).join("")}</ul>` : `<p class="empty">No shared chats yet.</p>`}
            ${edit ? `<form class="project-create-bar" data-shared-new-chat="${esc(ws)}"><input type="text" name="name" placeholder="New shared chat name…" aria-label="New shared chat name" autocomplete="off" required><button type="submit">New chat</button></form>` : ""}
          </section>
          <section class="project-sec">
            <div class="spread"><h2>Shared documents</h2>${edit ? `<label class="button">${icon("file")}<span>Add files</span><input type="file" multiple data-shared-upload hidden></label>` : ""}</div>
            ${files.length ? `<ul class="flist">${files.map(f => `<li data-shared-file="${esc(f.name)}"><span class="fchip"><span>${esc(f.name)}</span> <button type="button" class="link small" data-shared-file-dl="${esc(f.name)}">Download</button>${edit ? ` <button type="button" class="link small" data-shared-file-del="${esc(f.name)}">Delete</button>` : ""}</span> <span class="small faint">${esc(f.by_name || "")}${f.size ? ` · ${esc(kb(f.size))}` : ""}</span></li>`).join("")}</ul>` : `<p class="small muted">No shared documents yet.</p>`}
          </section>
        </div>
        <div class="project-hub-side">
          <details class="project-sec project-instructions"${(home.instructions || "").trim() ? " open" : ""}>
            <summary><span>Instructions</span><span class="small muted">${(home.instructions || "").trim() ? "Active" : "Optional"}</span></summary>
            <div class="project-ins-body">
              <textarea id="spin" rows="3" data-draft="spin-${esc(ws)}" placeholder="Rules or context Magnus should follow in this shared project…"${edit ? "" : " readonly"}>${esc(home.instructions || "")}</textarea>
              ${edit ? `<div class="row"><button type="button" data-shared-save-ins="${esc(ws)}">Save</button></div>` : ""}
            </div>
          </details>
          <section class="project-sec">
            <div class="spread"><h2>People</h2><button type="button" class="link small" data-open="shared:settings">${edit ? "Add or invite" : "See all"}</button></div>
            <ul class="tlist">${members.map(m => `<li class="trow"><span><span class="nav-dot${m.online ? " ok" : ""}" aria-hidden="true"></span> ${esc(m.name)}${m.id === home.me ? ` <span class="small faint">you</span>` : ""}</span><span class="small faint">${esc(typeof teamRoleLabel === "function" ? teamRoleLabel(m.role) : m.role)}</span></li>`).join("")}</ul>
          </section>
        </div>
      </div>
    </div>`;
  }

  /** Render a shared project's unified home in `#main` (`#/team/:ws`). */
  async function showShared(main, wsId) {
    App.thread = null;
    if (typeof Team !== "undefined") { Team.ws = wsId; Team.place = { kind: "home" }; }
    const draw = async () => {
      try {
        const [home, panel] = await Promise.all([
          typeof loadHome === "function" ? loadHome(wsId) : Server.workspaces.home(wsId),
          Server.workspaces.panel ? Server.workspaces.panel(wsId, null).catch(() => ({ files: [] })) : Promise.resolve({ files: [] }),
        ]);
        if (location.hash !== `#/team/${enc(wsId)}`) return;
        App.shared = { ws: wsId, channel: firstChannel(home), project: null, home, panel };
        if (typeof sidebar === "function") sidebar();
        paint(main, sharedPageHtml(home, panel));
      } catch (err) {
        if (err?.status === 404 && typeof leftWorkspace === "function") return leftWorkspace(wsId);
        paint(main, `<div class="project-page"><div class="banner bad">${esc(err.message)}</div><p><a href="#/projects">Back to Projects</a></p></div>`);
      }
    };
    await draw();
    if (typeof Team !== "undefined") Team.onWorkspaceEvent = () => { if (location.hash === `#/team/${enc(wsId)}` && main.isConnected) draw(); };
    if (typeof onLeave === "function") {
      onLeave(() => {
        if (typeof Team !== "undefined") Team.onWorkspaceEvent = null;
        if (App.shared?.ws === wsId) App.shared = null;
        if (typeof Panel !== "undefined" && String(Panel.what || "").startsWith("shared:")) closePanel(false);
      });
    }
    main.onsubmit = async e => {
      const askForm = /** @type {HTMLElement} */ (e.target).closest?.("[data-shared-project-ask]");
      if (askForm) {
        e.preventDefault();
        const inp = /** @type {?HTMLInputElement} */ ($('input[name="prompt"]', askForm));
        const text = (inp?.value || "").trim();
        const ch = askForm.dataset.sharedDefaultCh || "";
        if (!text || !ch) return;
        const agentName = App.shared?.home?.agent?.name || "Magnus";
        const body = /^@magnus\b/i.test(text) ? text : `@${agentName} ${text}`;
        try {
          await Server.channels.post(wsId, ch, { text: body });
          location.hash = `#/team/${enc(wsId)}/c/${enc(ch)}`;
        } catch (err) { toast(err.message); }
        return;
      }
      const newChat = /** @type {HTMLElement} */ (e.target).closest?.("[data-shared-new-chat]");
      if (newChat) {
        e.preventDefault();
        const inp = /** @type {?HTMLInputElement} */ ($('input[name="name"]', newChat));
        const name = (inp?.value || "").trim();
        if (!name) return;
        try {
          const r = await Server.workspaces.start(wsId, { kind: "channel", name });
          if (typeof loadHome === "function") await loadHome(wsId);
          location.hash = `#/team/${enc(wsId)}/c/${enc(r.channel)}`;
        } catch (err) { toast(err.message); }
      }
    };
    main.onchange = async e => {
      const t = /** @type {HTMLInputElement} */ (e.target);
      if (t.matches?.("[data-shared-upload]") && t.files?.length) {
        try {
          for (const f of t.files) {
            await Magnus.putBytes(`/v1/workspaces/${enc(wsId)}/files/${enc(f.name)}`, new Uint8Array(await f.arrayBuffer()));
          }
          toast(`Uploaded ${[...t.files].map(f => f.name).join(", ")}.`);
          t.value = "";
          await draw();
        } catch (err) { toast(err.message); }
      }
    };
    main.onclick = async e => {
      const t = /** @type {HTMLElement} */ (e.target);
      if (t.closest?.("[data-shared-save-ins]")) {
        try {
          const val = /** @type {?HTMLTextAreaElement} */ ($("#spin", main))?.value || "";
          await Server.workspaces.setInstructions(wsId, val);
          delete drafts[`spin-${wsId}`];
          toast("Instructions saved.");
          await draw();
        } catch (err) { toast(err.message); }
        return;
      }
      const dl = t.closest?.("[data-shared-file-dl]");
      if (dl) {
        const name = dl.dataset.sharedFileDl || "";
        try { await Magnus.download(`/v1/workspaces/${enc(wsId)}/files/${enc(name)}`, name); } catch (err) { toast(err.message); }
        return;
      }
      const rm = t.closest?.("[data-shared-file-del]");
      if (rm) {
        const name = rm.dataset.sharedFileDel || "";
        if (await ask(`Delete ${name}?`, "Delete")) {
          try { await Server.workspaces.removeFile(wsId, name); await draw(); } catch (err) { toast(err.message); }
        }
        return;
      }
      const del = t.closest?.("[data-shared-project-delete]");
      if (del) {
        await deleteShared(del.dataset.sharedProjectDelete, del.dataset.sharedProjectName);
      }
    };
  }

  /** Global sidebar handlers for `+` inline creation and quick hover delete. */
  function wireGlobal() {
    document.addEventListener("click", async e => {
      const t = /** @type {HTMLElement} */ (e.target);
      if (t.closest?.("[data-new-project]")) {
        e.preventDefault();
        if (addingInline) closeInlineNew(); else openInlineNew();
        return;
      }
      const dp = t.closest?.("[data-nav-del-project]");
      if (dp) {
        e.preventDefault();
        e.stopPropagation();
        await deletePersonal(dp.dataset.navDelProject, dp.dataset.navDelName);
        return;
      }
      const ds = t.closest?.("[data-nav-del-shared]");
      if (ds) {
        e.preventDefault();
        e.stopPropagation();
        await deleteShared(ds.dataset.navDelShared, ds.dataset.navDelName);
      }
    });
    document.addEventListener("submit", async e => {
      const f = /** @type {HTMLElement} */ (e.target).closest?.("[data-project-inline-form]");
      if (!f) return;
      e.preventDefault();
      const inp = /** @type {?HTMLInputElement} */ ($("[data-project-inline-input]", f));
      const name = (inp?.value || "").trim();
      if (!name) { closeInlineNew(); return; }
      addingInline = false;
      try {
        const r = await Server.projects.create({ name, instructions: "" });
        await refresh();
        location.hash = `#/project/${enc(r.id)}`;
      } catch (err) { toast(err.message); }
    });
    document.addEventListener("keydown", e => {
      if (e.key === "Escape" && addingInline && /** @type {HTMLElement} */ (e.target).matches?.("[data-project-inline-input]")) {
        e.preventDefault();
        closeInlineNew();
      }
    });
  }

  if (typeof document !== "undefined") wireGlobal();

  return { inlineNewHtml, openInlineNew, closeInlineNew, listHtml, pageHtml, sharedPageHtml, showList, show, showShared, deletePersonal, deleteShared };
})();
