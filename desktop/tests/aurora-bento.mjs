// Aurora shows the workspace as a bento in the conversation's column instead
// of opening the companion panel beside it (decisions/2026-11-20-aurora-bento.md).
// In node: Theme.bento names the Aurora styles only.
// In a real browser, with the app's sheets and aurora.js and bento.js on a
// page shaped like the app's:
// - the conversation is the main card and four tiles sit under it, each
//   opening its view, with the tone of what it shows;
// - an auto-opened companion panel is closed; a panel the person opened stays;
// - a narrow column keeps the tiles to one compact row;
// - redrawing the same workspace does not replace the tiles;
// - work waiting on the person is Aurora's "Needs you" state;
// - the request card's spark is amber, cyan or mint for needs, working, done;
// - in another style there is no bento.
//
// MAGNUS_CHROMIUM  Chrome to drive
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const failures = [];
const check = (what, fn) => { try { fn(); console.log(`ok - ${what}`); } catch (e) { failures.push(`${what}: ${e.message}`); console.log(`not ok - ${what}`); } };

const themeCtx = { globalThis: {} };
vm.runInNewContext(ui("theme.js"), themeCtx);
const Theme = themeCtx.globalThis.Theme || themeCtx.Theme;
check("only the Aurora styles have a bento", () => {
  assert.equal(Theme.STYLES.map(s => s[0]).filter(Theme.bento).join(" "), "obsidian-aurora aurora");
});

const { chromium } = createRequire(import.meta.url)(join(root, "browser/node_modules/playwright-core"));
const exe = process.env.MAGNUS_CHROMIUM;
if (!exe) { console.error("MAGNUS_CHROMIUM is not set"); process.exit(2); }

const stubs = `
  window.Theme = { bento: s => s === "aurora" || s === "obsidian-aurora" };
  const ENDED = ["completed", "failed", "cancelled"];
  const App = { tasks: [], schedules: [], connectors: [], inbox: [] };
  const needsMe = () => App.inbox;
  const KIND = { approval: "Approval" };
  function when() { return "2 min ago"; }
  function until() { return "in 1 hour"; }
  const Panel = { autoOpened: false, closed: 0 };
  function closePanel() { Panel.autoOpened = false; Panel.closed++; }`;
const reqcard = st => `<div class="reqgroup chatcard" data-chat-tasks><div class="rg-head"><span>From this request</span><span class="small">1 of 2 done</span></div><ul class="rg-rows">${st.map(s => `<li class="tcard"><a class="rg-row"><span class="st ${s}"><svg class="ic"></svg></span><span class="rg-main"><span class="ttl">Task</span></span></a></li>`).join("")}</ul></div>`;
const page = style => `<!doctype html><html data-style="${style}" data-mode="dark" data-motion="full" data-background="off"
  style="--bg:#05070E;--fg:#F4F4F5;--fg2:#B4B4BC;--muted:#8E8E96;--ok:#34D399;--warn:#FBBF24;--bad:#F87171;--line-strong:#35353E;--surface:#0E1424;--aurora-glass:rgba(13,19,34,.62);--aurora-glass-strong:rgba(15,22,40,.82);--aurora-rim:rgba(186,215,255,.12);--aurora-beam-a:#38BDF8;--aurora-beam-b:#9B6DFF;--aurora-spot:rgba(79,157,255,.2);--ease:cubic-bezier(.22,1,.36,1);--r-md:16px;--r-xl:26px;--r-pill:999px;--nav-width:228px">
<head><style>${ui("base.css")}${ui("layout.css")}${ui("chat.css")}${ui("aurora.css")}${ui("aurora-widgets.css")}</style></head>
<body data-navigation="expanded" data-workspace="conversation"><div class="shell" style="height:720px">
  <nav class="side"><div class="brand"><span class="logo"><svg></svg></span></div></nav>
  <main id="main"><section class="conv"><header class="conv-head" id="conv-head"><div class="row"></div></header>
    <div class="thread" id="thread">${reqcard(["need", "working"])}</div>
    <div id="composer" class="composer-slot"><div class="say"><textarea id="msg"></textarea></div></div></section></main>
  <aside class="detail" id="detail" hidden></aside>
</div><script>${stubs}</script><script>${ui("aurora.js")}</script><script>${ui("bento.js")}</script></body></html>`;

const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
try {
  const p = await browser.newPage({ viewport: { width: 1400, height: 760 } });
  p.on("pageerror", e => failures.push(`page error: ${e.message}`));
  await p.goto("about:blank");
  await p.setContent(page("aurora"));
  await p.evaluate(() => {
    App.tasks = [{ task_id: "r1", status: "running", title: "Draft the board deck", status_line: "Reading Drive" }, { task_id: "d1", status: "completed", title: "Sorted inbox", ended_at: "2026-11-20T09:00:00Z" }];
    App.inbox = [{ id: "i1", kind: "approval", summary: "Send the renewal note" }];
    App.schedules = [{ id: "s1", title: "Morning brief", next: "2026-11-21T08:00:00Z" }];
    App.connectors = [{ name: "gmail", title: "Gmail", state: "connected" }];
    Panel.autoOpened = true;
    Bento.draw();
  });
  await p.waitForTimeout(300);

  const tiles = await p.evaluate(() => [...document.querySelectorAll("main > section.conv + .bento-tiles > .bento-mini")].map(t => ({ key: t.dataset.bentoTile, tone: t.dataset.tone, open: t.dataset.open, title: t.querySelector("strong").textContent })));
  check("four tiles sit under the conversation, each opening its view", () => assert.deepEqual(tiles, [
    { key: "live", tone: "run", open: "task:r1", title: "Draft the board deck" },
    { key: "control", tone: "needs", open: "item:i1", title: "1 decision waiting" },
    { key: "next", tone: "idle", open: "schedule:s1", title: "Morning brief" },
    { key: "recent", tone: "done", open: "task:d1", title: "Sorted inbox" },
  ]));
  const closed = await p.evaluate(() => Panel.closed);
  check("the bento closes the companion the app opened on its own", () => assert.equal(closed, 1));

  const box = await p.evaluate(() => {
    const main = document.querySelector("main"), conv = main.querySelector(".conv"), row = main.querySelector(".bento-tiles");
    const m = getComputedStyle(main), c = getComputedStyle(conv);
    return { mainBg: m.backgroundColor, convBg: c.backgroundColor, below: row.getBoundingClientRect().top >= conv.getBoundingClientRect().bottom, cols: getComputedStyle(row).gridTemplateColumns.split(" ").length };
  });
  check("the conversation is the main card and the tiles sit below it in a row", () => {
    assert.equal(box.mainBg, "rgba(0, 0, 0, 0)");
    assert.notEqual(box.convBg, "rgba(0, 0, 0, 0)");
    assert.ok(box.below);
    assert.equal(box.cols, 4);
  });

  await p.setViewportSize({ width: 860, height: 760 });
  await p.waitForTimeout(100);
  const narrow = await p.evaluate(() => {
    const row = document.querySelector(".bento-tiles"), tiles = [...row.children];
    return { tops: new Set(tiles.map(t => Math.round(t.getBoundingClientRect().top))).size, height: Math.round(row.getBoundingClientRect().height), desc: getComputedStyle(tiles[0].querySelector(".bento-desc")).display };
  });
  check("a narrow column keeps the tiles to one compact row", () => {
    assert.equal(narrow.tops, 1);
    assert.ok(narrow.height <= 90, `row is ${narrow.height}px`);
    assert.equal(narrow.desc, "none");
  });
  await p.setViewportSize({ width: 1400, height: 760 });

  const same = await p.evaluate(async () => {
    const before = document.querySelector(".bento-tiles");
    document.getElementById("thread").insertAdjacentHTML("beforeend", "<p>more</p>");
    Bento.draw();
    await new Promise(r => setTimeout(r, 200));
    return before === document.querySelector(".bento-tiles");
  });
  check("redrawing the same workspace keeps the tiles", () => assert.ok(same));

  const state = await p.evaluate(() => [document.documentElement.dataset.auroraState, document.querySelector(".aurora-state-text")?.textContent]);
  check("work waiting on the person is the Needs you state", () => assert.deepEqual(state, ["needs", "Needs you"]));

  const sparks = await p.evaluate(() => {
    const thread = document.getElementById("thread"), out = [];
    const make = st => `<div class="reqgroup chatcard"><div class="rg-head"><span>From this request</span></div><ul class="rg-rows">${st.map(s => `<li class="tcard"><a class="rg-row"><span class="st ${s}"></span></a></li>`).join("")}</ul></div>`;
    for (const st of [["need", "working"], ["working", "done"], ["done", "done"]]) {
      thread.innerHTML = make(st);
      out.push(getComputedStyle(thread.querySelector(".rg-head > :first-child"), "::before").backgroundColor);
    }
    return out;
  });
  check("the request's spark is amber, cyan, then mint", () => assert.deepEqual(sparks, ["rgb(251, 191, 36)", "rgb(56, 189, 248)", "rgb(52, 211, 153)"]));

  await p.evaluate(() => {
    App.inbox = []; App.tasks = []; App.schedules = []; App.connectors = [];
    Panel.autoOpened = false;
    Bento.draw();
  });
  const idle = await p.evaluate(() => ({ tones: [...document.querySelectorAll(".bento-mini")].map(t => t.dataset.tone), closed: Panel.closed }));
  check("an idle workspace shows idle tiles and leaves a panel the person opened", () => assert.deepEqual(idle, { tones: ["idle", "idle", "idle", "idle"], closed: 1 }));

  await p.evaluate(() => { document.documentElement.dataset.style = "obsidian"; Bento.draw(); });
  const other = await p.evaluate(() => document.querySelectorAll(".bento-tiles").length);
  check("another style has no bento", () => assert.equal(other, 0));
} finally {
  await browser.close();
}
if (failures.length) { console.error(failures.join("\n")); process.exit(1); }
console.log("aurora bento: all checks pass");
