// Co-editing a shared project's file (decisions/2026-11-23-co-editing-shared-files.md).
// Sharing, a member picks "Share a copy", which stays theirs as before, or
// "Co-edit": the file is then the project's and changes only by versions.
// A change is made on the version this page last saw (its base); one that
// conflicts with what landed since is kept as the member's, and they see
// both versions of each paragraph, cell or field and decide. A drawing or
// other binary file is checked out to one editor at a time. An agent's
// edits wait with its result in its owner's Airlock (the room's lane card).
"use strict";

// The wire types, as the server defines them (crates/magnus/src/team/coedit):
/** @typedef {"mine"|"theirs"} CoeditKeep  merge::Keep */
/** @typedef {{ at: string, before?: ?string, after?: ?string }} CoeditEdit  merge::Edit */
/** @typedef {{ at: string, base?: ?string, mine?: ?string, theirs?: ?string }} CoeditConflict  merge::Conflict */
/** @typedef {{ person: string, run?: string }} CoeditAuthor  coedit::Author */
/** @typedef {{ id: string, name: string, base: number, author: CoeditAuthor, at: string, mission: ?string, lane: ?string, conflicts: CoeditConflict[], diff: CoeditEdit[] }} CoeditChange  coedit::Open */
/** @typedef {{ version: number, format: "text"|"sheet"|"records"|"binary", checked_out: ?{ person: string, until: string }, open_changes: number }} CoeditListed  coedit::listed */
/** @typedef {{ name: string, coedit?: ?CoeditListed }} CoeditFile */

const Coedit = (() => {
  const SHARE_AS = { copy: "Share a copy", coedit: "Co-edit" };

  /** The choice a member makes when sharing a new file. */
  function choice() {
    return `<select data-share-as aria-label="How to share">${Object.entries(SHARE_AS).map(([v, label]) => `<option value="${v}">${label}</option>`).join("")}</select>`;
  }

  /** Where an upload of `name` goes: a new version of a co-edited file, made
   * on the version this page saw; a new file shared to co-edit; or a copy.
   * @param {string} ws @param {string} name @param {?string} project @param {string} as @param {CoeditFile[]} files */
  function path(ws, name, project, as, files) {
    const listed = (files || []).find(f => f.name === name)?.coedit;
    const q = [listed ? `base=${listed.version}` : as === "coedit" ? "share=coedit" : "", project ? `project=${enc(project)}` : ""].filter(Boolean).join("&");
    return `/v1/workspaces/${enc(ws)}/files/${enc(name)}${q ? `?${q}` : ""}`;
  }

  /** Upload one file. A change that conflicted is kept as this member's: it
   * is opened for them to decide at once.
   * @param {string} ws @param {File} file @param {?string} project @param {string} as @param {CoeditFile[]} files */
  async function upload(ws, file, project, as, files) {
    try {
      await Magnus.putBytes(path(ws, file.name, project, as, files), new Uint8Array(await file.arrayBuffer()));
    } catch (err) {
      if (!(await resolve(ws, file.name))) toast(err.message);
    }
  }

  /** The file's line in a files list: its version, who has it checked out,
   * and what can be done.
   * @param {CoeditFile} f @param {string} me @param {boolean} edit */
  function badge(f, me, edit) {
    const c = f.coedit;
    if (!c) return "";
    const lease = c.checked_out;
    const held = lease ? `<span class="small faint">${lease.person === me ? "Checked out to you" : "Checked out"} until ${esc(new Date(lease.until).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }))}</span>` : "";
    const binary = c.format === "binary" && edit
      ? lease?.person === me ? `<button class="link" data-coedit-checkin>Check in</button>` : lease ? "" : `<button class="link" data-coedit-checkout>Check out</button>`
      : "";
    const open = c.open_changes ? `<button class="link" data-coedit-changes>${c.open_changes === 1 ? "1 change of yours waits" : `${c.open_changes} changes of yours wait`}</button>` : "";
    return `<span class="coedit-badge" title="Co-edited: the project's file, changed by versions">Co-edited · v${c.version}</span>${held}<button class="link" data-coedit-history>History</button>${binary}${open}`;
  }

  const shown = (/** @type {?string|undefined} */ s) => s == null || s === "" ? `<em class="faint">(none)</em>` : esc(s);

  /** A dialog: resolves with the button pressed (its `data-coedit-do`), or null.
   * @param {string} html @param {(d: HTMLDialogElement) => any} [read] */
  function dialog(html, read) {
    return new Promise(done => {
      const d = document.createElement("dialog");
      d.className = "ask coedit-dialog";
      d.innerHTML = html;
      d.addEventListener("click", e => {
        const t = /** @type {HTMLElement} */ (e.target);
        if (t === d) { d.close(""); return; }
        const b = /** @type {?HTMLElement} */ (t.closest("[data-coedit-do]"));
        if (b) { d.returnValue = b.dataset.coeditDo || ""; done({ what: d.returnValue, value: read ? read(d) : null }); d.close(d.returnValue); }
      });
      d.addEventListener("close", () => { d.remove(); done(null); });
      document.body.append(d);
      d.showModal();
    });
  }

  /** Both versions of each conflicting part, for the member to keep one.
   * @param {CoeditChange} c */
  function conflictsHtml(c) {
    return `<h3>${esc(c.name)}: decide what to keep</h3>
      <p class="small">It changed since version ${esc(c.base)}, the one you worked on. Nothing is lost: pick a version for each part.</p>
      ${c.conflicts.map((x, i) => `<fieldset class="coedit-conflict"><legend>${esc(x.at)}</legend>
        <label class="coedit-side"><input type="radio" name="keep-${i}" value="mine" data-at="${esc(x.at)}" checked> <span>Keep yours</span><div class="coedit-text">${shown(x.mine)}</div></label>
        <label class="coedit-side"><input type="radio" name="keep-${i}" value="theirs" data-at="${esc(x.at)}"> <span>Keep theirs</span><div class="coedit-text">${shown(x.theirs)}</div></label>
      </fieldset>`).join("")}
      <div class="row"><button data-coedit-do="back">Back</button><button class="danger" data-coedit-do="discard">Throw yours away</button><button class="primary" data-coedit-do="land">Save these</button></div>`;
  }

  /** @param {HTMLElement} d @returns {Record<string, CoeditKeep>} */
  function choices(d) {
    /** @type {Record<string, CoeditKeep>} */
    const out = {};
    for (const r of /** @type {NodeListOf<HTMLInputElement>} */ (d.querySelectorAll("input[type=radio]:checked"))) out[r.dataset.at || ""] = /** @type {CoeditKeep} */ (r.value);
    return out;
  }

  /** Decide one change: keep a version of each conflicting part, or throw it away.
   * @param {string} ws @param {CoeditChange} c @returns {Promise<boolean>} whether it was settled */
  async function decide(ws, c) {
    const r = /** @type {?{what: string, value: any}} */ (await dialog(conflictsHtml(c), choices));
    if (!r || r.what === "back") return false;
    try {
      if (r.what === "discard") await Server.workspaces.discardChange(ws, c.id);
      else { const v = await Server.workspaces.landChange(ws, c.id, r.value); toast(`Saved as version ${v.version}.`); }
      return true;
    } catch (err) {
      if (err?.status === 409 && err.body?.conflicts) return decide(ws, { ...c, conflicts: err.body.conflicts });
      toast(err.message);
      return false;
    }
  }

  /** Open this member's conflicted changes (to `name`, if given), one by one.
   * @param {string} ws @param {string} [name] @returns {Promise<boolean>} whether there were any */
  async function resolve(ws, name) {
    const open = /** @type {CoeditChange[]} */ ((await Server.workspaces.coeditChanges(ws).catch(() => ({ changes: [] }))).changes || []);
    const mine = open.filter(c => c.conflicts?.length && (!name || c.name === name));
    for (const c of mine) await decide(ws, c);
    return mine.length > 0;
  }

  /** A co-edited file's history, newest first, each version with its author;
   * reverting to one is one more version.
   * @param {string} ws @param {string} name @param {boolean} edit */
  async function history(ws, name, edit) {
    const h = await Server.workspaces.coedit(ws, name);
    const rows = (h.versions || []).map((/** @type {any} */ v) => `<li><span>v${esc(v.number)}</span><span class="small">${esc(v.author_name || v.author)}${v.by_agent ? "'s agent" : ""}</span><span class="small faint">${v.base ? `on v${esc(v.base)}` : "shared"} · ${esc(new Date(v.at).toLocaleString())}</span>${edit && v.number !== h.version ? `<button class="link" data-coedit-do="revert:${esc(v.number)}">Revert to this</button>` : ""}</li>`).join("");
    const lease = h.checked_out ? `<p class="small">Checked out to ${esc(h.checked_out.mine ? "you" : h.checked_out.name)} until ${esc(new Date(h.checked_out.until).toLocaleString())}: others read the last version.</p>` : "";
    const values = h.values ? `<p class="small">Its values now, formulas worked out:</p><pre class="coedit-text">${esc(h.values)}</pre>` : "";
    const r = /** @type {?{what: string}} */ (await dialog(`<h3>${esc(name)}</h3>${lease}<ul class="coedit-history">${rows}</ul>${values}<div class="row"><button class="primary" data-coedit-do="close">Close</button></div>`));
    const to = Number(r?.what?.startsWith("revert:") ? r.what.slice(7) : NaN);
    if (!to) return false;
    await Server.workspaces.revert(ws, name, to);
    toast(`Reverted to version ${to}, as a new version.`);
    return true;
  }

  /** A click in a files list on a co-edited file's controls. Whether it was one.
   * @param {Event} e @param {string} ws @param {boolean} edit @param {() => any} redraw */
  async function click(e, ws, edit, redraw) {
    const t = /** @type {HTMLElement} */ (e.target);
    const name = /** @type {?HTMLElement} */ (t.closest("[data-team-file]"))?.dataset.teamFile;
    if (!name || !t.closest("[data-coedit-checkout],[data-coedit-checkin],[data-coedit-history],[data-coedit-changes]")) return false;
    try {
      if (t.closest("[data-coedit-checkout]")) { await Server.workspaces.checkOut(ws, name); toast(`${name} is checked out to you. Others read the last version until you check it in.`); }
      if (t.closest("[data-coedit-checkin]")) await Server.workspaces.checkIn(ws, name);
      if (t.closest("[data-coedit-history]") && !(await history(ws, name, edit))) return true;
      if (t.closest("[data-coedit-changes]")) await resolve(ws, name);
    } catch (err) { toast(err.message); }
    await redraw();
    return true;
  }

  /** The edits an agent's lane result carries, on its owner's Airlock card.
   * @param {{ edits?: CoeditChange[] }} lane */
  function laneEdits(lane) {
    const edits = lane.edits || [];
    if (!edits.length) return "";
    return `<div class="coedit-lane"><div class="small">Its edits to shared files go with it:</div>${edits.map(c => `<details><summary class="small">${esc(c.name)}, on version ${esc(c.base)}${c.conflicts?.length ? ` · <span class="bad">conflicts with a later version</span>` : ""}</summary>${c.diff.map(d => `<div class="small coedit-diff"><b>${esc(d.at)}</b> ${d.after == null ? "removed" : esc(d.after)}</div>`).join("")}</details>`).join("")}</div>`;
  }

  return { SHARE_AS, choice, path, upload, badge, click, resolve, decide, history, laneEdits, conflictsHtml, choices };
})();
