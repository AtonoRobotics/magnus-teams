// Background polling & per-user local state helpers (extracted from app.js in
// Tier 4 Phase 4): seenKey/load/save, pollInbox, and pollVersions.
"use strict";

function seenKey(k) { return `magnus.${k}.${App.appliance}.${App.me?.id}`; }
function load(k, d) { try { return JSON.parse(localStorage.getItem(seenKey(k))) ?? d; } catch { return d; } }
function save(k, v) { try { localStorage.setItem(seenKey(k), JSON.stringify(v)); } catch { /* private window */ } }

async function pollInbox() {
  try {
    const items = await Resources.read("Needs you", "/v1/inbox", "items");
    if (!items) return;
    App.inbox = items;
    const pruned = typeof Notices !== "undefined" && typeof Notices.syncInbox === "function" ? Notices.syncInbox(items) : false;
    // Interrupt only for decisions a task is waiting on; the rest wait in
    // the inbox until the person looks (U3).
    const told = new Set(load("told", []));
    const wants = i => i.blocking ? App.prefs?.notify_blocking !== false : App.prefs?.notify_batched === true;
    // Each item is considered once, so turning a kind on later does not
    // bring back everything already waiting.
    const fresh = needsMe().filter(i => !told.has(i.id));
    for (const i of fresh.filter(wants)) Notices.show({ key: `inbox:${i.id}`, title: i.blocking ? "A task needs you" : "Something for you", body: i.summary, system: i.summary, href: "#/inbox", blocking: !!i.blocking });
    // Ideas waiting in Explore: only notify while open suggestions still wait.
    const hasOpenIdeas = ExploreIdeas.open(items).length > 0;
    const ideas = hasOpenIdeas ? items.filter(i => ExploreIdeas.isNotice(i) && i.status === "open" && !told.has(i.id)) : [];
    for (const i of ideas) Notices.show({ key: `inbox:${i.id}`, title: "Ideas in Explore", body: i.summary, system: i.summary, href: "#/explore" });
    fresh.push(...ideas);
    if (fresh.length) save("told", [...told, ...fresh.map(i => i.id)].slice(-500));
    if (pruned || fresh.length) {
      convHead(App.thread);
      const pop = $("#notice-center");
      if (pop && !pop.hidden && typeof Notices !== "undefined" && Notices.centerHtml) paint(pop, Notices.centerHtml());
    }
    sidebar();
  } catch (e) {
    // A dead session must land on sign-in once. Calling start() again
    // restores the same session and boots forever.
    if (e.status === 401 && App.me) endSession("Your session ended. Sign in again.");
  }
}

async function pollVersions() {
  try {
    const v = await Resources.read("Agent changes", "/v1/versions");
    if (!v) return;
    App.versions = v;
    const seen = load("version", null);
    if (seen === null) { save("version", v.latest_seq ?? 0); return; }
    const news = v.changes.filter(c => c.seq > seen);
    const told = load("version-told", 0);
    if (news.length && (v.latest_seq ?? 0) > told) {
      const said = news.map(describeChange).join("; ");
      if (App.prefs?.notify_agent_changes !== false) Notices.show({ key: `version:${v.latest_seq}`, title: "The agent changed", body: said, system: said, href: "" });
      save("version-told", v.latest_seq);
    }
  } catch { /* shown above the conversation */ }
}
