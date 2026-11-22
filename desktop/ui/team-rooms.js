// Intuitive, extensible Team Rooms Work panel & slot registry
// (teams_redesign_spec.md Phase 3):
// - Explicit mount slots (`headerActions`, `stageSlot`, `laneRenderers`) so
//   video conferencing, screen share, and future tools plug in cleanly.
// - Plain-language Live Work panel with strict owner-only private review
//   ("Only you see this until you share" -> Share with Room).
// - Member capability list (advertised app names + UserAvatar, zero credentials).
"use strict";

const TeamRooms = (() => {
  /** @type {{ headerActions: Array<(ctx: any) => string>, stageSlot: Array<(ctx: any) => string>, laneRenderers: Record<string, (lane: any, ctx: any) => string> }} */
  const slots = {
    headerActions: [],
    stageSlot: [],
    laneRenderers: {},
  };

  /**
   * Register an extensible Room UI plugin into a named slot (`headerActions`,
   * `stageSlot`, or `lane:<kind>`).
   * @param {string} slot @param {(arg: any, ctx?: any) => string} render
   */
  function registerSlot(slot, render) {
    if (slot === "headerActions" || slot === "stageSlot") {
      slots[slot].push(render);
    } else if (slot.startsWith("lane:")) {
      slots.laneRenderers[slot.slice(5)] = render;
    }
  }

  /** @param {"headerActions"|"stageSlot"} slot @param {any} ctx */
  function renderSlot(slot, ctx) {
    return (slots[slot] || []).map(fn => fn(ctx) || "").join("");
  }

  /** @param {any} lane @param {{ ws: string, missionId: string, me: string, nameOf: (id: string) => string }} ctx */
  function laneHtml(lane, ctx) {
    const custom = slots.laneRenderers[lane.kind];
    if (custom) return custom(lane, ctx);
    const ownerName = lane.owner ? ctx.nameOf(lane.owner) : "Magnus · Web";
    const who = lane.kind === "open_web" ? "Magnus · Web" : lane.kind === "peer_agent" ? `${ownerName}'s Agent · Peer` : `${ownerName}'s Agent`;
    let badge = `<span class="chip">Working</span>`;
    if (lane.status === "released") badge = `<span class="chip ok">✓ Done</span>`;
    else if (lane.status === "waiting_release") badge = `<span class="chip warn">Needs ${esc(ownerName)}'s OK</span>`;
    else if (lane.status === "failed") badge = `<span class="chip bad">Failed</span>`;

    const isOwner = Boolean(lane.owner && lane.owner === ctx.me);
    let body = "";
    if (lane.status === "waiting_release" && isOwner && lane.private_draft) {
      const warn = lane.scrub_flagged ? `<div class="small bad">Sensitive data was flagged — review before sharing.</div>` : "";
      body = `<div class="room-private-card" data-airlock-lane="${esc(lane.id)}">
        <div class="small room-private-head">Only you see this until you share</div>
        ${warn}
        <p class="small room-private-draft">${esc(lane.private_draft)}</p>
        <div class="room-steer-row">
          <input type="text" data-lane-steer-input="${esc(lane.id)}" placeholder="Ask your agent to edit before sharing…" aria-label="Edit draft">
          <button type="button" data-lane-steer="${esc(lane.id)}" data-mission="${esc(ctx.missionId)}">Edit</button>
        </div>
        <div class="room-release-row">
          ${lane.scope ? `<label class="small"><input type="checkbox" data-lane-standing="${esc(lane.id)}"> Always share ${esc(lane.scope)}</label>` : `<span></span>`}
          <button type="button" class="primary" data-lane-release="${esc(lane.id)}" data-mission="${esc(ctx.missionId)}">Share with Room</button>
        </div>
      </div>`;
    } else if (lane.status === "waiting_release") {
      body = `<p class="small faint">${esc(ownerName)}'s agent finished privately. Waiting for ${esc(ownerName)} to share.</p>`;
    } else if (lane.status === "released" && lane.contribution) {
      body = `<p class="small room-contrib">${esc(lane.contribution)}</p>`;
    } else if (lane.status === "failed" && lane.failure_reason) {
      body = `<p class="small bad">${esc(lane.failure_reason)}</p>`;
    }

    return `<div class="room-lane${lane.status === "waiting_release" ? " is-waiting" : ""}" data-lane="${esc(lane.id)}">
      <div class="spread"><div><strong class="small">${esc(who)}</strong><div class="small faint">${esc(lane.objective)}</div></div>${badge}</div>
      ${body}
    </div>`;
  }

  /**
   * Renders the Live Work panel for active missions in a Room.
   * @param {any[]} missions @param {string} ws @param {string} me @param {(id: string) => string} [nameOf]
   */
  function workPanelHtml(missions, ws, me, nameOf = id => id) {
    if (!missions || !missions.length) return "";
    return `<section class="room-work" aria-label="Active room work">
      ${missions.map(m => {
        const ctx = { ws, missionId: m.id, me, nameOf };
        const badge = m.complete
          ? `<span class="chip ok">✓ Ready</span>`
          : `<span class="chip warn">${m.released_count}/${m.total_lanes} updates</span>`;
        const deliv = m.deliverable
          ? `<div class="room-deliverable"><div class="small faint">Shared output</div><div class="small room-deliverable-body">${esc(m.deliverable)}</div></div>`
          : "";
        return `<article class="room-mission" data-mission-id="${esc(m.id)}">
          <div class="spread"><strong>${esc(m.goal)}</strong>${badge}</div>
          ${deliv}
          <div class="room-lanes">${(m.lanes || []).map((/** @type {any} */ l) => laneHtml(l, ctx)).join("")}</div>
        </article>`;
      }).join("")}
    </section>`;
  }

  /**
   * Renders the People & Capabilities list using UserAvatar (names + advertised
   * personal apps only — never credentials or private content).
   * @param {Array<{ id: string, name: string, avatar?: any, apps?: Array<{ name: string, title: string }> }>} members
   * @param {string} me
   */
  function peopleCapabilitiesHtml(members, me) {
    if (!members || !members.length) return "";
    const av = (name, avatar) => (typeof UserAvatar !== "undefined" ? UserAvatar.html(name, avatar, "sm") : "");
    return `<ul class="room-cap-list">${members.map(m => {
      const apps = (m.apps || []).map(a => a.title || a.name).join(" · ") || "No apps shared";
      const you = m.id === me ? ` <span class="small faint">you</span>` : "";
      return `<li class="room-cap-row" data-cap-member="${esc(m.id)}">${av(m.name, m.avatar)}<div class="room-cap-meta"><div class="small"><strong>${esc(m.name)}</strong>${you}</div><div class="small faint">${esc(apps)}</div></div></li>`;
    }).join("")}</ul>`;
  }

  /**
   * Wires click handlers for Share with Room (`data-lane-release`) and private
   * Edit (`data-lane-steer`) inside `container`.
   * @param {HTMLElement} container @param {string} ws @param {() => void} onChange
   */
  function wire(container, ws, onChange) {
    container.addEventListener("click", async e => {
      const t = /** @type {HTMLElement} */ (e.target);
      const rel = t.closest("[data-lane-release]");
      if (rel) {
        const laneId = /** @type {HTMLElement} */ (rel).dataset.laneRelease || "";
        const missionId = /** @type {HTMLElement} */ (rel).dataset.mission || "";
        const chk = /** @type {HTMLInputElement | null} */ (container.querySelector(`[data-lane-standing="${laneId}"]`));
        if (typeof Server !== "undefined" && Server.workspaces?.releaseLane) {
          await Server.workspaces.releaseLane(ws, missionId, laneId, { save_standing_rule: Boolean(chk?.checked) });
          onChange();
        }
        return;
      }
      const st = t.closest("[data-lane-steer]");
      if (st) {
        const laneId = /** @type {HTMLElement} */ (st).dataset.laneSteer || "";
        const missionId = /** @type {HTMLElement} */ (st).dataset.mission || "";
        const inp = /** @type {HTMLInputElement | null} */ (container.querySelector(`[data-lane-steer-input="${laneId}"]`));
        const text = (inp?.value || "").trim();
        if (text && typeof Server !== "undefined" && Server.workspaces?.steerLane) {
          await Server.workspaces.steerLane(ws, missionId, laneId, text);
          if (inp) inp.value = "";
          onChange();
        }
      }
    });
  }

  return { registerSlot, renderSlot, workPanelHtml, peopleCapabilitiesHtml, wire };
})();
