// Compact ambient Work Queue strip replacing noisy stacked informational banners.
// Surfaces live running count, waiting approvals count, and dismissible session
// updates in one calm horizontal pill bar.
"use strict";

const WorkQueue = (() => {
  /**
   * Build the compact Work Queue strip HTML.
   * @param {{ running: number, waiting: number, sessionUpdates: boolean }} state
   * @returns {string}
   */
  function html(state) {
    const running = Number(state?.running || 0);
    const waiting = Number(state?.waiting || 0);
    const updates = !!state?.sessionUpdates;
    if (!running && !waiting && !updates) return "";
    const pills = [];
    if (running) {
      pills.push(`<a class="wq-pill wq-run" href="#/tasks" data-open="tasks"><span class="wq-dot" aria-hidden="true"></span><span>${running} running</span></a>`);
    }
    if (waiting) {
      pills.push(`<a class="wq-pill wq-need" href="#/inbox"><span class="wq-dot" aria-hidden="true"></span><span>${waiting} needs you</span></a>`);
    }
    if (updates) {
      pills.push(`<div class="wq-pill wq-updates banner-row" role="status"><span>Updates since your last session. <a href="#/activity" data-open="activity">View Activity</a></span><button type="button" class="wq-dismiss" data-act="dismiss-updates" aria-label="Dismiss updates">Dismiss</button></div>`);
    }
    return `<div class="work-queue-strip" data-work-queue role="region" aria-label="Work queue">${pills.join("")}</div>`;
  }

  return { html };
})();
