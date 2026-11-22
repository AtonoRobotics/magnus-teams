// Content-rich hero & Explore gallery checks (desktop/ui/discover.js).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);

const sandbox = {
  esc,
  enc: encodeURIComponent,
  icon: k => `<svg data-icon="${k}"></svg>`,
  paint: (el, html) => { el.innerHTML = html; },
  App: { me: { name: "Alex Rivera" }, seq: 1, appSearch: null },
  Resources: {
    read: async () => [
      { name: "google-gmail", title: "Gmail", state: "connected", for_you: true },
      { name: "github", title: "GitHub", state: "available", for_you: true },
    ],
  },
  location: { hash: "#/chat/new" },
  openDiscoveryWizard: async () => {},
  openSetupWizard: async () => {},
  Event: class { constructor(type) { this.type = type; } },
};
sandbox.globalThis = sandbox;
vm.runInNewContext(readFileSync(join(here, "../ui/discover.js"), "utf8") + "\nglobalThis.Discover = Discover;", sandbox);
const { Discover } = sandbox;

// 1. Hero renders personalized greeting, capability strip, category tabs, playbook cards, and 3 pillars.
const hero = Discover.heroHtml();
assert.match(hero, /Hello, Alex/);
assert.match(hero, /data-open-discovery/);
assert.match(hero, /data-open-setup/);
assert.match(hero, /href="#\/explore"/);
assert.match(hero, /Morning Inbox &amp; Calendar Triage|Morning Inbox &#38; Calendar Triage/);
assert.match(hero, /1\. Give a goal/);
assert.match(hero, /3\. Pauses for your OK/);

// 2. Clicking a playbook card in the hero populates the composer textarea and dispatches input.
let focused = false;
let dispatched = false;
const box = {
  value: "",
  focus() { focused = true; },
  dispatchEvent() { dispatched = true; },
};
const cardEl = { dataset: { starterPrompt: Discover.PLAYBOOKS[0].prompt } };
const handled = Discover.handleHeroClick({
  target: { closest: sel => (sel === ".hello-card" ? cardEl : null) },
}, { section: { querySelector: s => (s === "textarea" ? box : null) } });
assert.equal(handled, true);
assert.equal(box.value, Discover.PLAYBOOKS[0].prompt);
assert.equal(focused, true);
assert.equal(dispatched, true);

// 3. Explore page renders connected apps, all playbooks, and filters cleanly.
const main = { innerHTML: "", onclick: null, oninput: null };
await Discover.show(main);
assert.match(main.innerHTML, /Explore what Magnus can do/);
assert.match(main.innerHTML, /\(1 connected\)/);
assert.match(main.innerHTML, /Gmail/);
assert.match(main.innerHTML, /GitHub/);
assert.match(main.innerHTML, /Daily PR Review Queue/);

// Filter by Engineering category.
await main.onclick({ target: { closest: sel => (sel === "[data-exp-cat]" ? { dataset: { expCat: "eng" } } : null) } });
assert.match(main.innerHTML, /Daily PR Review Queue/);
assert.doesNotMatch(main.innerHTML, /Morning Inbox/);
// GitHub is unconnected in the fixture, so GitHub playbooks show a Connect CTA instead of a dead-end Run button.
assert.match(main.innerHTML, /Connect GitHub/);
assert.match(main.innerHTML, /Read-only · never touches code/);

// 4. Inline configure & run opens inline fields and substitutes placeholders cleanly.
await main.onclick({ target: { closest: sel => (sel === "[data-exp-cat]" ? { dataset: { expCat: "research" } } : null) } });
await main.onclick({ target: { closest: sel => (sel === "[data-explore-config]" ? { dataset: { exploreConfig: "market-matrix" } } : null) } });
assert.match(main.innerHTML, /data-explore-field="market-matrix:space"/);
main.oninput({ target: { id: "", dataset: { exploreField: "market-matrix:space" }, value: "robotics" } });
await main.onclick({ target: { closest: sel => (sel === "[data-explore-launch]" ? { dataset: { exploreLaunch: "market-matrix" } } : null) } });
assert.match(sandbox.App.appSearch, /competitors in robotics/);
assert.equal(sandbox.location.hash, "#/chat/new");

// 5. usePlaybook preloads prompt and navigates to #/chat/new.
Discover.usePlaybook("supplier-rfq");
assert.match(sandbox.App.appSearch, /compare prices across suppliers/);
assert.equal(sandbox.location.hash, "#/chat/new");

console.log("discover check passed");
