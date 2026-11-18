// Appearance preferences: mode × accent × motion, plus navigation and
// panel-width layout choices. Legacy records with material: "glass" or
// conversationWidthPx are migrated cleanly.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
const source = readFileSync(new URL('../ui/theme.js', import.meta.url), 'utf8');
const context = {}; vm.runInNewContext(source, context); const T = context.Theme;
assert.equal(T.PRESETS, undefined, 'named presets were removed');
assert.equal(T.MATERIALS, undefined, 'material options were removed');
assert.equal(T.GLASS, undefined, 'glass palette was removed');
assert.ok(T.LAYOUT_BOUNDS && T.LAYOUT_BOUNDS.panelDefaultWidthPx === 360 && T.LAYOUT_BOUNDS.panelMinWidthPx === 300 && T.LAYOUT_BOUNDS.panelMaxWidthPx === 560 && T.LAYOUT_BOUNDS.conversationMinWidthPx === 560, 'named layout bounds defined with unit suffixes');
assert.deepEqual([...T.STYLES.map(s => s[0])], ['obsidian', 'obsidian-2', 'prism', 'studio']);
assert.ok(T.STYLE_TOKENS.obsidian && T.STYLE_TOKENS['obsidian-2'] && T.STYLE_TOKENS.prism && T.STYLE_TOKENS.studio, 'style radius tokens defined');
assert.equal(T.STYLE_TOKENS.obsidian['r-pill'], '6px');
assert.equal(T.STYLE_TOKENS['obsidian-2']['r-send'], '50%');
assert.equal(T.STYLE_TOKENS.prism['r-pill'], '999px');
assert.equal(T.STYLE_TOKENS.studio['r-composer'], '22px');
assert.deepEqual([...T.MODES.map(m => m[0])], ['light', 'dark', 'system']);

// Validation and clamping.
const p = T.preferences({ style: 'retro', mode: 'sepia', material: 'glass', accent: 'neon', motion: 'wild', navigation: 'floating', panelWidthPx: 9999 });
assert.deepEqual({ ...p }, { style: 'obsidian', mode: 'dark', accent: 'graphite', motion: 'full', navigation: 'expanded', panelWidthPx: 560 });
assert.equal(T.preferences({ panelWidthPx: 10 }).panelWidthPx, 300);
assert.equal(T.preferences({}).panelWidthPx, 360);
assert.equal(T.preferences({ style: 'obsidian-2' }).style, 'obsidian-2');
assert.equal(T.preferences({ style: 'prism' }).style, 'prism');
assert.equal(T.preferences({ style: 'studio' }).style, 'studio');
assert.equal(T.preferences({ style: 'antigravity' }).style, 'obsidian');
assert.equal(T.preferences({ style: 'gemini' }).style, 'prism');
assert.equal(T.preferences({ style: 'companion' }).style, 'prism');
assert.equal(T.preferences({ accent: '#7C8CFF' }).accent, '#7C8CFF');
assert.equal(T.preferences({ accent: '#xyz' }).accent, 'graphite');

// Records written by the four-preset build.
const v2 = (x) => T.preferences({ version: 2, followComputer: false, motion: 'reduced', navigation: 'compact', panelWidthPx: 480, ...x });
assert.equal(v2({ theme: 'light' }).mode, 'light');
assert.equal(v2({ theme: 'dark' }).mode, 'dark');
assert.equal(v2({ theme: 'ember' }).mode, 'dark');
assert.equal(v2({ theme: 'corporate' }).mode, 'light');
assert.equal(v2({ theme: 'dark', followComputer: true }).mode, 'system');
assert.deepEqual({ ...v2({ theme: 'custom', mode: 'dark', accent: 'amber' }) }, { style: 'obsidian', mode: 'dark', accent: 'amber', motion: 'reduced', navigation: 'compact', panelWidthPx: 480 });

// In the page: per-person storage, legacy read once and left intact, a
// failed write keeps the choice for the session and says so, and old records
// with `material` are migrated on scope().
const store = new Map([['magnus.appearance', JSON.stringify({ mode: 'dark', accent: 'amber', material: 'glass', motion: 'off' })]]);
let fail = false; const events = [];
const page = { localStorage: { getItem: k => store.get(k) ?? null, setItem: (k, v) => { if (fail) throw Error('disk'); store.set(k, v); } },
  document: { documentElement: { style: { setProperty() {} }, dataset: {} }, querySelectorAll: () => [] },
  addEventListener() {}, dispatchEvent: e => events.push(e), CustomEvent: class { constructor(type, o) { this.type = type; this.detail = o.detail; } } };
page.window = page; vm.runInNewContext(source, page); const U = page.Theme;
U.scope('appliance-a', 'alice'); assert.equal(U.prefs().accent, 'amber'); assert.equal(U.prefs().mode, 'dark'); assert.equal(U.prefs().style, 'obsidian');
assert.equal('material' in JSON.parse(store.get('magnus.ui.v2.appliance-a.alice')), false);
U.set({ style: 'prism', accent: 'blue' });
assert.equal(U.look.style, 'prism'); assert.equal(U.prefs().style, 'prism'); assert.equal(page.document.documentElement.dataset.style, 'prism');
U.set({ accent: 'emerald' }); assert.equal(U.prefs().mode, 'dark', 'an accent never changes the mode');
U.scope('appliance-a', 'bob'); assert.equal(U.prefs().accent, 'amber', 'a new person starts from the legacy record'); assert.equal(U.prefs().style, 'obsidian');
U.set({ mode: 'light' }); U.scope('appliance-a', 'alice'); assert.equal(U.prefs().accent, 'emerald'); assert.equal(U.prefs().mode, 'dark'); assert.equal(U.prefs().style, 'prism');
store.set('magnus.ui.v2.appliance-a.carol', JSON.stringify({ mode: 'dark', material: 'glass', motion: 'full' }));
U.scope('appliance-a', 'carol'); assert.equal(U.look.mode, 'dark'); assert.equal(U.look.style, 'obsidian');
assert.equal('material' in JSON.parse(store.get('magnus.ui.v2.appliance-a.carol')), false, 'saved material: glass is migrated out');
// A record from before the Theme choice: its "blue" was the default then, so
// it takes today's default; another accent is a choice and stays. A chosen
// "blue" (stored with a style) stays blue.
store.set('magnus.ui.v2.appliance-a.dan', JSON.stringify({ mode: 'dark', accent: 'blue', motion: 'full', navigation: 'expanded', conversationWidthPx: 420 }));
U.scope('appliance-a', 'dan'); assert.equal(U.prefs().accent, 'graphite', 'the old default accent is not kept as a choice');
assert.equal(U.prefs().panelWidthPx, 360, 'legacy conversationWidthPx record migrates to default panelWidthPx');
assert.equal(JSON.parse(store.get('magnus.ui.v2.appliance-a.dan')).style, 'obsidian', 'the record is migrated');
assert.equal('conversationWidthPx' in JSON.parse(store.get('magnus.ui.v2.appliance-a.dan')), false, 'legacy conversationWidthPx key is removed on migration');
store.set('magnus.ui.v2.appliance-a.erin', JSON.stringify({ mode: 'dark', accent: 'rose', motion: 'full' }));
U.scope('appliance-a', 'erin'); assert.equal(U.prefs().accent, 'rose', 'a chosen accent stays');
U.set({ accent: 'blue' }); U.scope('appliance-a', 'erin'); assert.equal(U.prefs().accent, 'blue', 'Indigo chosen today stays Indigo');
// Nothing is stored for someone who has chosen nothing, so a new default reaches them.
store.delete('magnus.appearance');
U.scope('appliance-b', 'frank'); assert.equal(store.has('magnus.ui.v2.appliance-b.frank'), false, 'defaults are stored as if chosen');
store.set('magnus.appearance', JSON.stringify({ mode: 'dark', accent: 'amber', material: 'glass', motion: 'off' }));
fail = true; U.set({ mode: 'light' }); assert.equal(U.saved, false); assert.equal(U.look.mode, 'light');
assert.equal(JSON.parse(store.get('magnus.appearance')).accent, 'amber', 'the legacy record is never rewritten');
assert.ok(events.some(e => e.type === 'magnus-theme'));
console.log('preferences: validation, style, migration, per-person storage and storage failure passed');
