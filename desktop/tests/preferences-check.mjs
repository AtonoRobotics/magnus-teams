// Appearance preferences (U7, amended; owner 2026-11-08, #543): mode ×
// material × accent × motion, plus the navigation and conversation-width
// layout choices. No named presets. A record saved by the four-preset
// build reads back as its nearest mode so no one's choice is lost.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
const source = readFileSync(new URL('../ui/theme.js', import.meta.url), 'utf8');
const context = {}; vm.runInNewContext(source, context); const T = context.Theme;
assert.equal(T.PRESETS, undefined, 'named presets were removed');
assert.deepEqual([...T.MODES.map(m => m[0])], ['light', 'dark', 'system']);

// Validation and clamping.
const p = T.preferences({ mode: 'sepia', material: 'chrome', accent: 'neon', motion: 'wild', navigation: 'floating', conversationWidthPx: 9999 });
assert.deepEqual({ ...p }, { mode: 'system', material: 'solid', accent: 'blue', motion: 'full', navigation: 'expanded', conversationWidthPx: 560 });
assert.equal(T.preferences({ conversationWidthPx: 10 }).conversationWidthPx, 360);

// Records written by the four-preset build.
const v2 = (x) => T.preferences({ version: 2, followComputer: false, material: 'glass', motion: 'reduced', navigation: 'compact', conversationWidthPx: 480, ...x });
assert.equal(v2({ theme: 'light' }).mode, 'light');
assert.equal(v2({ theme: 'dark' }).mode, 'dark');
assert.equal(v2({ theme: 'ember' }).mode, 'dark');
assert.equal(v2({ theme: 'corporate' }).mode, 'light');
assert.equal(v2({ theme: 'dark', followComputer: true }).mode, 'system');
assert.deepEqual({ ...v2({ theme: 'custom', mode: 'dark', accent: 'clay' }) }, { mode: 'dark', material: 'glass', accent: 'clay', motion: 'reduced', navigation: 'compact', conversationWidthPx: 480 });

// The computer can rule glass out without changing the stored choice.
for (const os of [{ reduceTransparency: true }, { blur: false }, { forcedColors: true }]) assert.equal(T.effective({ material: 'glass' }, os).material, 'solid');
assert.equal(T.effective({ material: 'glass' }, {}).material, 'glass');

// In the page: per-person storage, legacy read once and left intact, a
// failed write keeps the choice for the session and says so.
const store = new Map([['magnus.appearance', JSON.stringify({ mode: 'dark', accent: 'clay', material: 'glass', motion: 'off' })]]);
let fail = false; const events = [];
const page = { localStorage: { getItem: k => store.get(k) ?? null, setItem: (k, v) => { if (fail) throw Error('disk'); store.set(k, v); } },
  document: { documentElement: { style: { setProperty() {} }, dataset: {} }, querySelectorAll: () => [] },
  addEventListener() {}, dispatchEvent: e => events.push(e), CustomEvent: class { constructor(type, o) { this.type = type; this.detail = o.detail; } } };
page.window = page; vm.runInNewContext(source, page); const U = page.Theme;
U.scope('appliance-a', 'alice'); assert.equal(U.prefs().accent, 'clay'); assert.equal(U.prefs().mode, 'dark');
U.set({ accent: 'teal' }); assert.equal(U.prefs().mode, 'dark', 'an accent never changes the mode');
U.scope('appliance-a', 'bob'); assert.equal(U.prefs().accent, 'clay', 'a new person starts from the legacy record');
U.set({ mode: 'light' }); U.scope('appliance-a', 'alice'); assert.equal(U.prefs().accent, 'teal'); assert.equal(U.prefs().mode, 'dark');
store.set('magnus.ui.v2.appliance-a.carol', JSON.stringify({ version: 2, theme: 'ember', followComputer: false, material: 'glass', motion: 'full' }));
U.scope('appliance-a', 'carol'); assert.equal(U.look.mode, 'dark'); assert.equal(U.look.material, 'glass');
fail = true; U.set({ mode: 'light' }); assert.equal(U.saved, false); assert.equal(U.look.mode, 'light');
assert.equal(JSON.parse(store.get('magnus.appearance')).accent, 'clay', 'the legacy record is never rewritten');
assert.ok(events.some(e => e.type === 'magnus-theme'));
console.log('preferences: validation, four-preset records, OS fallbacks, per-person storage and storage failure passed');
