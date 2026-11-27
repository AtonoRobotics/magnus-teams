// Regression checks for the UI review improvements
// (decisions/2026-11-23-ui-review-improvements.md):
// - composer @mention keyboard navigation + combobox ARIA, drag-depth counter,
//   clipboard paste-to-attach, attachment sizes, personal-agent label;
// - unified Cmd+K palette, dead TeamCommand.open removal, Settings filter
//   preservation + #/models close fix, backdrop-dismissible dialogs, hover-paused
//   actionable toasts, global Ctrl+B & shortcuts sheet;
// - visibility-aware polling + in-flight guards, rAF-throttled avatar pointermove,
//   cached answersOf, AuroraCore.onDom routing, WebP Explore assets;
// - polite screen-reader reply announcements, centralized --z-* stacking scale,
//   Workspace.openDrawer hook, and app.js domain extraction.
import assert from "node:assert/strict";
import { readAppBundle } from "./app-bundle.mjs";
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const ui = join(here, "../ui");
const read = f => readFileSync(join(ui, f), "utf8");

// --- 1. Behavioral test of chat-composer.js in a minimal DOM harness ---
{
  function makeEl(tag = "div") {
    const attrs = new Map();
    const classes = new Set();
    const listeners = new Map();
    const el = {
      tagName: tag.toUpperCase(),
      id: "",
      value: "",
      hidden: false,
      disabled: false,
      type: "",
      title: "",
      selectionStart: 0,
      selectionEnd: 0,
      scrollHeight: 24,
      isConnected: true,
      dataset: {},
      style: {},
      _html: "",
      _children: [],
      options: [],
      classList: {
        add: c => classes.add(c),
        remove: c => classes.delete(c),
        contains: c => classes.has(c),
        toggle: (c, on) => (on === undefined ? (classes.has(c) ? classes.delete(c) : classes.add(c)) : on ? classes.add(c) : classes.delete(c)),
      },
      setAttribute: (k, v) => attrs.set(k, String(v)),
      getAttribute: k => (attrs.has(k) ? attrs.get(k) : null),
      removeAttribute: k => attrs.delete(k),
      addEventListener: (t, fn) => { listeners.set(t, [...(listeners.get(t) || []), fn]); },
      setSelectionRange: (a, b) => { el.selectionStart = a; el.selectionEnd = b; },
      focus: () => {},
      closest: () => null,
      querySelector: () => null,
      querySelectorAll: () => [],
      requestSubmit: () => { el.submitted = true; },
      set innerHTML(v) {
        this._html = String(v);
        const matches = [...this._html.matchAll(/<button[^>]*id="([^"]*)"[^>]*data-mention="([^"]*)"[^>]*>/g)];
        this._children = matches.map(m => {
          const b = makeEl("button");
          b.id = m[1];
          b.dataset.mention = m[2];
          return b;
        });
      },
      get innerHTML() { return this._html; },
    };
    el.querySelectorAll = sel => (sel === "[data-mention]" ? el._children : []);
    return el;
  }

  const box = makeEl("textarea");
  const pending = makeEl("div");
  const attachInput = makeEl("input");
  const sendBtn = makeEl("button");
  const menu = makeEl("div");
  const zone = makeEl("section");
  const form = makeEl("form");
  form.closest = sel => (sel === "section.conv" ? zone : null);

  let uidCounter = 0;
  const subs = new Set();
  const uploads = [];
  const ctx = {
    $: (sel, rootEl) => {
      if (rootEl === form) {
        if (sel === "textarea") return box;
        if (sel === ".pending") return pending;
        if (sel === "[data-attach]") return attachInput;
        if (sel === ".send") return sendBtn;
        if (sel === ".say-mentions") return menu;
        return null;
      }
      return null;
    },
    esc: s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`),
    icon: k => `<svg data-ic="${k}"></svg>`,
    paint: (el, html) => { el.innerHTML = html; },
    toast: () => {},
    onLeave: () => {},
    innerHeight: 900,
    window: { setTimeout },
    navigator: {},
    MessageDrafts: {
      epoch: () => 1,
      uid: () => `u${++uidCounter}`,
      originals: new Map(),
      subscribe: fn => { subs.add(fn); return () => subs.delete(fn); },
      changed: () => { for (const f of subs) f(); },
    },
  };
  vm.createContext(ctx);
  vm.runInContext(read("chat-composer.js"), ctx);

  const record = { text: "", files: [], selection: [0, 0] };
  const conv = {
    mentions: () => [{ name: "Magnus", agent: true }, { name: "Dana" }],
    upload: async file => { uploads.push(file); return { id: "f1", name: file.name, media_type: file.type, size: file.size }; },
    send: async () => null,
    refresh: async () => {},
  };
  ctx.wireComposer(form, record, conv);

  // Combobox ARIA initialized on textarea
  assert.equal(box.getAttribute("role"), "combobox");
  assert.equal(box.getAttribute("aria-controls"), menu.id);
  assert.equal(box.getAttribute("aria-expanded"), "false");

  // Typing "@" opens mentions with "personal agent" (not retired "team agent")
  box.value = "hi @";
  box.setSelectionRange(4, 4);
  box.oninput();
  assert.equal(menu.hidden, false);
  assert.equal(box.getAttribute("aria-expanded"), "true");
  assert.match(menu.innerHTML, /personal agent/);
  assert.doesNotMatch(menu.innerHTML, /team agent/);
  assert.equal(box.getAttribute("aria-activedescendant"), `${menu.id}-0`);

  // ArrowDown moves active selection to second option ("Dana") and Enter picks Dana
  let prevented = false;
  box.onkeydown({ key: "ArrowDown", preventDefault: () => { prevented = true; } });
  assert.equal(prevented, true);
  assert.equal(box.getAttribute("aria-activedescendant"), `${menu.id}-1`);
  box.onkeydown({ key: "Enter", preventDefault: () => {} });
  assert.equal(box.value, "hi @Dana ");
  assert.equal(menu.hidden, true);
  assert.equal(box.getAttribute("aria-expanded"), "false");

  // Drag depth counter prevents flicker when moving over child elements
  const dragEv = { dataTransfer: { types: ["Files"] }, preventDefault: () => {} };
  zone.ondragenter(dragEv);
  zone.ondragenter(dragEv);
  assert.equal(zone.classList.contains("dropping"), true);
  zone.ondragleave();
  assert.equal(zone.classList.contains("dropping"), true, "dragleave from child must not remove .dropping");
  zone.ondragleave();
  assert.equal(zone.classList.contains("dropping"), false, "final dragleave removes .dropping");

  // Clipboard paste attaches files and displays formatted size
  const fakeFile = { name: "shot.png", type: "image/png", size: 2048 };
  let pastePrevented = false;
  box.onpaste({ clipboardData: { files: [fakeFile] }, preventDefault: () => { pastePrevented = true; } });
  assert.equal(pastePrevented, true);
  assert.equal(uploads.length, 1);
  await new Promise(r => setTimeout(r, 10));
  assert.match(pending.innerHTML, /2\.0 KB · Ready/, "attachment chip shows formatted file size");
}

// --- 2. Batch 1: Settings, unified Cmd+K, dead code & stale labels ---
const app = readAppBundle();
const teamCmd = read("team-command.js");
const teamsCss = read("teams.css");
const teamPanel = read("team-panel.js");
const stageJs = read("aurora-stage.js");
const screenJs = read("screen.js");

assert.match(app, /if\(\(e\.metaKey \|\| e\.ctrlKey\) && e\.key\.toLowerCase\(\)==="k"\)\{e\.preventDefault\(\);openPalette\(\);return;\}/, "Cmd+K consistently opens unified palette");
assert.match(app, /role="combobox" aria-autocomplete="list" aria-controls="workspace-palette-list"/, "palette input has combobox ARIA");
assert.match(app, /openShortcutsModal/, "keyboard shortcuts modal exists");
assert.match(app, /e\.key\.toLowerCase\(\)==="b"/, "Ctrl+B works outside Aurora stage mode");
assert.match(app, /location\.hash === "#\/models"/, "closing Settings restores hash from #/models");
assert.match(app, /if \(!\$\("\.settings-shell", d\)/, "Settings modal preserves filter input across tab switches");
assert.match(app, /data-settings-empty/, "Settings filter shows an empty state");
assert.match(app, /t\.addEventListener\("mouseenter", pause\)/, "actionable toasts pause auto-dismiss on hover");
assert.ok(!teamCmd.includes("open(initial"), "unreachable TeamCommand.open dialog is removed");
assert.ok(!teamsCss.includes(".team-command"), "unused .team-command CSS is removed");
assert.ok(!teamPanel.includes("team agent"), "team-panel uses personal agent label");
assert.ok(!stageJs.includes("Shared Computer"), "aurora-stage removed retired Shared Computer label");
assert.ok(!screenJs.includes("Shared computer"), "screen.js removed retired Shared computer label");

// --- 3. Batch 2: Polling, rAF avatar, observers, caching, WebP assets ---
assert.match(app, /let refreshInFlight = null/, "refresh() guards against overlapping requests");
assert.match(app, /REFRESH_HIDDEN_MS/, "background polling slows down when tab is hidden");
assert.match(app, /document\.addEventListener\("visibilitychange", visibilityHandler\)/, "polling refreshes when returning to foreground");
assert.match(read("notices.js"), /if \(this\.polling\) return;/, "Notices.poll guards against overlapping requests");
assert.match(read("aurora-avatar.js"), /lookRaf = requestAnimationFrame\(/, "avatar pointermove throttled with requestAnimationFrame");
assert.match(stageJs, /answersCache/, "answersOf caches parsed approval HTML");
assert.match(read("aurora-explore.js"), /AuroraCore\.onDom\(onMutate\)/, "aurora-explore routes through AuroraCore.onDom");

const webpAssets = ["hero-antigravity.webp", "hero-gemini.webp", "card-inbox.webp", "card-research.webp", "card-monitor.webp", "card-eng.webp"];
let totalWebpBytes = 0;
const assetsRs = readFileSync(join(root, "crates/magnus/src/ui/assets.rs"), "utf8");
for (const f of webpAssets) {
  assert.ok(existsSync(join(ui, f)), `${f} exists`);
  assert.ok(!existsSync(join(ui, f.replace(".webp", ".jpg"))), `legacy JPEG ${f.replace(".webp", ".jpg")} removed`);
  assert.ok(assetsRs.includes(`("${f}", "image/webp"`), `assets.rs serves ${f} as image/webp`);
  totalWebpBytes += statSync(join(ui, f)).size;
}
assert.ok(totalWebpBytes < 150_000, `Explore WebP assets stay lightweight (got ${totalWebpBytes} bytes)`);

// --- 4. Batch 3: A11y reply announcer, --z-* tokens, Workspace.openDrawer, app.js split ---
assert.match(read("chat-view.js"), /data-chat-announce aria-live="polite"/, "chat-view has polite reply announcer");
const baseCss = read("base.css");
for (const tok of ["--z-base:", "--z-sticky:", "--z-drawer:", "--z-flyout:", "--z-notice:", "--z-toast:", "--z-menu:", "--z-banner:", "--z-tooltip:"]) {
  assert.ok(baseCss.includes(tok), `base.css defines ${tok}`);
}
assert.equal((baseCss.match(/@media \(any-pointer: coarse\)/g) || []).length, 1, "base.css consolidates coarse-pointer media queries into one block");
assert.match(read("layout.css"), /#toasts \{[^}]*z-index: var\(--z-toast\)/, "toasts use --z-toast");
assert.match(read("admin.css"), /#stop-banner \{[^}]*z-index: var\(--z-banner\)/, "stop banner uses --z-banner");
assert.match(read("aurora-rail.js"), /Workspace\.openDrawer\(\)/, "aurora-rail uses Workspace.openDrawer()");

const indexHtml = read("index.html");
for (const mod of ["composed-frames.js", "signin.js", "tasks-schedules.js", "settings-sections.js"]) {
  assert.ok(existsSync(join(ui, mod)), `extracted module ${mod} exists`);
  assert.ok(indexHtml.includes(`<script src="${mod}"></script>`), `index.html loads ${mod}`);
  assert.ok(assetsRs.includes(`("${mod}", "text/javascript`), `assets.rs embeds ${mod}`);
}

console.log("ui polish check passed");
