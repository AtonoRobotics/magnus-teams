// Regression checks for Aurora stability, page close, Explore live patching,
// bento flicker prevention, ghost/drawer cleanup, complete rail navigation,
// container-responsive conversation header, and update flow
// (decisions/2026-11-22-aurora-stability-navigation-and-updates.md).
import assert from "node:assert/strict";
import { readAppBundle } from "./app-bundle.mjs";
import { existsSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const read = (/** @type {string} */ rel) => readFileSync(join(here, "..", rel), "utf8");

// 1. Retired aurora-pages.js is removed (Nova uses the rail; Classic has no stage).
assert.equal(existsSync(join(here, "../ui/aurora-pages.js")), false, "retired aurora-pages.js must be deleted");

// 2. Discover.show does not wipe .explore-page on background App.redraw() calls.
{
  const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
  let fullPaints = 0;
  let ideasHtml = "";
  let gridHtml = "";
  const gridNode = {
    set outerHTML(v) { gridHtml = v; },
    get outerHTML() { return gridHtml; },
  };
  const ideasNode = { innerHTML: "", _html: "" };
  const main = {
    innerHTML: "",
    _html: "",
    onclick: null,
    oninput: null,
    querySelector(sel) {
      if (sel === ".explore-page") return main.innerHTML ? {} : null;
      if (sel === ".explore-grid") return gridNode;
      if (sel === "[data-explore-ideas]") return ideasNode;
      return null;
    },
    querySelectorAll() { return []; },
  };
  const ctx = {
    esc,
    enc: encodeURIComponent,
    icon: k => `<svg data-icon="${k}"></svg>`,
    paint: (el, html) => {
      if (el === main) fullPaints++;
      if (el._html === html) return;
      el._html = html;
      el.innerHTML = html;
    },
    App: { me: { name: "Alex" }, seq: 1, inbox: [], schedules: [], redraw: null },
    ExploreIdeas: { html: () => ideasHtml },
    card: () => "",
    onLeave: () => {},
    pollInbox: async () => {},
    Resources: { read: async () => [{ name: "google-gmail", title: "Gmail", state: "connected", for_you: true }] },
    location: { hash: "#/explore" },
    openDiscoveryWizard: async () => {},
    openSetupWizard: async () => {},
  };
  ctx.globalThis = ctx;
  vm.createContext(ctx);
  vm.runInContext(`${read("ui/discover.js")}\nglobalThis.Discover = Discover;`, ctx);
  await ctx.Discover.show(main);
  assert.equal(fullPaints, 1, "initial draw paints #main once");
  await main.onclick({ target: { closest: sel => (sel === "[data-exp-cat]" ? { dataset: { expCat: "eng" } } : null) } });
  await ctx.App.redraw();
  await ctx.App.redraw();
  assert.equal(fullPaints, 1, "background App.redraw() after filter click must not full-repaint #main");
  ideasHtml = "<section class='explore-ideas'>New idea</section>";
  await ctx.App.redraw();
  assert.equal(fullPaints, 1, "updating For you ideas patches only [data-explore-ideas]");
  assert.equal(ideasNode.innerHTML, ideasHtml);
}

// 3. Aurora entrance, ghost, stage-flicker, drawer, rail, header, and update guards stay wired.
const aurora = read("ui/aurora.js");
const stage = read("ui/aurora-stage.js");
const exploreAnim = read("ui/aurora-explore.js");
const rail = read("ui/aurora-rail.js");
const app = readAppBundle();
const admin = read("ui/admin.js");
const stageCss = read("ui/nova-stage.css");
const updateRs = read("src-tauri/src/update.rs");

assert.match(aurora, /querySelector\("#main > \*:not\(section\.conv\):not\(\.aurora-page-bar\)"\)/, "aurora.js ignores .aurora-page-bar in viewKey");
assert.match(aurora, /clearDetailGhosts\(\)/, "aurora.js clears detail ghosts on route change");
assert.match(stage, /a\.id === "stage-slide"\) a\.cancel\(\)/, "aurora-stage.js cancels prior slide animation before measuring");
assert.match(stage, /lastBentoWidth/, "aurora-stage.js ResizeObserver only slides on width changes");
assert.match(exploreAnim, /EXPLORE_LEAVE_FALLBACK_MS/, "aurora-explore.js has a fallback cleanup timeout");
assert.match(rail, /addEventListener\("hashchange"/, "aurora-rail.js closes navigation drawer on hashchange");
assert.doesNotMatch(app, /class="nav-teams primary-nav/, "left rail has no separate Teams icon");
assert.match(app, /data-nav-del-shared/, "sidebar merges shared projects into Projects");
assert.doesNotMatch(app, /aria-label="People"/, "the sidebar has no People section: people are in a shared project's page (decisions/2026-11-28-chat-first-navigation.md)");
assert.match(app, /"#\/library","nav-apps"/, "the sidebar's foot row includes Apps");
assert.match(app, /if \(App\.serverVersion && App\.serverVersion !== v\.version\) refreshUpdate\(true\)/, "server version change triggers desktop update check");
assert.match(app, /let restartingUpdate = false;/, "restartToUpdate guards against concurrent clicks");
assert.match(admin, /submit\.disabled = true/, "stepUp disables Confirm button while in flight");
assert.match(stageCss, /@container aurora-main \(min-width: 820px\)/, "nova-stage.css adapts to container width");
// The restore takes the lock however a poisoned lock is handled (7b1847e
// replaced `.unwrap()` with `.unwrap_or_else(PoisonError::into_inner)`).
const installFailure = updateRs.slice(updateRs.indexOf("fn update_install"));
assert.match(installFailure, /\*ups\.ready\.lock\(\)\.unwrap(?:\(\)|_or_else\(std::sync::PoisonError::into_inner\)) = Some\(\(update, bytes\)\);/, "update_install restores ready update on install failure");

// 4. Send-to-Stop square, Steer, close deduplication, sizing harmony, and autonomy UX.
const composerJs = read("ui/chat-composer.js");
const personalJs = read("ui/personal-chat.js");
const teamConvJs = read("ui/team-conversation.js");
const asksJs = read("ui/asks.js");
const chatCss = read("ui/chat.css");
const novaCss = read("ui/nova.css");
const pagesCss = read("ui/pages.css");

assert.match(app, /stop:\s*'<rect /, "app.js defines UI.stop so Stop square SVG always renders");
assert.match(app, /mic:\s*'<path /, "app.js defines UI.mic so mic SVG always renders");
assert.match(composerJs, /record\.sending \|\| record\.pendingRun \|\| record\.runningIds\?\.length/, "composer enters stop mode immediately on send");
assert.match(composerJs, /data-say-stop/, "composer provides companion Stop button while drafting");
assert.match(composerJs, /conv\.steerRun/, "composer supports steering active work mid-run");
assert.match(personalJs, /steerRun:/, "personal chat wires Server.tasks.steer");
assert.match(personalJs, /next\.pendingRun = carry\.pendingRun/, "new-chat handoff preserves optimistic Stop state");
assert.match(teamConvJs, /stopRun:/, "team chat wires Stop support");
assert.match(app, /\[data-close-work\]"\)\.hidden=!work \|\| m\.split \|\| region==="work"/, "workspace-bar hides duplicate Close work when panel shows its own close");
assert.match(app, /class="icon-btn settings-close" data-settings-done/, "Settings modal uses standard X close button");
assert.match(app, /data-settings-filter/, "Settings modal includes live tab filter");
assert.match(app, /class="palette-list"/, "Cmd+K search dialog includes instant jump palette");
assert.match(app, /label: "Undo"[\s\S]{0,120}Server\.trash\.restore/, "Trash toast offers one-click Undo restore");
assert.match(asksJs, /class="ask-tier"/, "approval cards display risk-tier impact badge");
assert.match(rail, /function syncBadge\(\)/, "Aurora left rail syncs active-work/approval badge");
assert.match(stage, /aurora-verified-pill/, "completed answers show Verified pill when receipts exist");
assert.match(chatCss, /button\.send \{ width: 32px; height: 32px;/, "composer Send/Stop button is the mockup's 32px (decisions/2026-11-27-compact-nova.md)");
assert.match(chatCss, /button\.send\.is-stop/, "chat.css styles Stop square with high contrast");
assert.match(pagesCss, /\.lclose \{ width: 36px; height: 36px;/, "Apps close button harmonized to 36px on desktop");

console.log("aurora stability check passed");
