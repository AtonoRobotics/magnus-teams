// Setup Wizard & Automation Discovery Wizard checks.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const stored = {};
const routes = [];
const opened = [];
const sandbox = {
  esc, enc: encodeURIComponent, UI: {}, Connecting: {},
  icon: k => `<svg data-icon="${k}"></svg>`,
  tile: c => `<span class="ctile">${esc(c.title)}</span>`,
  stateOf: c => (sandbox.Connecting[c.name] ? "connecting" : c.state),
  bindConnectorActs: () => {},
  paint: (el, html) => { el.innerHTML = html; },
  route: (re, view) => routes.push([re, view]),
  load: (k, d) => stored[k] ?? d,
  save: (k, v) => { stored[k] = v; },
  location: { hash: "" },
  App: { me: { id: "ann" }, tasks: [], threads: [], schedules: [] },
  Resources: { read: async () => catalog() },
  Server: { workspaces: { list: async () => [] } },
  document: {
    _els: {},
    body: { appendChild: el => { sandbox.document._els[el.id] = el; } },
    getElementById: id => sandbox.document._els[id] || null,
    createElement: () => ({
      id: "", className: "", innerHTML: "", open: false,
      setAttribute() {}, addEventListener() {},
      showModal() { this.open = true; opened.push(this.id); },
      close() { this.open = false; },
    }),
  },
};
sandbox.globalThis = sandbox;
vm.runInNewContext(readFileSync(join(here, "../ui/onboarding.js"), "utf8") + "\n;globalThis.Guide = Guide; globalThis.WIZARD_TIERS = WIZARD_TIERS;", sandbox);
const G = sandbox;

const app = (name, title, state = "available", extra = {}) => ({ name, title, state, ready: true, for_you: true, listing: { publisher: title }, ...extra });
const catalog = () => [
  app("google-gmail", "Gmail"), app("google-calendar", "Google Calendar"),
  app("microsoft-outlook", "Outlook"),
  app("slack", "Slack"), app("github", "GitHub"),
];

// 1. Getting started is no longer a sidebar menu item.
assert.equal(G.guideNavItem("#/home"), "");

// 2. Setup Wizard steps render Connect Apps, How Magnus Works (interactive approval), and 3 task tiers.
await G.openSetupWizard();
const setupDlg = sandbox.document.getElementById("setup-wizard-modal");
assert.ok(setupDlg && setupDlg.open, "Setup Wizard modal did not open");
assert.match(setupDlg.innerHTML, /Connect the apps Magnus works in/);
assert.match(setupDlg.innerHTML, /Triage inbox (?:&amp;|&#38;) draft replies/);

G.Guide.setup.step = 2;
G.Guide.setup.sim = "always";
await G.openSetupWizard();
assert.match(setupDlg.innerHTML, /How Magnus works safely on your behalf/);
assert.match(setupDlg.innerHTML, /Auto-approve verified supplier orders under \$500/);

G.Guide.setup.step = 3;
G.Guide.setup.tier = "complex";
await G.openSetupWizard();
assert.match(setupDlg.innerHTML, /Research product, contact suppliers (?:&amp;|&#38;) order/);
assert.match(setupDlg.innerHTML, /data-wiz-open-disc/);
assert.match(setupDlg.innerHTML, /data-wiz-launch="complex"/);

// 3. Automation Discovery Wizard opens separately and shows 3 tiers + safety modes.
await G.openDiscoveryWizard();
const discDlg = sandbox.document.getElementById("discovery-wizard-modal");
assert.ok(discDlg && discDlg.open, "Discovery Wizard modal did not open");
assert.match(discDlg.innerHTML, /Where should Magnus look for work to take off your plate\?/);

G.Guide.disc.step = 2;
await G.openDiscoveryWizard();
assert.match(discDlg.innerHTML, /Here is what Magnus found you can automate/);

G.Guide.disc.step = 3;
await G.openDiscoveryWizard();
assert.match(discDlg.innerHTML, /Inbox Triage (?:&amp;|&#38;) Schedule Conflict Resolver/);
assert.match(discDlg.innerHTML, /Ask before external actions/);
assert.match(discDlg.innerHTML, /data-disc-activate/);

// 4. guideFirstRun opens Setup Wizard once for newcomers without redirecting away from home.
setupDlg.close();
opened.length = 0;
delete stored["guide-opened"];
G.App.tasks = []; G.App.threads = [{ thread: "o1", opener: {} }];
await G.guideFirstRun();
assert.ok(opened.includes("setup-wizard-modal"), "first run did not open Setup Wizard modal");
assert.equal(G.location.hash, "");
opened.length = 0;
await G.guideFirstRun();
assert.equal(opened.length, 0, "Setup Wizard opened a second time on first run");

// 5. guideAsk pre-loads the prompt and navigates to #/chat/new.
G.guideAsk(G.WIZARD_TIERS.simple.prompt);
assert.equal(G.App.appSearch, G.WIZARD_TIERS.simple.prompt);
assert.equal(G.location.hash, "#/chat/new");

// 6. #/start route remains registered for deep links / Settings.
assert.ok(routes.some(([re]) => re.test("#/start")), "#/start route missing");
console.log("onboarding check passed");
