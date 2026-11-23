// Pure peer collaboration settings: any collaborator can add people on the
// server (1-click chips or typing a name) or invite by email / copy an invite
// link. One merged roster shows each collaborator with their advertised
// personal apps — no workspace Admin role bureaucracy, no shared Team Agent,
// no shared workspace apps, and no shared spend. Only the creator can delete
// the shared project; anyone else can leave from the Danger Zone at the bottom.
"use strict";

/** @param {string} r */
const teamRank = r => TEAM_ROLES.indexOf(r);

/** Whether the viewer is the creator (or legacy admin) of the shared project. @param {TeamHome} home */
const isTeamAdmin = home => teamRank(home.role) >= teamRank("admin");

/** Whether the viewer is a peer collaborator (any non-guest member). @param {TeamHome} home */
const canCollaborate = home => home.role !== "guest";

/** Label for opening the People & settings view. @param {TeamHome} home */
const peopleLabel = home => canCollaborate(home) ? "People & invites" : "People";

/**
 * Pure peer Project Settings: two clean cards — People (unified add/invite +
 * 1-click suggestion chips + merged peer roster with advertised personal apps)
 * and Danger Zone at the bottom.
 * @param {TeamHome} home
 * @param {{ id: string, name: string }[]} candidates
 * @param {Array<{ id: string, name?: string, avatar?: any, apps?: Array<{ name: string, title?: string }> }>} [caps]
 */
function teamPeopleHtml(home, candidates, caps = []) {
  const creator = isTeamAdmin(home), peer = canCollaborate(home);
  const owners = home.members.filter(m => m.role === "owner").length;
  const candList = Array.isArray(candidates) ? candidates : [];
  const capById = new Map((Array.isArray(caps) ? caps : []).map(c => [c.id, c]));
  const lastOwner = home.role === "owner" && owners <= 1;

  const chips = candList.slice(0, 8).map(p =>
    `<button type="button" class="people-chip" data-quick-add="${esc(p.id)}" data-quick-name="${esc(p.name)}" title="Add ${esc(p.name)} to ${esc(home.name)}">+ ${esc(p.name)}</button>`
  ).join("");

  const addBlock = peer ? `<div class="people-add-wrap" data-people-add>
    <form class="people-add-form" data-people-invite>
      <input type="text" data-people-input placeholder="Name or email…" aria-label="Add collaborator by name or email" autocomplete="off">
      <button type="submit" class="primary">Add</button>
    </form>
    ${candList.length ? `<div class="people-chips" data-people-suggestions>${chips}</div>` : ""}
    <p class="small faint people-status" data-invite-result aria-live="polite"></p>
  </div>` : `<p class="small faint">Guests can view who is in this shared project.</p>`;

  const row = m => {
    const me = m.id === home.me;
    const cap = capById.get(m.id);
    const appNames = (cap?.apps || []).map(a => a.title || a.name).filter(Boolean);
    const sub = appNames.length ? appNames.join(" · ") : "Personal Magnus";
    const badge = m.role === "owner" ? `<span class="chip">Creator</span>` : m.role === "guest" ? `<span class="chip">Guest</span>` : "";
    const canRemove = creator && !me && m.role !== "owner";
    const action = canRemove ? `<button type="button" class="link" data-remove>Remove</button>` : "";
    return `<div class="gbox-row people-row" data-member="${esc(m.id)}" data-name="${esc(m.name)}">
      <span class="team-face${m.online ? " on" : ""}">${avatar(m.name, false, m.id)}</span>
      <span class="gbox-main"><span class="gbox-lbl">${esc(m.name)}${me ? " (you)" : ""} ${badge}</span><span class="small muted">${esc(sub)}</span></span>
      ${action}
    </div>`;
  };

  const leaveBtn = !lastOwner
    ? `<div class="gbox-row people-danger-row"><span class="gbox-main"><span class="gbox-lbl">Leave ${esc(home.name)}</span><span class="small muted">You lose access to this shared project. Your personal conversations and files stay untouched.</span></span><button type="button" data-leave>Leave</button></div>`
    : "";
  const deleteBtn = creator
    ? `<div class="gbox-row people-danger-row"><span class="gbox-main"><span class="gbox-lbl">Delete ${esc(home.name)}</span><span class="small muted">Deletes shared conversations, tasks, files, and knowledge for everyone. Personal conversations and computers are never touched.</span></span><button type="button" class="danger" data-delete-workspace>Delete project</button></div>`
    : "";
  const dangerSec = (leaveBtn || deleteBtn)
    ? `<section class="companion-sec people-danger"><div class="companion-sec-head"><span>Danger zone</span></div><div class="gbox">${leaveBtn}${deleteBtn}</div></section>`
    : "";

  return `<section class="companion-sec people-card">
    <div class="companion-sec-head people-head"><span>People <span class="count">${home.members.length}</span></span>${peer ? `<button type="button" class="link" data-invite-link>Copy invite link</button>` : ""}</div>
    ${addBlock}
    <div class="gbox people-roster" data-peer-roster>${home.members.map(row).join("")}</div>
  </section>${dangerSec}`;
}

/**
 * Wires the unified Project Settings UI (live candidate filtering, 1-click chip
 * add, name/email(s) submit, 1-click Copy invite link, Remove, Leave, Delete)
 * once per host element so repeated redraws never double-fire requests.
 * @param {HTMLElement} root @param {string} ws @param {TeamHome} home
 * @param {{ id: string, name: string }[]} candidates @param {() => Promise<void>} onReload
 */
function wirePeopleSettings(root, ws, home, candidates, onReload) {
  root._peopleCtx = { ws, home, candidates: candidates || [], onReload };
  const input = /** @type {?HTMLInputElement} */ ($("[data-people-input]", root));
  if (input && typeof document !== "undefined" && document.activeElement !== input) {
    input.focus({ preventScroll: true });
  }
  if (root.dataset.peopleBound) return;
  root.dataset.peopleBound = "1";

  const renderChips = q => {
    const box = $("[data-people-suggestions]", root);
    const ctx = root._peopleCtx;
    if (!box || !ctx) return;
    const low = String(q || "").trim().toLowerCase();
    const list = (ctx.candidates || []).filter(p => !low || p.name.toLowerCase().includes(low) || p.id.toLowerCase().includes(low)).slice(0, 8);
    paint(box, list.map(p => `<button type="button" class="people-chip" data-quick-add="${esc(p.id)}" data-quick-name="${esc(p.name)}" title="Add ${esc(p.name)} to ${esc(ctx.home.name)}">+ ${esc(p.name)}</button>`).join(""));
  };

  root.addEventListener("input", e => {
    if (e.target.matches?.("[data-people-input]")) renderChips(e.target.value);
  });

  root.addEventListener("submit", async e => {
    const form = e.target.closest?.("[data-people-invite]");
    if (!form) return;
    e.preventDefault();
    const ctx = root._peopleCtx;
    if (!ctx) return;
    const field = $("[data-people-input]", form);
    const resultEl = $("[data-invite-result]", root);
    const raw = (field?.value || "").trim();
    if (!raw) return;
    const parts = raw.split(/[\s,;]+/).map(s => s.trim()).filter(Boolean);
    const emails = parts.filter(p => p.includes("@"));
    try {
      if (emails.length) {
        const notes = [];
        for (const email of emails) {
          const r = await Server.workspaces.invite(ctx.ws, email);
          notes.push(inviteOutcome(email, r));
        }
        if (field) field.value = "";
        await ctx.onReload();
        const afterRes = $("[data-invite-result]", root);
        if (afterRes) afterRes.textContent = notes.join(" ");
        else if (resultEl) resultEl.textContent = notes.join(" ");
        return;
      }
      const low = raw.toLowerCase();
      const match = (ctx.candidates || []).find(p => p.id.toLowerCase() === low || p.name.toLowerCase() === low)
        || (ctx.candidates || []).find(p => p.name.toLowerCase().startsWith(low));
      if (match) {
        await Server.workspaces.setMember(ctx.ws, match.id, "member");
        toast(`Added ${match.name}.`);
        if (field) field.value = "";
        await ctx.onReload();
      } else {
        toast("Pick a teammate below or enter an email address to invite them.");
      }
    } catch (err) {
      toast(err.message);
    }
  });

  root.addEventListener("click", async e => {
    const ctx = root._peopleCtx;
    if (!ctx) return;
    const chip = e.target.closest?.("[data-quick-add]");
    if (chip) {
      const id = chip.dataset.quickAdd, name = chip.dataset.quickName || id;
      try {
        await Server.workspaces.setMember(ctx.ws, id, "member");
        toast(`Added ${name}.`);
        await ctx.onReload();
      } catch (err) { toast(err.message); }
      return;
    }
    if (e.target.closest?.("[data-invite-link]")) {
      try {
        const r = await Server.workspaces.invite(ctx.ws);
        const link = serverLink(r);
        try { await navigator.clipboard.writeText(link); toast("Invite link copied. It works for a week."); } catch { /* clipboard unavailable */ }
        const res = $("[data-invite-result]", root);
        if (res) res.textContent = `Invite link (valid 7 days): ${link}`;
      } catch (err) { toast(err.message); }
      return;
    }
    if (e.target.closest?.("[data-remove]")) {
      const m = e.target.closest("[data-member]");
      if (!m || !(await ask(`Remove ${m.dataset.name} from ${ctx.home.name}? They lose access at once. What they shared stays.`, "Remove"))) return;
      try { await Server.workspaces.removeMember(ctx.ws, m.dataset.member); await ctx.onReload(); } catch (err) { toast(err.message); }
      return;
    }
    if (e.target.closest?.("[data-leave]")) {
      if (!(await ask(`Leave ${ctx.home.name}? You lose access to its shared conversations and files. Your personal space is untouched.`, "Leave"))) return;
      try { await Server.workspaces.removeMember(ctx.ws, ctx.home.me); leftWorkspaceQuietly(ctx.ws); toast(`You left ${ctx.home.name}.`); } catch (err) { toast(err.message); }
      return;
    }
    if (e.target.closest?.("[data-delete-workspace]")) {
      await deleteWorkspace(ctx.home);
    }
  });
}

/** Legacy full-page fallback for #/team/:ws/people before SharedCanvas loads. */
async function teamPeople(center, side, ws) {
  const home = Team.home, peer = canCollaborate(home);
  const [candidates, caps] = await Promise.all([
    peer ? Server.workspaces.people(ws).catch(() => []) : Promise.resolve([]),
    Server.workspaces.capabilities ? Server.workspaces.capabilities(ws).catch(() => []) : Promise.resolve([]),
  ]);
  paint(center, `<header class="team-head"><h1>People</h1></header><div class="team-scroll">${teamPeopleHtml(home, candidates, caps)}</div>`);
  if (side?.isConnected) TeamPanel.side(side, ws, null, null);
  const again = async () => { await loadHome(ws); return teamPeople(center, side, ws); };
  wirePeopleSettings(center, ws, home, candidates, again);
}

/** Forget a shared project the person is no longer in, and return to the list. */
function leftWorkspaceQuietly(ws) {
  delete Team.homes[ws];
  if (Team.ws === ws) { Team.ws = null; Team.home = null; }
  location.hash = "#/teams";
}

/** Delete it, once its name is typed: only its creator may. */
async function deleteWorkspace(home) {
  if (!(await askTyped(`Delete ${home.name} for everyone? Its shared conversations, tasks, files, and knowledge go, and this cannot be undone.`, home.name, "Delete project"))) return;
  try {
    await Server.workspaces.remove(home.id);
    leftWorkspaceQuietly(home.id);
    toast(`${home.name} was deleted.`);
  } catch (err) { if (!err.cancelled) toast(err.body?.message || err.message); }
}

/** A confirmation for what cannot be undone: the button works once
 * `expected` is typed. Resolves whether the person went ahead. */
function askTyped(text, expected, label) {
  return new Promise(done => {
    const d = document.createElement("dialog");
    d.className = "ask"; d.setAttribute("aria-label", label);
    d.innerHTML = `<p>${esc(text)}</p><label class="stack small">Type “${esc(expected)}” to confirm<input data-typed autocomplete="off" spellcheck="false"></label>
      <div class="row"><button data-ask="no">Cancel</button><button class="danger" data-ask="yes" disabled>${esc(label)}</button></div>`;
    const input = /** @type {HTMLInputElement} */ ($("[data-typed]", d)), go = /** @type {HTMLButtonElement} */ ($("[data-ask=yes]", d));
    const ready = () => input.value.trim() === expected.trim();
    input.addEventListener("input", () => { go.disabled = !ready(); });
    input.addEventListener("keydown", e => { if (e.key === "Enter" && ready()) d.close("yes"); });
    d.addEventListener("click", e => { const b = e.target.closest("[data-ask]"); if (b && !b.disabled) d.close(b.dataset.ask); });
    d.addEventListener("close", () => { d.remove(); done(d.returnValue === "yes"); });
    document.body.append(d);
    d.showModal();
    input.focus();
  });
}
