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
assert.deepEqual([...T.STYLES.map(s => s[0])], ['nova', 'obsidian', 'obsidian-2', 'obsidian-aurora', 'prism', 'studio', 'aurora']);
assert.ok(T.STYLE_TOKENS.nova && T.STYLE_TOKENS.obsidian && T.STYLE_TOKENS['obsidian-2'] && T.STYLE_TOKENS['obsidian-aurora'] && T.STYLE_TOKENS.prism && T.STYLE_TOKENS.studio && T.STYLE_TOKENS.aurora, 'style radius tokens defined');
assert.equal(T.STYLE_TOKENS.nova['r-composer'], '24px');
assert.equal(T.STYLE_TOKENS.obsidian['r-pill'], '6px');
assert.equal(T.STYLE_TOKENS['obsidian-2']['r-send'], '50%');
assert.equal(T.STYLE_TOKENS['obsidian-aurora']['r-composer'], '14px');
assert.equal(T.STYLE_TOKENS.prism['r-pill'], '999px');
assert.equal(T.STYLE_TOKENS.studio['r-composer'], '22px');
assert.equal(T.STYLE_TOKENS.aurora['r-pill'], '999px');
assert.deepEqual([...T.MODES.map(m => m[0])], ['light', 'dark', 'system']);
assert.deepEqual([...T.BACKGROUNDS.map(b => b[0])], ['off', 'particles', 'ribbons', 'constellation']);
assert.deepEqual([...T.AVATAR_CHOICES.map(a => a[0])], ['pip', 'mochi', 'orb', 'prism']);

// Validation and clamping.
const p = T.preferences({ style: 'retro', mode: 'sepia', material: 'glass', accent: 'neon', motion: 'wild', background: 'lasers', avatar: 'robot', navigation: 'floating', sidebar: 'drawer', panelWidthPx: 9999 });
assert.deepEqual({ ...p }, { style: 'nova', mode: 'dark', accent: 'blue', motion: 'full', background: 'particles', avatar: 'pip', navigation: 'expanded', sidebar: 'rail', panelWidthPx: 560 });
// Stage styles start as a rail unless the sidebar is pinned; other styles keep the navigation chosen.
for (const style of ['nova', 'aurora', 'obsidian-aurora']) {
  assert.equal(T.navigation(T.preferences({ style, navigation: 'expanded' })), 'compact', `${style} starts as a rail`);
  assert.equal(T.navigation(T.preferences({ style, navigation: 'compact', sidebar: 'pinned' })), 'expanded', `${style} keeps a pinned sidebar`);
}
assert.equal(T.navigation(T.preferences({ style: 'obsidian', navigation: 'expanded', sidebar: 'rail' })), 'expanded');
assert.equal(T.navigation(T.preferences({ style: 'obsidian', navigation: 'compact', sidebar: 'pinned' })), 'compact');
assert.equal(T.preferences({ background: 'ribbons' }).background, 'ribbons');
assert.equal(T.preferences({ background: 'constellation' }).background, 'constellation');
assert.equal(T.preferences({ background: 'off' }).background, 'off');
assert.equal(T.preferences({ avatar: 'prism' }).avatar, 'prism');
assert.equal(T.avatar('nova'), 'pip');
assert.equal(T.avatar('nova', 'prism'), 'prism');
assert.equal(T.avatar('obsidian', 'pip'), null);
assert.equal(T.preferences({ panelWidthPx: 10 }).panelWidthPx, 300);
assert.equal(T.preferences({}).panelWidthPx, 360);
assert.equal(T.preferences({ style: 'nova' }).style, 'nova');
assert.equal(T.preferences({ style: 'obsidian-2' }).style, 'obsidian-2');
assert.equal(T.preferences({ style: 'obsidian-aurora' }).style, 'obsidian-aurora');
assert.equal(T.preferences({ style: 'prism' }).style, 'prism');
assert.equal(T.preferences({ style: 'studio' }).style, 'studio');
assert.equal(T.preferences({ style: 'aurora' }).style, 'aurora');
assert.equal(T.preferences({ style: 'antigravity' }).style, 'obsidian');
assert.equal(T.preferences({ style: 'gemini' }).style, 'prism');
assert.equal(T.preferences({ style: 'companion' }).style, 'prism');
assert.equal(T.preferences({ accent: '#7C8CFF' }).accent, '#7C8CFF');
assert.equal(T.preferences({ accent: '#xyz' }).accent, 'blue');

// Records written by the four-preset build.
const v2 = (x) => T.preferences({ version: 2, followComputer: false, motion: 'reduced', navigation: 'compact', panelWidthPx: 480, ...x });
assert.equal(v2({ theme: 'light' }).mode, 'light');
assert.equal(v2({ theme: 'dark' }).mode, 'dark');
assert.equal(v2({ theme: 'ember' }).mode, 'dark');
assert.equal(v2({ theme: 'corporate' }).mode, 'light');
assert.equal(v2({ theme: 'dark', followComputer: true }).mode, 'system');
assert.deepEqual({ ...v2({ theme: 'custom', mode: 'dark', accent: 'amber' }) }, { style: 'nova', mode: 'dark', accent: 'amber', motion: 'reduced', background: 'particles', avatar: 'pip', navigation: 'compact', sidebar: 'rail', panelWidthPx: 480 });

// In the page: per-person storage, legacy read once and left intact, a
// failed write keeps the choice for the session and says so, and old records
// with `material` are migrated on scope().
const store = new Map([['magnus.appearance', JSON.stringify({ mode: 'dark', accent: 'amber', material: 'glass', motion: 'off' })]]);
let fail = false; const events = [];
const page = { localStorage: { getItem: k => store.get(k) ?? null, setItem: (k, v) => { if (fail) throw Error('disk'); store.set(k, v); } },
  document: { documentElement: { style: { setProperty() {} }, dataset: {} }, querySelectorAll: () => [] },
  addEventListener() {}, dispatchEvent: e => events.push(e), CustomEvent: class { constructor(type, o) { this.type = type; this.detail = o.detail; } } };
page.window = page; vm.runInNewContext(source, page); const U = page.Theme;
U.scope('appliance-a', 'alice'); assert.equal(U.prefs().accent, 'amber'); assert.equal(U.prefs().mode, 'dark'); assert.equal(U.prefs().style, 'nova');
assert.equal('material' in JSON.parse(store.get('magnus.ui.v2.appliance-a.alice')), false);
U.set({ style: 'aurora', background: 'constellation', avatar: 'orb' });
assert.equal(U.look.background, 'constellation'); assert.equal(page.document.documentElement.dataset.background, 'constellation'); assert.equal(page.document.documentElement.dataset.avatar, 'orb');
U.set({ style: 'prism', accent: 'blue' });
assert.equal(U.look.style, 'prism'); assert.equal(U.prefs().style, 'prism'); assert.equal(page.document.documentElement.dataset.style, 'prism');
U.set({ accent: 'emerald' }); assert.equal(U.prefs().mode, 'dark', 'an accent never changes the mode');
U.scope('appliance-a', 'bob'); assert.equal(U.prefs().accent, 'amber', 'a new person starts from the legacy record'); assert.equal(U.prefs().style, 'nova');
U.set({ mode: 'light' }); U.scope('appliance-a', 'alice'); assert.equal(U.prefs().accent, 'emerald'); assert.equal(U.prefs().mode, 'dark'); assert.equal(U.prefs().style, 'prism');
store.set('magnus.ui.v2.appliance-a.carol', JSON.stringify({ mode: 'dark', material: 'glass', motion: 'full' }));
U.scope('appliance-a', 'carol'); assert.equal(U.look.mode, 'dark'); assert.equal(U.look.style, 'nova');
assert.equal('material' in JSON.parse(store.get('magnus.ui.v2.appliance-a.carol')), false, 'saved material: glass is migrated out');
// A record from before the Theme choice: its "blue" was the default then, so
// it takes today's default; another accent is a choice and stays. A chosen
// "blue" (stored with a style) stays blue.
store.set('magnus.ui.v2.appliance-a.dan', JSON.stringify({ mode: 'dark', accent: 'blue', motion: 'full', navigation: 'expanded', conversationWidthPx: 420 }));
U.scope('appliance-a', 'dan'); assert.equal(U.prefs().accent, 'blue', 'default accent matches Nova Indigo');
assert.equal(U.prefs().panelWidthPx, 360, 'legacy conversationWidthPx record migrates to default panelWidthPx');
assert.equal(JSON.parse(store.get('magnus.ui.v2.appliance-a.dan')).style, 'nova', 'the record is migrated');
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
// A record saved before Nova became the default holds the default of then
// (written whole by a splitter drag or a pin): it moves to Nova once, keeping
// everything else. A style chosen after that stays.
fail = false;
store.set('magnus.ui.v2.appliance-c.gail', JSON.stringify({ style: 'obsidian', mode: 'light', accent: 'rose', motion: 'full', background: 'off', avatar: 'pip', navigation: 'expanded', sidebar: 'rail', panelWidthPx: 420 }));
U.scope('appliance-c', 'gail');
assert.equal(U.prefs().style, 'nova', 'a record from before Nova moves to Nova');
assert.equal(U.prefs().accent, 'rose'); assert.equal(U.prefs().mode, 'light'); assert.equal(U.prefs().panelWidthPx, 420);
assert.equal(JSON.parse(store.get('magnus.ui.v2.appliance-c.gail')).look, 2, 'the move is written once');
U.set({ style: 'obsidian' }); U.scope('appliance-c', 'gail');
assert.equal(U.prefs().style, 'obsidian', 'a style chosen after the move stays');
U.set({ panelWidthPx: 400 }); U.scope('appliance-c', 'gail');
assert.equal(U.prefs().style, 'obsidian', 'a later splitter drag keeps the chosen style');
console.log('preferences: validation, style, migration, per-person storage and storage failure passed');
