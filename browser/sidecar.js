// Browser sidecar for the Magnus kernel. It runs on the host, outside the code
// sandbox, and is reached only through the kernel's boundary. Chromium itself
// runs in the jail that jail.sh sets up.
// Protocol: one JSON request per connection on --socket, one JSON reply.
// Requests: { op, run, ... }. Each run gets its own isolated browser context.
'use strict';
const net = require('net');
const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright-core');
const { policy } = require('./destinations');

const arg = n => { const i = process.argv.indexOf(n); return i > 0 ? process.argv[i + 1] : undefined; };
const socket = arg('--socket');
const executablePath = process.env.MAGNUS_CHROMIUM;
const bwrap = process.env.MAGNUS_BWRAP;
// In a browser VM: this identity's profile, and the proxy that is the VM's
// only way out. The host's jail and destination list are for the host.
const vmProfile = process.env.MAGNUS_VM_PROFILE;
const vmProxy = process.env.MAGNUS_VM_PROXY;
if (!socket || !executablePath || (!bwrap && !vmProfile)) { console.error('usage: MAGNUS_CHROMIUM=<chrome> MAGNUS_BWRAP=<bwrap> node sidecar.js --socket <path> [--allow <site>]...'); process.exit(2); }
// Internal sites the owner allows (`[browser] allow`); see destinations.js.
const destinations = vmProfile ? null : policy(process.argv.flatMap((a, i) => a === '--allow' ? [process.argv[i + 1]] : []));
const egress = vmProfile ? null : destinations.proxy();
// The only directory Chromium shares with the host: its profile, and the
// socket to the egress proxy. Made empty each start.
const work = `${socket}.d`;
if (!vmProfile) {
  fs.rmSync(work, { recursive: true, force: true });
  fs.mkdirSync(work, { mode: 0o700 });
  process.env.TMPDIR = work;
}
const proxySocket = path.join(work, 'p');
// A port on the jail's own loopback, where nothing else listens.
const JAIL_PROXY_PORT = 3128;

let browser; // a promise of the launched browser, while one is open
const runs = new Map(); // run -> { context, page }
// Counts close_all: a context made for a request that was already under way
// when the browser closed is closed too, not kept.
let epoch = 0;
const MAX_TEXT = 40000;
// A click on a busy build box used the whole 10s and failed
// (browser_navigates_clicks_types_and_screenshots, runs 41930362272,
// 41902288030, 41929170386) while the same test passed when that box ran
// one suite. Navigation already waits 45s.
const ACTION_MS = 45000;

// This browser reads websites. Host files, browser settings and executable
// URLs are not websites, even when Chromium knows how to open them.
function website(raw) {
  const url = new URL(raw);
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    throw new Error('The browser can only open http and https websites.');
  }
}

function visiblePage(p) {
  if (p.url() !== 'about:blank') website(p.url());
}

// The whole VM is one identity's, so the one context is its profile. TLS
// stops at the VM's own proxy, which checks the real site's certificate.
// What a secure form or the vault filled: a page may repeat it, in its URL or
// its text, and the snapshot then must not.
const filled = new Set();
const hide = text => [...filled].reduce((t, v) => t.split(v).join('(hidden)'), text);
// Chromium drops a cookie with no expiry when it stops, unless it is set to
// continue where it left off. A login is often exactly such a cookie.
function keepSessionCookies() {
  const file = path.join(vmProfile, 'Default', 'Preferences');
  fs.mkdirSync(path.dirname(file), { recursive: true });
  let prefs = {};
  try { prefs = JSON.parse(fs.readFileSync(file, 'utf8')); } catch {}
  fs.writeFileSync(file, JSON.stringify({ ...prefs, session: { ...prefs.session, restore_on_startup: 1 } }));
}
async function vmPage(run) {
  let r = runs.get(run);
  if (!r) {
    keepSessionCookies();
    const context = await chromium.launchPersistentContext(vmProfile, {
      executablePath, chromiumSandbox: false, ignoreHTTPSErrors: true,
      args: ['--disable-dev-shm-usage', '--force-webrtc-ip-handling-policy=disable_non_proxied_udp', '--disable-gpu'],
      proxy: { server: vmProxy },
      viewport: { width: 1280, height: 860 }, acceptDownloads: false,
    });
    r = { context, page: context.pages()[0] || await context.newPage() };
    runs.set(run, r);
  }
  return r.page;
}
async function page(run) {
  if (vmProfile) return vmPage(run);
  const at = epoch;
  if (!browser) {
    // A launch that fails is tried again on the next request.
    // Every connection Chromium makes goes through the egress proxy, loopback
    // included (Playwright adds `<-loopback>` to the bypass list); WebRTC may
    // not send UDP around it. Chromium's own sandbox stays on inside the jail.
    try { fs.accessSync(bwrap, fs.constants.X_OK); } catch {
      throw new Error(`This user cannot run the browser jail (${bwrap}): run install-browser-jail.sh from the release with sudo, then restart so the magnus group applies.`);
    }
    const launching = chromium.launch({
      executablePath: path.join(__dirname, 'jail.sh'),
      chromiumSandbox: true,
      args: ['--disable-dev-shm-usage', '--force-webrtc-ip-handling-policy=disable_non_proxied_udp'],
      proxy: { server: `http://127.0.0.1:${JAIL_PROXY_PORT}` },
      env: {
        MAGNUS_BWRAP: bwrap, MAGNUS_CHROME: fs.realpathSync(executablePath), MAGNUS_NODE: process.execPath, MAGNUS_WORK: work,
        MAGNUS_RELAY: path.join(__dirname, 'relay.js'), MAGNUS_PROXY_SOCK: proxySocket, MAGNUS_PROXY_PORT: String(JAIL_PROXY_PORT),
        PATH: process.env.PATH,
      },
    });
    browser = launching;
    launching.catch(() => { if (browser === launching) browser = undefined; });
  }
  const b = await browser;
  let r = runs.get(run);
  if (!r) {
    const context = await b.newContext({ viewport: { width: 1280, height: 860 }, acceptDownloads: false });
    if (at !== epoch) { await context.close().catch(() => {}); throw new Error('The browser was closed.'); }
    const p = await context.newPage();
    r = { context, page: p };
    runs.set(run, r);
  }
  return r.page;
}

// Interactive elements get refs [e1], [e2] ... that later calls can target.
async function snapshot(p) {
  visiblePage(p);
  const tree = await p.evaluate(() => {
    let n = 0;
    const lines = [];
    const vis = el => { const s = getComputedStyle(el); const r = el.getBoundingClientRect(); return s.visibility !== 'hidden' && s.display !== 'none' && r.width > 0 && r.height > 0; };
    // A password, one-time code or card field's value is the person's
    // secret. The snapshot says the field is filled, never what it holds.
    const secret = el => el.hasAttribute('data-magnus-secret') || (el.tagName === 'INPUT' && el.type === 'password') ||
      (el.getAttribute('autocomplete') || '').toLowerCase().split(/\s+/).some(t => ['current-password', 'new-password', 'one-time-code', 'cc-number', 'cc-csc'].includes(t) || t.startsWith('cc-exp'));
    const label = el => (el.getAttribute('aria-label') || el.innerText || (secret(el) ? '' : el.value) || el.placeholder || el.title || el.alt || '').trim().replace(/\s+/g, ' ').slice(0, 120);
    document.querySelectorAll('[data-magnus-ref]').forEach(el => el.removeAttribute('data-magnus-ref'));
    const walk = (el, depth) => {
      if (!(el instanceof Element) || !vis(el)) return;
      const tag = el.tagName.toLowerCase();
      const role = el.getAttribute('role');
      const interactive = ['a', 'button', 'input', 'select', 'textarea', 'summary'].includes(tag) || ['button', 'link', 'checkbox', 'tab', 'menuitem', 'option', 'textbox', 'combobox'].includes(role) || el.isContentEditable;
      if (interactive) {
        const ref = 'e' + (++n);
        el.setAttribute('data-magnus-ref', ref);
        const kind = role || (tag === 'a' ? 'link' : tag === 'input' ? (el.type || 'text') + ' input' : tag);
        const extra = tag === 'a' && el.href ? ` -> ${el.href}` : (tag === 'input' || tag === 'textarea') && el.value ? (secret(el) ? ' = (hidden)' : ` = "${String(el.value).slice(0, 60)}"`) : '';
        lines.push(`${'  '.repeat(Math.min(depth, 6))}[${ref}] ${kind} "${label(el)}"${extra}`);
        if (tag !== 'select') return;
      } else if (/^h[1-6]$/.test(tag)) {
        lines.push(`${'  '.repeat(Math.min(depth, 6))}${tag}: ${label(el)}`);
        return;
      } else if (['p', 'li', 'td', 'th', 'label', 'span', 'div', 'pre', 'code'].includes(tag)) {
        const own = [...el.childNodes].filter(c => c.nodeType === 3).map(c => c.textContent.trim()).join(' ').replace(/\s+/g, ' ');
        if (own) lines.push(`${'  '.repeat(Math.min(depth, 6))}${own.slice(0, 300)}`);
      }
      for (const c of el.children) walk(c, depth + 1);
    };
    walk(document.body, 0);
    return lines.join('\n');
  });
  const text = hide(`URL: ${p.url()}\nTitle: ${await p.title()}\n\n${tree}`);
  return text.length > MAX_TEXT ? text.slice(0, MAX_TEXT) + '\n[page shortened]' : text;
}

// A picture cannot be masked the way text is, so a page showing a value that
// was filled in is not photographed at all.
async function refuseIfShown(p) {
  if (!filled.size) return;
  const values = [...filled];
  const shown = await p.evaluate(vs => {
    const seen = [document.body ? document.body.innerText : ''];
    for (const i of document.querySelectorAll('input, textarea')) if (i.type !== 'password') seen.push(i.value);
    return vs.some(v => seen.some(t => t.includes(v)));
  }, values);
  if (shown) throw new Error('this page shows a value that was filled in securely, so it was not captured');
}

function locate(p, target) {
  if (/^e\d+$/.test(target)) return p.locator(`[data-magnus-ref="${target}"]`).first();
  return p.getByText(target, { exact: false }).first();
}

async function handle(req) {
  const { op, run } = req;
  if (op === 'close') { const r = runs.get(run); if (r) { await r.context.close(); runs.delete(run); } return { ok: true }; }
  // The kill switch: every run's pages close, with the browser itself; the
  // next request launches a new one.
  if (op === 'close_all') {
    epoch++;
    const b = browser;
    browser = undefined;
    const open = [...runs.values()];
    runs.clear();
    // Closing a profile's context is what writes its cookies out.
    await Promise.all(open.map(r => r.context.close().catch(() => {})));
    if (b) await b.then(x => x.close()).catch(() => {});
    return { ok: true };
  }
  // Agent-built HTML, drawn so the agent can check it: no network of any kind.
  if (op === 'render_html') {
    const q = await (await page(run)).context().newPage();
    try {
      await q.route('**/*', r => /^(data|about):/.test(r.request().url()) ? r.continue() : r.abort());
      await q.setContent(String(req.html), { waitUntil: 'load', timeout: ACTION_MS });
      const b = await q.screenshot({ type: 'png' });
      return { image_b64: b.toString('base64') };
    } finally { await q.close().catch(() => {}); }
  }
  if (op === 'navigate') { website(req.url); if (destinations) await destinations.check(req.url); }
  const p = await page(run);
  switch (op) {
    case 'navigate': {
      const resp = await p.goto(req.url, { waitUntil: 'domcontentloaded', timeout: 45000 });
      // A redirect the proxy refused ends on its refusal.
      if (resp && resp.headers()['x-magnus-refused']) throw new Error(await resp.text());
      await p.waitForLoadState('networkidle', { timeout: 5000 }).catch(() => {});
      return { status: resp ? resp.status() : null, text: await snapshot(p) };
    }
    case 'snapshot': return { text: await snapshot(p) };
    case 'click': { await locate(p, req.target).click({ timeout: ACTION_MS }); await p.waitForLoadState('domcontentloaded').catch(() => {}); await p.waitForTimeout(500); return { text: await snapshot(p) }; }
    case 'type': {
      const l = locate(p, req.target); await l.fill(req.text, { timeout: ACTION_MS });
      if (req.submit) { await l.press('Enter'); await p.waitForLoadState('domcontentloaded').catch(() => {}); await p.waitForTimeout(500); }
      return { text: await snapshot(p) };
    }
    // From the secure form or the vault. The field is marked so no snapshot
    // ever repeats what it holds, whatever kind of input it is.
    case 'fill_secret': {
      const l = locate(p, req.target);
      await l.evaluate(el => el.setAttribute('data-magnus-secret', ''), undefined, { timeout: ACTION_MS });
      // A code of three digits would blank every page that has those digits.
      if (!req.quiet) filled.add(req.value);
      await l.fill(req.value, { timeout: ACTION_MS });
      if (req.submit) { await l.press('Enter'); await p.waitForLoadState('domcontentloaded').catch(() => {}); await p.waitForTimeout(500); }
      return { filled: true };
    }
    case 'select': { await locate(p, req.target).selectOption(req.value, { timeout: ACTION_MS }); return { text: await snapshot(p) }; }
    case 'back': { await p.goBack({ timeout: 15000 }).catch(() => {}); return { text: await snapshot(p) }; }
    case 'screenshot': { visiblePage(p); await refuseIfShown(p); const b = await p.screenshot({ type: 'png' }); return { image_b64: b.toString('base64'), text: `URL: ${p.url()}` }; }
    default: return { error: `unknown op ${op}` };
  }
}

try { fs.unlinkSync(socket); } catch {}
const listen = then => (egress ? egress.listen(proxySocket, then) : then());
listen(() => net.createServer(conn => {
  let buf = '';
  conn.on('data', async d => {
    buf += d.toString();
    const i = buf.indexOf('\n');
    if (i < 0) return;
    const line = buf.slice(0, i); buf = '';
    let out;
    try { out = await handle(JSON.parse(line)); } catch (e) { out = { error: String(e && e.message || e).split('\n')[0] }; }
    conn.end(JSON.stringify(out) + '\n');
  });
}).listen(socket, () => console.error('browser sidecar listening')));
