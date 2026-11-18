// Ordering of actual Theme.apply bridge calls (setBackground) for dark/light mode.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
const calls = [], waiting = [], store = new Map();
const host = {
  localStorage: { getItem: k => store.get(k) ?? null, setItem: (k, v) => store.set(k, v) },
  document: { documentElement: { style: { setProperty() {} }, dataset: {} }, querySelectorAll: () => [] },
  Magnus: { setBackground: (...args) => { calls.push(args); return new Promise((resolve, reject) => waiting.push({ resolve, reject })); } },
  addEventListener() {}, dispatchEvent() {}, CustomEvent: class { constructor(type, options) { this.type = type; this.detail = options.detail; } },
};
host.window = host;
vm.runInNewContext(readFileSync(new URL('../ui/theme.js', import.meta.url), 'utf8'), host);
const T = host.Theme, settle = async () => { for (let i = 0; i < 5; i++) await Promise.resolve(); };
assert.equal(calls.length, 1);
assert.deepEqual(calls[0], [true]);
T.scope('appliance', 'alice');
T.set({ mode: 'light' }); T.set({ accent: 'amber' }); T.set({ panelWidthPx: 420, navigation: 'compact' });
assert.equal(T.look.mode, 'light', 'tokens apply immediately while a host command is pending');
assert.equal(calls.length, 1, 'native operations must not overlap');
waiting.shift().resolve(); await settle();
assert.equal(calls.length, 2, 'coalesce pending choices to the latest mode');
assert.deepEqual(calls[1], [false]);
waiting.shift().resolve(); await settle();
T.set({ panelWidthPx: 460 });
assert.equal(calls.length, 2, 'layout preferences must not repeat host background calls');
T.set({ mode: 'dark' });
assert.equal(calls.length, 3);
waiting.shift().reject(Error('host unavailable')); await settle();
assert.equal(host.document.documentElement.dataset.backdrop, undefined, 'no backdrop state: the window is solid');
T.set({ mode: 'light' });
assert.equal(calls.length, 4, 'a failed host call does not stop the next one');
assert.deepEqual(calls[3], [false]);
console.log('native background: serialized commands, coalescing and failure recovery passed');
