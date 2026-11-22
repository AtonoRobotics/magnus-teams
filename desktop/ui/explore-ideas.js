// Magnus's own ideas, in the For you section of Explore
// (decisions/2026-11-21-ideas-go-to-explore.md): what an opener (a first
// sign-in, a newly connected app) or a review suggested it could take over.
// They open no conversation, wait in no Approvals list and make no box:
// each is started or dismissed from its card here, with app.js's card()
// and its one click handler. The person is told once that ideas are here,
// by the server's notice (reason "ideas"), which links here.
"use strict";

const ExploreIdeas = (() => {
  /** A notice's reason when ideas are waiting in Explore (openers.rs). */
  const REASON = "ideas";

  /** Whether an inbox item is one of Magnus's ideas. @param {any} i */
  const isIdea = i => i?.kind === "suggestion";

  /** Whether an inbox item is the notice that ideas are waiting. @param {any} i */
  const isNotice = i => i?.kind === "notice" && i.details?.reason === REASON;

  /** The ideas still waiting, newest first. @param {any[]} inbox */
  const open = inbox => (inbox || []).filter(i => isIdea(i) && i.status === "open").sort((a, b) => String(b.created_at || "").localeCompare(String(a.created_at || "")));

  /**
   * The For you section, or nothing when no idea waits.
   * @param {any[]} inbox @param {(i: any) => string} cardOf how an item's card is drawn
   */
  function html(inbox, cardOf) {
    const list = open(inbox);
    if (!list.length) return "";
    return `<section class="explore-ideas" aria-labelledby="explore-ideas-head">
      <div class="spread"><h2 id="explore-ideas-head">For you <span class="muted small">(${list.length})</span></h2></div>
      <p class="small muted">What Magnus could take over, from what it saw in your apps and work. Nothing starts until you say so.</p>
      <div class="explore-ideas-list">${list.map(cardOf).join("")}</div>
    </section>`;
  }

  return { REASON, isIdea, isNotice, open, html };
})();
