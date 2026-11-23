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
      const admin = isTeamAdmin(home);
      const [candidates, caps] = await Promise.all([
        admin ? Server.workspaces.people(ws).catch(() => []) : Promise.resolve([]),
        Server.workspaces.capabilities ? Server.workspaces.capabilities(ws).catch(() => []) : Promise.resolve([]),
      ]);
      const peopleBody = teamPeopleHtml(home, candidates).replace(/^<header class="team-head"><h1>People<\/h1><\/header>/, "");
      const capsSec = typeof TeamRooms !== "undefined" && caps?.length ? `<section class="companion-sec"><div class="companion-sec-head"><span>Advertised personal apps</span></div>${TeamRooms.peopleCapabilitiesHtml(caps, home.me)}</section>` : "";
      const spendSec = spendHtml(home, channel);
      const limitForm = home.role === "owner" ? `<form class="row small" data-shared-limit style="margin-top:8px"><input type="number" min="0" step="1" name="usd" placeholder="Monthly limit ($)" value="${home.spend?.limit_usd ?? ""}" aria-label="Monthly spend limit in USD"><button type="submit">Set limit</button>${home.spend?.limit_usd != null ? `<button type="button" class="link" data-lift-limit>Lift</button>` : ""}</form>` : "";
      const body = `${peopleBody}${capsSec}${spendSec}${limitForm}`;
      return {
        head, body,
        after: el => {
          bindInvite(el, ws);
          const who = e => e.target.closest("[data-member]");
          const app = e => e.target.closest("[data-app]");
          const again = async () => { await loadHome(ws); redraw(); };
          el.onchange = async e => {
            if (e.target.matches("[data-app-roles]")) { try { await Server.workspaces.grantApp(ws, app(e).dataset.app, APP_ROLES[e.target.value] || []); toast("Changed."); } catch (err) { toast(err.message); } return again(); }
            if (e.target.matches("[data-role]")) { try { await Server.workspaces.setMember(ws, who(e).dataset.member, e.target.value); toast("Changed."); } catch (err) { toast(err.message); } return again(); }
          };
          el.onsubmit = async e => {
            if (e.target.closest("[data-add-people]")) {
              e.preventDefault();
              const person = $("[data-add-person]", el).value, role = $("[data-add-role]", el).value;
              try { await Server.workspaces.setMember(ws, person, role); toast("Added."); await again(); } catch (err) { toast(err.message); }
            }
            if (e.target.closest("[data-shared-limit]")) {
              e.preventDefault();
              const raw = e.target.querySelector('[name="usd"]')?.value;
              const usd = raw === "" ? null : Number(raw);
              try { await Server.workspaces.setLimit(ws, { usd }); toast(usd == null ? "Limit lifted." : `Limited to ${teamUsd(usd)} a month.`); await again(); } catch (err) { toast(err.message); }
            }
          };
          el.onclick = async e => {
            if (e.target.closest("[data-lift-limit]")) { try { await Server.workspaces.setLimit(ws, { usd: null }); toast("Limit lifted."); await again(); } catch (err) { toast(err.message); } }
            if (e.target.closest("[data-remove]")) { const m = who(e); if (!(await ask(`Take ${m.dataset.name} out of ${home.name}?`, "Remove"))) return; try { await Server.workspaces.removeMember(ws, m.dataset.member); await again(); } catch (err) { toast(err.message); } }
            if (e.target.closest("[data-leave]")) { if (!(await ask(`Leave ${home.name}?`, "Leave"))) return; try { await Server.workspaces.removeMember(ws, home.me); leftWorkspaceQuietly(ws); toast(`You left ${home.name}.`); } catch (err) { toast(err.message); } }
            if (e.target.closest("[data-delete-workspace]")) await deleteWorkspace(home);
            if (e.target.closest("[data-app-connect]")) { const a = app(e); await connectWorkspaceApp(ws, a.dataset.app, a.dataset.title); await again(); }
            if (e.target.closest("[data-app-disconnect]")) { const a = app(e); if (!(await ask(`Disconnect ${a.dataset.title}?`, "Disconnect"))) return; try { await Server.workspaces.appDisconnect(ws, a.dataset.app); toast("Disconnected."); } catch (err) { toast(err.body?.message || err.message); } await again(); }
          };
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

  return { boxes, dockItems, view };
})();

if (typeof PANEL_TITLES !== "undefined") PANEL_TITLES.shared = "Shared project";
if (typeof PANELS !== "undefined") PANELS.shared = id => SharedCanvas.view(id);
