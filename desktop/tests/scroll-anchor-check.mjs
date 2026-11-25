// Regression check for chat scroll preservation:
// - Content growth / iframe resize / bento opening while following bottom must
//   NEVER detach follow mode or snap back to the first prompt response.
// - Deliberate upward scrolling by the user DOES detach follow mode and keeps
//   their reading position across resizes; scrolling back to bottom re-pins.
// - Non-persistent panels (persist=false) preserve deliberate reading position.
// - Sending a new user message always re-attaches follow=true to the latest.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const uiDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../ui");
const read = f => fs.readFileSync(path.join(uiDir, f), "utf8");

const appSrc = read("app.js");
const chatViewSrc = read("chat-view.js");
const personalSrc = read("personal-chat.js");

const raMatch = appSrc.match(/const ReadingAnchors = \(\(\) => \{[\s\S]*?\n\}\)\(\);/);
assert.ok(raMatch, "ReadingAnchors block found in app.js");

function makeRoot(scrollHeight, clientHeight) {
  const listeners = new Map();
  return {
    clientHeight,
    scrollHeight,
    _scrollTop: 0,
    get scrollTop() { return this._scrollTop; },
    set scrollTop(v) { this._scrollTop = Math.max(0, Math.min(v, Math.max(0, this.scrollHeight - this.clientHeight))); },
    children: [],
    parentElement: { querySelector: () => ({ setAttribute() {} }) },
    getBoundingClientRect: () => ({ top: 0, bottom: clientHeight }),
    querySelectorAll: () => [],
    querySelector: () => null,
    addEventListener: (ev, fn) => listeners.set(ev, fn),
    removeEventListener: (ev) => listeners.delete(ev),
    fireScroll: () => listeners.get("scroll")?.(),
  };
}

let resizeCb = null;
const ctx = vm.createContext({
  CSS: { escape: s => s },
  ResizeObserver: class { constructor(cb) { resizeCb = cb; } observe() {} disconnect() {} },
  MutationObserver: class { observe() {} disconnect() {} },
});
vm.runInContext(`${raMatch[0]}\nthis.ReadingAnchors = ReadingAnchors;`, ctx);
const { ReadingAnchors } = ctx;

// Scenario A: Chat thread (persist = true).
const root = makeRoot(1600, 600);
const record = { anchor: null };
const unwatch = ReadingAnchors.watch(root, record, true);

// 1. Starts pinned to bottom (follow: true).
assert.equal(root.scrollTop, 1000, "starts at bottom");

// 2. Content grows dramatically right after programmatic restore (iframe load / reply stream) and async scroll fires.
root.scrollHeight = 2600;
root.fireScroll();
assert.equal(root.scrollTop, 2000, "growth right after programmatic restore stays pinned at new bottom");
assert.equal(ReadingAnchors.capture(root).follow, true, "capture preserves follow=true when user did not scroll up");

// 3. Subsequent growth (after programmatic flag cleared) + viewport shrink (Bottom Bento opening).
root.fireScroll(); // consumes programmatic flag at 2000
root.scrollHeight = 3200;
root.clientHeight = 400;
root.fireScroll();
assert.equal(root.scrollTop, 2800, "growth + viewport shrink stays pinned to bottom");

// 4. ResizeObserver fires: stays at bottom, never jumps back to top/first response.
resizeCb();
assert.equal(root.scrollTop, 2800, "ResizeObserver keeps bottom when following");

// 5. History collapse via ReadingAnchors.hold shrinks scrollHeight while following bottom.
ReadingAnchors.hold(root, () => {
  root.scrollHeight = 2000;
  root.scrollTop = root.scrollTop; // browser clamps scrollTop to 1600
});
assert.equal(root.scrollTop, 1600, "hold keeps bottom after height shrink");
assert.equal(ReadingAnchors.capture(root).follow, true, "follow stays true after height shrink");

// 6. User deliberately scrolls UP to read an earlier response.
root._scrollTop = 300;
root.fireScroll();
assert.equal(record.anchor.follow, false, "deliberate upward scroll detaches follow mode");
assert.equal(record.anchor.top, 300, "captures user reading position");
resizeCb();
assert.equal(root.scrollTop, 300, "ResizeObserver preserves user reading position when scrolled up");

// 7. User scrolls back DOWN to the bottom -> re-attaches follow=true.
root._scrollTop = 1600;
root.fireScroll();
assert.equal(record.anchor.follow, true, "scrolling back to bottom re-attaches follow=true");

unwatch();

// Scenario B: Detail panel (persist = false) — preserves exact scrollTop on deliberate scroll & resize.
const detail = makeRoot(1200, 500);
const detailRecord = { anchor: { follow: true } };
const unwatchDetail = ReadingAnchors.watch(detail, detailRecord, false);
detail._scrollTop = 150;
detail.fireScroll();
assert.equal(detailRecord.anchor.follow, false, "detail panel never persists follow=true");
assert.equal(detailRecord.anchor.top, 150, "detail panel records scrollTop");
resizeCb();
assert.equal(detail.scrollTop, 150, "detail panel ResizeObserver preserves scrollTop=150");
unwatchDetail();

// Scenario C: Sending a new message re-pins follow=true.
assert.match(personalSrc, /record\.anchor = \{ follow: true \};/, "personalComposer.send resets record.anchor to follow: true");
assert.match(chatViewSrc, /const newUserMessage = newEntries\.some\(/, "chat-view draw pins follow: true on new user message");

console.log("scroll-anchor-check: ok");
