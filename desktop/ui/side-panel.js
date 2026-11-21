// The one right-side panel (side-panel.js) for both the personal chat and a
// team workspace or channel, matching how chat-view.js unifies the center
// conversation. The personal scope (several: false) and the team scope
// (several: true) pass their items and tabs into these shared building blocks
// so the header, segmented tab bar, waiting/approval banner, Active now card,
// file list and computer footer bar are written once.
"use strict";

const SidePanel = (() => {
  /**
   * A brand logo for a row's text when one is known.
   * @param {string} text @param {string} [cls]
   */
  function brand(text, cls = "gbox-ic has-logo") {
    const b = typeof AppLogos !== "undefined" && AppLogos.forText ? AppLogos.forText(text) : null;
    return b ? `<span class="${cls}" aria-hidden="true">${b.svg}</span>` : "";
  }

  /**
   * The single-row header (.companion-id): avatar with status dot, title,
   * status pill, subtitle counters, and optional top-right actions (kill
   * switch and close button in personal scope).
   * @param {{ letter?: string, avatarHref?: string, avatarTitle?: string, dot: string, tone?: string, label: string, titleHtml: string, subText: string, topActs?: boolean }} h
   */
  function head(h) {
    const letter = esc((h.letter || "M").slice(0, 1).toUpperCase());
    const tone = h.tone || h.dot;
    const avatarInner = `<span class="companion-m">${letter}</span><span class="comp-dot ${h.dot}" aria-hidden="true"></span>`;
    const avatar = h.avatarHref
      ? `<a href="${h.avatarHref}" class="companion-avatar" aria-label="${esc(h.avatarTitle || "Customize theme and accent")}" title="${esc(h.avatarTitle || "Theme & accent")}">${avatarInner}</a>`
      : `<span class="companion-avatar" aria-hidden="true">${avatarInner}</span>`;
    const acts = h.topActs
      ? `<div class="companion-top-acts"><div class="admin-kill" data-admin-kill role="group" aria-label="Kill switch" hidden></div><button type="button" class="icon-btn close" data-act="close-panel" aria-label="Close panel" title="Close panel">${icon("close")}</button></div>`
      : "";
    return `<div class="companion-id"><div class="companion-row-left">${avatar}<div class="companion-title-col"><div class="companion-name-row"><h2>${h.titleHtml}</h2><span class="companion-status-lbl ${tone}">${esc(h.label)}</span></div><div class="companion-sub"><span class="muted">${esc(h.subText)}</span></div></div></div>${acts}</div>`;
  }

  /**
   * The segmented 4-tab bar (.panel-tabs): renders links in personal scope
   * and buttons in team scope.
   * @param {Array<[string, string, { href?: string, open?: string, badge?: number }?]>} list
   * @param {string} current
   * @param {{ label?: string, buttonAttr?: string }} [opts]
   */
  function tabs(list, current, opts = {}) {
    const aria = esc(opts.label || "Inspector");
    const attr = opts.buttonAttr;
    const items = list.map(([k, l, meta = {}]) => {
      const cur = k === current ? ' aria-current="page"' : "";
      const badge = meta.badge ? ` <span class="badge">${meta.badge}</span>` : "";
      if (attr) return `<button type="button" ${attr}="${esc(k)}"${cur}><span>${esc(l)}</span>${badge}</button>`;
      const href = meta.href || `#/${k}`;
      const open = meta.open || k;
      return `<a href="${href}" data-open="${esc(open)}"${cur}><span>${esc(l)}</span>${badge}</a>`;
    }).join("");
    return `<nav class="panel-tabs" aria-label="${aria}">${items}</nav>`;
  }

  /**
   * The top banner for work waiting on a person (.companion-needs-banner):
   * - In personal scope (several: false), shows "Needs your approval (N)"
   *   with inline Deny / Approve buttons and "View all →", or the cleared bar.
   * - In team scope (several: true), shows "Waiting on someone (N)" naming
   *   who the agent is waiting for and the channel, without leaking private
   *   approval payloads to other channel members.
   * @param {{ several?: boolean, count: number, viewAllHref?: string, viewAllOpen?: string, clearedHref?: string, clearedOpen?: string, rowsHtml: string }} b
   */
  function waitingBanner(b) {
    if (!b.count) {
      if (b.several) return "";
      return `<section class="companion-sec companion-cleared-banner"><span class="cleared-lbl">✓ All approvals cleared</span><a class="cleared-link" href="${b.clearedHref || "#/inbox"}" data-open="${b.clearedOpen || "needs"}">Approvals</a></section>`;
    }
    const title = b.several ? `Waiting on someone (${b.count})` : `Needs your approval (${b.count})`;
    const more = b.viewAllHref ? `<a class="needs-view-all" href="${b.viewAllHref}"${b.viewAllOpen ? ` data-open="${b.viewAllOpen}"` : ""}>View all →</a>` : "";
    const wrapCls = b.several ? "gbox" : "needs-inline-list";
    return `<section class="companion-sec companion-needs-banner"><div class="companion-sec-head"><span class="needs-banner-lbl"><span class="comp-dot warn" aria-hidden="true"></span>${esc(title)}</span>${more}</div><div class="${wrapCls}">${b.rowsHtml}</div></section>`;
  }

  /**
   * The "Active now" card (.companion-active-card) for both personal tasks
   * and team channel runs.
   * @param {{ href: string, open?: string, rightMeta: string, title: string, pct?: ?number, footHtml: string, inspectLabel?: string }} c
   */
  function activeCard(c) {
    const progress = typeof c.pct === "number"
      ? `<div class="active-progress" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${c.pct}"><span style="width:${c.pct}%"></span></div>`
      : "";
    return `<section class="companion-sec"><a class="gbox companion-active-card" href="${c.href}"${c.open ? ` data-open="${esc(c.open)}"` : ""}><div class="active-card-top"><span class="active-card-kicker"><span class="comp-dot run" aria-hidden="true"></span>Active now</span><span class="mono active-elapsed">${esc(c.rightMeta)}</span></div><div class="active-card-title">${esc(c.title)}</div>${progress}<div class="active-card-foot">${c.footHtml}<span class="small active-inspect">${esc(c.inspectLabel || "Inspect →")}</span></div></a></section>`;
  }

  /**
   * The computer status bar at the panel's foot (.companion-computer),
   * shared by Cloud Computer (personal) and Team Computer (workspace).
   * @param {{ id?: ?string, kindLabel: string, stateText: string, awake?: boolean, stateAttr?: boolean, pauseId?: ?string, watchHref?: ?string, watchOpen?: ?string, wakeId?: ?string, showSetupWhenMissing?: boolean }} f
   */
  function computerFoot(f) {
    if (!f.id) {
      if (!f.showSetupWhenMissing) return "";
      return `<div class="companion-computer"><div class="comp-head"><span class="comp-dot idle" aria-hidden="true"></span><span class="muted">${esc(f.kindLabel)} ·</span><strong>Being set up</strong></div></div>`;
    }
    const dotCls = f.awake ? "ok" : "idle";
    const dotAttr = f.stateAttr ? ` data-team-comp-dot="${esc(f.id)}"` : "";
    const stAttr = f.stateAttr ? ` data-team-comp-state="${esc(f.id)}"` : "";
    const pauseBtn = f.pauseId ? `<button type="button" class="danger" data-pause-computer="${esc(f.pauseId)}" title="Pause computer and revoke accesses">Pause</button>` : "";
    const actBtn = f.watchHref
      ? `<a class="link comp-wake-btn" href="${f.watchHref}">Watch →</a>`
      : f.watchOpen
        ? `<button type="button" class="link comp-wake-btn" data-open="${esc(f.watchOpen)}">Watch →</button>`
        : f.wakeId
          ? `<button type="button" class="link comp-wake-btn" data-wake-computer="${esc(f.wakeId)}">Wake VM →</button>`
          : "";
    return `<div class="companion-computer" data-computer-bar="${esc(f.id)}"><div class="comp-head"><span class="comp-dot ${dotCls}" aria-hidden="true"${dotAttr}></span><span class="muted">${esc(f.kindLabel)} ·</span><strong${stAttr}>${esc(f.stateText)}</strong></div><div class="row comp-acts">${pauseBtn}${actBtn}</div></div>`;
  }

  /**
   * The complete right panel frame (.detail-head + .detail-body + computerFoot).
   * @param {{ headHtml: string, panelKey: string, bodyHtml: string, footHtml?: string, wrapClass?: string }} s
   */
  function shell(s) {
    const inner = `<div class="detail-head"><div class="dh-main">${s.headHtml}</div></div><div class="detail-body" data-panel="${esc(s.panelKey)}">${s.bodyHtml}</div>${s.footHtml || ""}`;
    return s.wrapClass ? `<div class="${s.wrapClass}">${inner}</div>` : inner;
  }

  return { brand, head, tabs, waitingBanner, activeCard, computerFoot, shell };
})();
if (typeof module !== "undefined") module.exports = SidePanel;
