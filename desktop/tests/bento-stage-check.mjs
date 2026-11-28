// Regression check for the Side + Bottom composed bento stage and
// Antigravity-style minimal change history inside #thread:
// 1. Container query anchors on main:has(> section.conv) (aurora-main), NEVER
//    self-querying section.conv (aurora-conv).
// 2. Side Bento (.stage-bento) sits on the right column of section.conv beside
//    #thread, and Bottom Bento (.stage-bento-bottom) spans across the bottom
//    above .composer-slot.
// 3. Sub-agents are consolidated inline inside the single Live Workspace tile
//    (never spawning a separate bento card per sub-agent), and stage caps stay
//    calm (SHOWN_BOXES <= 3, MAX_APPROVAL_BOXES = 1).
// 4. Inline task/sub-agent/result cards inside #thread are compacted into
//    Antigravity-style vertical change history (data-history="1") with
//    auto-collapsed <details> so cards never fill the chat box inline.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const uiDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../ui");
const read = f => fs.readFileSync(path.join(uiDir, f), "utf8");

const stageCss = read("nova-stage.css");
const shellCss = read("nova-shell.css");
const stageJs = read("aurora-stage.js") + "\n" + read("aurora-stage-view.js");

// 1. Container query targets parent main (aurora-main), never self-querying conv.
assert.match(stageCss, /main:has\(> section\.conv\)\s*\{[^}]*container-name:\s*aurora-main;/s, "main:has(> section.conv) must declare container-name: aurora-main");
assert.match(stageCss, /@container\s+aurora-main\s+\(min-width:\s*820px\)/, "composed stage grid must query @container aurora-main");

// 2. Wide composed grid puts .thread left, .stage-bento right (Side), and .stage-bento-bottom full-width below (Bottom).
assert.match(stageCss, /section\.conv\[data-stage-mode="composed"\][^\{]*>\s*\.stage-bento\s*\{[^}]*grid-column:\s*2;\s*grid-row:\s*3;/s, "Side Bento occupies right column beside .thread");
assert.match(stageCss, /section\.conv\[data-stage-mode="composed"\][^\{]*>\s*\.stage-bento-bottom\s*\{[^}]*grid-column:\s*1\s*\/\s*-1;\s*grid-row:\s*4;/s, "Bottom Bento spans full width below .thread and Side Bento");
assert.match(shellCss, /\.stage-bento-bottom\[data-count="0"\]\s*\{[^}]*height:\s*0;/s, "empty Bottom Bento folds to 0 height");

// 3. Antigravity-style change history styling strips card chrome inside #thread.
assert.match(shellCss, /#thread \[data-history="1"\] \.sub-agent\s*\{[^}]*background:\s*transparent;/s, "sub-agent rows in #thread strip card background");
assert.match(shellCss, /#thread \[data-history="1"\] \.card-acts\s*\{\s*display:\s*none;\s*\}/, "inline card action bars are hidden in #thread history");

// 4. aurora-stage.js never repeats on the stage the sub-agents the thread shows inline (decisions/2026-11-28-stage-column-fits-its-tiles.md), and keeps tight caps.
assert.doesNotMatch(stageJs, /key:\s*`sub:\$\{/, "sub-agents must not spawn a separate bento card per sub-agent");
assert.match(stageJs, /const SHOWN_BOXES = 3;/, "stage caps total cards to 3");
assert.match(stageJs, /const MAX_APPROVAL_BOXES = 1;/, "stage shows at most 1 approval card at a time");
assert.doesNotMatch(stageJs, /subAgents:/, "the stage does not list the sub-agents the thread's inline card already shows");
assert.doesNotMatch(stageJs, /data-key="sub:/, "no sub-agent rows inside a stage tile");
assert.doesNotMatch(stageJs, /\} else if \(activeSubs\.length\) \{/, "no stage tile of sub-agents alone, which would repeat the inline card");
assert.match(stageJs, /openKeys\.has\(d\.dataset\.key\)/, "place() preserves expanded inline line items across live updates");
assert.match(stageJs, /function bentoBottomIn\(/, "Bottom Bento container is managed by AuroraStage");
assert.match(stageJs, /const isPrimary = c => c\.key === "live" \|\| c\.key\.startsWith\("approval:"\);/, "primary focus tiles stay in Side Bento while secondary tiles arrange in Bottom Bento");
assert.match(stageJs, /function compactThreadHistory\(conv\)/, "compactThreadHistory() compacts inline process cards in #thread");
assert.match(stageJs, /el\.dataset\.history = "1";/, "compactThreadHistory marks inline process cards as change history");
assert.match(stageJs, /d\.dataset\.historyInit = "1";\s*d\.open = false;/, "compactThreadHistory auto-collapses verbose <details> logs once");


// 5. The stage's side column fits its tiles (decisions/2026-11-28-stage-column-fits-its-tiles.md):
// no wider than a tile needs, no taller than its tiles, and no tip card.
{
  const stageCss = read("nova-stage.css");
  const composed = stageCss.match(/section\.conv\[data-stage-mode="composed"\]:not\(:has\(\.hello-hero\)\)\s*\{([^}]*)\}/);
  assert.ok(composed, "the composed conversation grid rule exists");
  assert.match(composed[1], /grid-template-columns:\s*minmax\(360px, 1fr\) clamp\(280px, 30%, 360px\);/, "the side column is at most 360px and the conversation keeps the rest");
  const side = stageCss.match(/section\.conv\[data-stage-mode="composed"\]:not\(:has\(\.hello-hero\)\) > \.stage-bento\s*\{([^}]*)\}/);
  assert.ok(side, "the composed side bento rule exists");
  assert.match(side[1], /align-self:\s*start;/, "the side column is as tall as its tiles, not the whole row");
  assert.doesNotMatch(side[1], /background:|border-left:/, "no tinted band runs on below the tiles");
  const ux = read("nova-ux.js");
  assert.doesNotMatch(ux, /While Magnus works|nova-wait-tips/, "no tip card takes the stage while Magnus works");
}
console.log("bento-stage-check: ok");
