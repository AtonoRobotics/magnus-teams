// Unified Shared Canvas & Bento adapter: surfaces Missions, private Release
// Airlock drafts, Tasks, Files, Routines, Knowledge, and Project Settings
// natively in the single Aurora Bento and Canvas (#detail) when viewing a
// shared conversation.
"use strict";

const SharedCanvas = (() => {
  /**
   * Native Bento tiles for an active shared conversation: private Release
   * Airlock drafts waiting on the current user (urgent `needs`) and active
   * Missions (`run`) or finished shared outputs (`done`).
   */
  function boxes() {
    const sh = typeof App !== "undefined" ? App.shared : null;
    const home = sh?.home || (typeof Team !== "undefined" ? Team.home : null);
    if (!sh || !home) return [];
    const ws = sh.ws, missions = sh.missions || [], list = [];
    const openOnMissions = typeof Panel !== "undefined" && Panel.what === "shared:missions";
    for (const m of missions) {
      for (const l of (m.lanes || [])) {
        if (l.owner === home.me && l.status === "waiting_release" && !openOnMissions) {
          list.push({
            key: `airlock:${m.id}:${l.id}`,
            label: "Release Airlock",
            icon: "need",
            open: "shared:missions",
            href: `#/team/${enc(ws)}`,
            tone: "needs",
            title: m.goal,
            desc: l.scrub_flagged ? "Sensitive data flagged — review privately before sharing" : "Only you see this draft — review and share with the group",
          });
        }
      }
    }
    if (!openOnMissions) {
      const active = missions.filter(m => !m.complete);
      if (active.length) {
        const m = active[0];
        const total = m.total_lanes || (m.lanes || []).length;
        list.push({
          key: `mission:${m.id}`,
          label: "Mission",
          icon: "sparkles",
          open: "shared:missions",
          href: `#/team/${enc(ws)}`,
          tone: "run",
          count: active.length > 1 ? active.length : 0,
          title: m.goal,
          desc: `${m.released_count || 0} of ${total} updates shared`,
        });
      } else {
        const done = missions.find(m => m.complete && m.deliverable);
        if (done) {
          list.push({
            key: `mission:${done.id}`,
            label: "Mission ready",
            icon: "done",
            open: "shared:missions",
            href: `#/team/${enc(ws)}`,
            tone: "done",
            title: done.goal,
            desc: firstLine(done.deliverable),
          });
        }
      }
    }
    return list;
  }

  /** Context-aware Views popover entries when inside a shared conversation. */
  function dockItems(ws) {
    const base = `#/team/${enc(ws)}`;
    return [
      ["shared:missions", base, "sparkles", "Missions"],
      ["shared:tasks", `${base}/p/board`, "clock", "Tasks"],
      ["shared:files", `${base}/files`, "file", "Files"],
      ["shared:routines", base, "calendar", "Routines"],
      ["shared:knowledge", `${base}/knowledge`, "shield", "Knowledge"],
      ["shared:settings", `${base}/people`, "gear", "Project settings"],
    ];
  }

  /**
   * Renders a shared project view (`missions`, `tasks`, `files`, `routines`,
   * `knowledge`, `settings`, `activity`, `search:<q>`) inside the unified Canvas.
   * @param {string} [subId]
   */
  async function view(subId = "missions") {
    const sh = (typeof App !== "undefined" && App.shared) || (typeof Team !== "undefined" && Team.ws ? { ws: Team.ws, channel: TeamChat.s?.channel || null, project: null, home: Team.home } : null);
    if (!sh || !sh.ws) return { body: `<p class="empty">Open a shared conversation to view its workspace.</p>` };
    const ws = sh.ws, home = sh.home || Team.home || await loadHome(ws);
    const channel = sh.channel || firstChannel(home);
    const project = sh.project || home.channels.find(c => c.id === channel)?.project || null;
    const colon = subId.indexOf(":");
    const kind = colon < 0 ? (subId || "missions") : subId.slice(0, colon);
    const arg = colon < 0 ? "" : subId.slice(colon + 1);
    const titles = { missions: "Missions", tasks: "Tasks", files: "Files", routines: "Routines", knowledge: "Knowledge", settings: "Project settings", activity: "Activity", search: arg ? `Search: ${arg}` : "Search" };
    const head = `<h2>${esc(titles[kind] || "Shared project")}</h2><p class="small muted">${esc(home.name)}</p>`;
    const nameOf = id => id === "agent" ? home.agent.name : home.members.find(m => m.id === id)?.name || id;
    const where = id => { const c = home.channels.find(x => x.id === id); return c ? esc(channelTitle(home, c)) : ""; };
    const edit = home.role !== "guest";
    const redraw = () => { if (typeof drawPanel === "function") drawPanel(); if (typeof AuroraStage !== "undefined") AuroraStage.refresh(); };

    if (kind === "missions") {
      const [d, missions] = await Promise.all([
        TeamPanel.load(ws, project).catch(() => ({ work: [], delegations: [], routines: [], tasks: [], files: [], members: home.members })),
        channel && Server.workspaces.missions ? Server.workspaces.missions(ws, channel).catch(() => []) : Promise.resolve([]),
      ]);
      if (App.shared?.ws === ws) App.shared.missions = missions || [];
      const workHtml = typeof TeamRooms !== "undefined" && missions?.length
        ? TeamRooms.workPanelHtml(missions, ws, home.me, nameOf)
        : `<div class="room-empty-work"><div class="small"><strong>Missions &amp; Release Airlock</strong></div><p class="small muted">Start shared work below or mention @${esc(home.agent.name)}. Teammates review their private slices before sharing.</p></div>`;
      const others = home.members.filter(m => m.id !== home.me);
      const memberPicks = others.map(m => `<label class="room-pick small"><input type="checkbox" name="member" value="${esc(m.id)}"> ${esc(m.name)}</label>`).join("");
      const form = channel && edit ? `<section class="companion-sec"><div class="companion-sec-head"><span>Start shared work</span></div>
        <form class="room-mission-form" data-mission-new data-mission-room="${esc(channel)}">
          <div class="room-steer-row"><input name="goal" placeholder="e.g. Draft Friday's board update…" aria-label="Shared work goal" autocomplete="off"><button class="primary" type="submit">Start</button></div>
          <div class="room-picks">${memberPicks}<label class="room-pick small"><input type="checkbox" name="open_web" value="1"> Public web</label></div>
        </form></section>` : "";
      const body = `${workHtml}${form}${delegationsHtml(d, ws, where)}`;
      return {
        head, body,
        after: el => {
          if (el.dataset.sharedMissionsWs === ws) return;
          el.dataset.sharedMissionsWs = ws;
          if (typeof TeamRooms !== "undefined") TeamRooms.wire(el, ws, redraw);
          el.addEventListener("submit", async e => {
            const f = e.target.closest?.("[data-mission-new]");
            if (!f) return;
            e.preventDefault();
            const goal = (f.querySelector('input[name="goal"]')?.value || "").trim();
            const room = f.dataset.missionRoom || channel;
            if (!goal || !room || !Server.workspaces.createMission) return;
            const lanes = [{ kind: "member_agent", target_member: home.me, objective: goal, scope: "room_work" }];
            for (const cb of f.querySelectorAll('input[name="member"]:checked')) if (cb.value) lanes.push({ kind: "member_agent", target_member: cb.value, objective: goal, scope: "room_work" });
            if (f.querySelector('input[name="open_web"]:checked')) lanes.push({ kind: "open_web", objective: goal, scope: "public_research" });
            try { await Server.workspaces.createMission(ws, room, { goal, lanes }); redraw(); } catch (err) { toast(err.message); }
          });
        },
      };
    }

    if (kind === "tasks") {
      const d = await TeamPanel.load(ws, project);
      const body = `${d.progress ? `<div class="team-side-progress">${progressBar(d.progress)}</div>` : ""}${boardHtml(d.tasks || [], ws, nameOf, edit)}`;
      return {
        head, body,
        after: el => {
          el.onchange = async e => {
            if (!e.target.matches("[data-task-status]")) return;
            const id = e.target.closest("[data-task]")?.dataset.task;
            if (!id) return;
            try { await Server.workspaces.updateTask(ws, id, { status: e.target.value }); redraw(); } catch (err) { toast(err.message); }
          };
          el.onsubmit = async e => {
            const f = e.target.closest("[data-new-task]");
            if (!f) return;
            e.preventDefault();
            const title = $("input", f)?.value.trim();
            if (!title) return;
            try { await Server.workspaces.addTask(ws, { title, project: project || undefined, status: f.dataset.newTask }); redraw(); } catch (err) { toast(err.message); }
          };
          el.onclick = async e => {
            const rm = e.target.closest("[data-task-remove]");
            if (!rm) return;
            const id = rm.closest("[data-task]")?.dataset.task;
            if (!id) return;
            try { await Server.workspaces.removeTask(ws, id); redraw(); } catch (err) { toast(err.message); }
          };
        },
      };
    }

    if (kind === "files") {
      const d = await TeamPanel.load(ws, project);
      const files = d.files || [];
      const body = `${files.length ? `<ul class="team-files">${files.map(f => `<li data-team-file="${esc(f.name)}"><span>${esc(f.name)}</span><span class="small faint">${esc(f.by_name)} · ${esc(kb(f.size))}</span><button class="link" data-file-save>Download</button>${edit ? `<button class="link" data-file-delete>Delete</button>` : ""}</li>`).join("")}</ul>` : `<p class="empty">No files yet. Drop one into the conversation or add one below.</p>`}
        ${edit ? `<label class="team-drop" data-drop-files>Add files to ${esc(home.name)}: <input type="file" multiple data-project-files aria-label="Add files"></label>` : ""}`;
      return {
        head, body,
        after: el => {
          el.onchange = async e => {
            if (!e.target.matches("[data-project-files]")) return;
            for (const f of e.target.files) {
              try { await Magnus.putBytes(`/v1/workspaces/${enc(ws)}/files/${enc(f.name)}${project ? `?project=${enc(project)}` : ""}`, new Uint8Array(await f.arrayBuffer())); } catch (err) { toast(err.message); }
            }
            e.target.value = "";
            redraw();
          };
          el.onclick = async e => {
            const name = e.target.closest("[data-team-file]")?.dataset.teamFile;
            if (!name) return;
            const at = `/v1/workspaces/${enc(ws)}/files/${enc(name)}`;
            try {
              if (e.target.closest("[data-file-save]")) await Magnus.download(at, name);
              if (e.target.closest("[data-file-delete]") && await ask(`Delete ${name}?`, "Delete")) { await Server.workspaces.removeFile(ws, name); redraw(); }
            } catch (err) { toast(err.message); }
          };
        },
      };
    }

    if (kind === "routines") {
      const d = await TeamPanel.load(ws, project);
      const body = routinesHtml(d, home, channel, where) || `<p class="empty">No scheduled routines yet.</p>`;
      return {
        head, body,
        after: el => {
          el.onsubmit = async e => {
            const f = e.target.closest("[data-routine-new]");
            if (!f) return;
            e.preventDefault();
            const text = f.querySelector('[name="text"]')?.value.trim();
            const every_hours = Number(f.querySelector('[name="every_hours"]')?.value) || 24;
            const ch = f.querySelector('[name="channel"]')?.value || channel;
            if (!text || !ch) return;
            try { await Server.workspaces.addRoutine(ws, { channel: ch, text, every_hours }); redraw(); } catch (err) { toast(err.message); }
          };
          el.onclick = async e => {
            const restart = e.target.closest("[data-routine-restart]"), remove = e.target.closest("[data-routine-remove]");
            try {
              if (restart) { await Server.workspaces.updateRoutine(ws, restart.dataset.routineRestart, {}); redraw(); }
              if (remove) { await Server.workspaces.removeRoutine(ws, remove.dataset.routineRemove); redraw(); }
            } catch (err) { toast(err.message); }
          };
        },
      };
    }

    if (kind === "knowledge") {
      const entries = await Server.workspaces.knowledge(ws, project || "").catch(() => []);
      const proj = home.projects.find(p => p.id === project);
      const curator = isTeamAdmin(home) || (proj && (proj.owner === home.me || (proj.curators || []).includes(home.me)));
      const types = [...new Set([...TEAM_KB_TYPES, ...entries.map(k => k.kind)])];
      const body = `<div class="row"><span class="grow small faint">Durable facts the group and ${esc(home.agent.name)} rely on.</span><button class="link" data-kb-export>Export</button></div>
        ${entries.length ? `<ul class="team-kb">${entries.map(k => kbEntryHtml(k, entries, nameOf, curator)).join("")}</ul>` : `<p class="empty">Nothing recorded yet. ${esc(home.agent.name)} adds what it learns here, and you can too.</p>`}
        ${edit ? `<form data-new-entry class="team-kb-new"><select aria-label="Type">${types.map(t => `<option>${esc(t)}</option>`).join("")}</select><textarea rows="2" placeholder="Something the group should know" aria-label="Statement"></textarea><div class="row"><button class="primary">Add</button></div></form>` : ""}`;
      return {
        head, body,
        after: el => {
          el.onsubmit = async e => {
            const f = e.target;
            if (f.matches("[data-new-entry]")) {
              e.preventDefault();
              const statement = $("textarea", f)?.value.trim();
              if (!statement) return;
              try { await Server.workspaces.addKnowledge(ws, { statement, kind: $("select", f)?.value || undefined, project: project || undefined }); redraw(); } catch (err) { toast(err.message); }
            }
            if (f.matches("[data-entry-fix]")) {
              e.preventDefault();
              const statement = $("input", f)?.value.trim(), id = f.closest("[data-entry]")?.dataset.entry;
              if (!statement || !id) return;
              try { await Server.workspaces.correctKnowledge(ws, id, statement); toast("Corrected."); redraw(); } catch (err) { toast(err.message); }
            }
          };
          el.onclick = async e => {
            const id = e.target.closest("[data-entry]")?.dataset.entry;
            try {
              if (e.target.closest("[data-kb-export]")) await Magnus.download(`/v1/workspaces/${enc(ws)}/knowledge/export`, "knowledge.json");
              if (e.target.closest("[data-entry-flag]") && id) { const why = await askText("What is wrong with it?", "Describe what should be corrected", "Flag"); if (why) { await Server.workspaces.flagKnowledge(ws, id, why); toast("Flagged."); redraw(); } }
              if (e.target.closest("[data-entry-correct]") && id) {
                const li = e.target.closest("[data-entry]"), was = entries.find(x => x.id === id)?.statement || "";
                paint($(".team-kb-head .grow", li), `<form data-entry-fix class="row"><input aria-label="Corrected statement" value="${esc(was)}"><button class="primary">Save</button></form>`);
                $("[data-entry-fix] input", li)?.focus();
              }
              if (e.target.closest("[data-entry-retire]") && id && await ask("Retire this entry?", "Retire")) { await Server.workspaces.retireKnowledge(ws, id); redraw(); }
            } catch (err) { toast(err.message); }
          };
        },
      };
    }

    if (kind === "settings") {
      const peer = canCollaborate(home);
      const [candidates, caps] = await Promise.all([
        peer ? Server.workspaces.people(ws).catch(() => []) : Promise.resolve([]),
        Server.workspaces.capabilities ? Server.workspaces.capabilities(ws).catch(() => []) : Promise.resolve([]),
      ]);
      const body = teamPeopleHtml(home, candidates, caps);
      return {
        head, body,
        after: el => {
          const again = async () => { await loadHome(ws); redraw(); };
          wirePeopleSettings(el, ws, home, candidates, again);
        },
      };
    }

    if (kind === "search") {
      const found = arg.trim() ? await Server.workspaces.search(ws, arg).catch(() => ({})) : {};
      const items = typeof foundItems === "function" ? foundItems(found, home) : [];
      const body = `<form class="team-search-form" data-shared-search><input value="${esc(arg)}" aria-label="Search shared project" placeholder="Search messages, files, tasks and knowledge"><button class="primary">Search</button></form>
        ${items.length ? `<ul class="team-activity-list">${items.map((it, i) => `<li><button class="link" data-found="${i}"><strong>${esc(it.label)}</strong></button> <span class="small faint">${esc(it.hint || "")}</span></li>`).join("")}</ul>` : `<p class="empty">${arg ? "Nothing found." : "Type what to look for."}</p>`}`;
      return {
        head, body,
        after: el => {
          el.onsubmit = e => { if (e.target.matches("[data-shared-search]")) { e.preventDefault(); openPanel(`shared:search:${$("input", e.target).value.trim()}`); } };
          el.onclick = e => { const b = e.target.closest("[data-found]"); if (b && typeof TeamCommand !== "undefined") TeamCommand.run(items[Number(b.dataset.found)].run); };
        },
      };
    }

    const a = await Server.workspaces.activity(ws).catch(() => ({ mentions: [], replies: [], asks: [] }));
    const row = i => `<li><a href="${teamHref(ws, { kind: "channel", id: i.channel, thread: i.thread || undefined })}"><strong>${esc(nameOf(i.sender))}</strong> in ${esc(i.channel_name || "direct")} <span class="small faint">${esc(when(i.at))}</span><div class="small">${Compose.maskedText(firstLine(i.text))}</div></a></li>`;
    const section = (title, list, empty) => `<h3>${title}</h3>${list.length ? `<ul class="team-activity-list">${list.map(row).join("")}</ul>` : `<p class="small faint">${empty}</p>`}`;
    return {
      head,
      body: `${a.asks?.length ? `<h3>Waiting on you</h3><ul class="team-activity-list">${a.asks.map(x => `<li><a href="#/inbox" data-open="needs">Magnus needs your answer${x.reason ? `: ${esc(x.reason)}` : ""}</a></li>`).join("")}</ul>` : ""}${section("Mentions", a.mentions || [], "No one has mentioned you.")}${section("Replies", a.replies || [], "No replies yet.")}`,
    };
  }

  /**
   * Builds the exact clean-room payload that crosses from a private thread or
   * project into a shared conversation. Never includes unchecked private turns.
   * @param {{ mode: "summary"|"selected", summary?: string, messages?: Array<{ id: string, kind: string, text: string }>, selectedIds?: string[] }} opts
   */
  function cleanRoomPayload(opts) {
    if (opts.mode === "selected") {
      const picked = new Set(opts.selectedIds || []);
      const lines = (opts.messages || [])
        .filter(m => picked.has(m.id) && String(m.text || "").trim())
        .map(m => `${m.kind === "answer" ? "Magnus" : "Note"}: ${String(m.text).trim()}`);
      return lines.join("\n\n");
    }
    return String(opts.summary || "").trim();
  }

  /**
   * Opens the clean-room Share dialog for promoting a private conversation or
   * private project into an existing or new Shared Project without exposing
   * private history.
   * @param {{ thread?: ?string, title?: string, projectId?: ?string, projectName?: string }} opts
   */
  async function openShareDialog(opts = {}) {
    const [workspaces, mayStart, directory, threadData] = await Promise.all([
      Server.workspaces.list().catch(() => App.workspaces || []),
      typeof mayStartWorkspace === "function" ? mayStartWorkspace().catch(() => false) : Promise.resolve(false),
      Server.directory ? Server.directory().catch(() => []) : Promise.resolve([]),
      opts.thread && typeof Resources !== "undefined" ? Resources.read("Conversation", `/v1/chat/${enc(opts.thread)}`).catch(() => null) : Promise.resolve(null),
    ]);
    const entries = (threadData?.entries || []).filter(e => (e.kind === "message" || e.kind === "answer") && String(e.text || "").trim());
    const lastAnswer = [...entries].reverse().find(e => e.kind === "answer");
    const defaultSummary = lastAnswer ? String(lastAnswer.text).trim() : (opts.title || opts.projectName || "");
    const defaultName = (opts.title || opts.projectName || "shared-topic").slice(0, 60);
    const dirList = Array.isArray(directory) ? directory : (directory?.people || []);
    const teammates = dirList.filter(p => p.id && p.id !== App.me?.id);
    const hasExisting = workspaces.length > 0;
    const canProceed = hasExisting || mayStart;

    const d = document.createElement("dialog");
    d.className = "share-dialog";
    d.setAttribute("aria-label", "Share to a shared project");
    const wsOptions = workspaces.map(w => `<option value="${esc(w.id)}">${esc(w.name)}</option>`).join("")
      + (mayStart ? `<option value="__new__"${!hasExisting ? " selected" : ""}>+ Create a new shared project…</option>` : "");
    const peoplePicks = teammates.length
      ? `<fieldset class="share-fieldset"><legend class="small">Add people (optional)</legend><div class="share-pick-list">${teammates.map(p => `<label class="room-pick small"><input type="checkbox" name="member" value="${esc(p.id)}"> ${esc(p.name)}</label>`).join("")}</div></fieldset>`
      : "";
    const msgPicks = entries.length
      ? entries.map(e => `<label class="share-msg-pick small"><input type="checkbox" name="msg_id" value="${esc(e.id)}"><span><strong>${e.kind === "answer" ? "Magnus" : "You"}:</strong> ${esc(firstLine(e.text).slice(0, 120))}</span></label>`).join("")
      : `<p class="small faint">No individual messages to select.</p>`;

    d.innerHTML = `<form method="dialog" class="share-form">
      <div class="share-dialog-head"><h2>Share to a shared project</h2><button type="button" class="icon-btn" data-share-close aria-label="Close">${icon("close")}</button></div>
      <p class="small share-boundary-note">Your private conversation stays private. Only what you explicitly choose below is copied into the shared conversation.</p>
      ${canProceed ? `
        <label class="stack small"><span>Shared project</span><select name="workspace" data-share-ws>${wsOptions}</select></label>
        <label class="stack small" data-share-new-wrap${hasExisting ? " hidden" : ""}><span>New shared project name</span><input name="new_ws_name" placeholder="e.g. Q4 Launch" autocomplete="off" value="${esc(opts.projectName || "")}"></label>
        <label class="stack small"><span>Shared conversation name</span><input name="channel_name" required autocomplete="off" value="${esc(defaultName)}"></label>
        ${peoplePicks}
        <label class="room-pick small"><input type="checkbox" name="copy_invite" value="1"> Copy an invite link after sharing</label>
        <fieldset class="share-fieldset">
          <legend class="small">What crosses into the shared conversation</legend>
          <label class="room-pick small"><input type="radio" name="share_mode" value="summary" checked> Summary or deliverable only (recommended)</label>
          <textarea name="summary" rows="4" aria-label="Summary to share" placeholder="Edit the summary or deliverable to share…">${esc(defaultSummary)}</textarea>
          ${entries.length ? `<label class="room-pick small"><input type="radio" name="share_mode" value="selected"> Selected messages only</label><div class="share-msg-list" data-share-msgs hidden>${msgPicks}</div>` : ""}
        </fieldset>
        <div class="row share-actions"><button type="button" data-share-close>Cancel</button><button type="submit" class="primary" data-share-go>Share clean-room copy</button></div>
      ` : `<p class="empty">Guest accounts cannot create shared projects.</p><div class="row share-actions"><button type="button" data-share-close>Close</button></div>`}
    </form>`;

    const wsSel = $("[data-share-ws]", d);
    const newWrap = $("[data-share-new-wrap]", d);
    const msgWrap = $("[data-share-msgs]", d);
    const sumBox = $('textarea[name="summary"]', d);
    wsSel?.addEventListener("change", () => {
      if (newWrap) newWrap.hidden = wsSel.value !== "__new__";
      if (wsSel.value === "__new__") $('input[name="new_ws_name"]', d)?.focus();
    });
    d.addEventListener("change", e => {
      if (!e.target.matches('input[name="share_mode"]')) return;
      const selMode = e.target.value === "selected";
      if (msgWrap) msgWrap.hidden = !selMode;
      if (sumBox) sumBox.hidden = selMode;
    });
    d.addEventListener("click", e => {
      if (e.target === d || e.target.closest("[data-share-close]")) d.close();
    });
    const form = $("form", d);
    form?.addEventListener("submit", async e => {
      e.preventDefault();
      if (!canProceed) return d.close();
      const go = $("[data-share-go]", d);
      if (go) go.disabled = true;
      try {
        let wsId = wsSel?.value || "";
        if (wsId === "__new__") {
          const wsName = ($('input[name="new_ws_name"]', d)?.value || "").trim();
          if (!wsName) { toast("Enter a name for the new shared project."); if (go) go.disabled = false; return; }
          const created = await Server.workspaces.create(wsName);
          wsId = created.id;
        }
        const pickedMembers = [...d.querySelectorAll('input[name="member"]:checked')].map(el => el.value).filter(Boolean);
        for (const pid of pickedMembers) await Server.workspaces.setMember(wsId, pid, "member").catch(() => {});
        const chName = ($('input[name="channel_name"]', d)?.value || defaultName).trim() || "shared-topic";
        const started = await Server.workspaces.start(wsId, { kind: "channel", name: chName }).catch(() => null);
        const home = await loadHome(wsId);
        const chId = started?.channel || firstChannel(home);
        const mode = d.querySelector('input[name="share_mode"]:checked')?.value === "selected" ? "selected" : "summary";
        const selectedIds = [...d.querySelectorAll('input[name="msg_id"]:checked')].map(el => el.value);
        const payload = cleanRoomPayload({ mode, summary: sumBox?.value || "", messages: entries, selectedIds });
        if (payload && chId) await Server.channels.post(wsId, chId, { text: payload });
        if ($('input[name="copy_invite"]', d)?.checked) {
          const inv = await Server.workspaces.invite(wsId).catch(() => null);
          if (inv) {
            const link = serverLink(inv);
            try { await navigator.clipboard.writeText(link); } catch { /* clipboard unavailable */ }
            toast(`Shared! Invite link copied: ${link}`, 6000);
          }
        } else {
          toast("Shared into the shared project. Your original conversation stayed private.");
        }
        d.close();
        if (typeof refresh === "function") refresh().catch(() => {});
        location.hash = chId ? teamHref(wsId, { kind: "channel", id: chId }) : `#/team/${enc(wsId)}`;
      } catch (err) {
        toast(err.message);
        if (go) go.disabled = false;
      }
    });
    d.addEventListener("close", () => d.remove());
    document.body.append(d);
    d.showModal();
  }

  return { boxes, dockItems, view, cleanRoomPayload, openShareDialog };
})();

if (typeof PANEL_TITLES !== "undefined") PANEL_TITLES.shared = "Shared project";
if (typeof PANELS !== "undefined") PANELS.shared = id => SharedCanvas.view(id);

