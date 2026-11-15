// Getting started (decisions/2026-11-15-getting-started.md): three steps,
// each done when the server says so, each one action the rest of the app
// already uses, with what happened said where the step is. onboarding.js
// draws it from plain facts, so it is checked here without a page.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const stored = {};
const routes = [];
const sandbox = {
  esc, enc: encodeURIComponent, UI: {}, Connecting: {},
  icon: k => `<svg data-icon="${k}"></svg>`,
  tile: c => `<span class="ctile">${esc(c.title)}</span>`,
  stateOf: c => (sandbox.Connecting[c.name] ? "connecting" : c.state),
  rule: sc => sc.daily_at ? `Every day at ${sc.daily_at}` : "",
  until: () => "in 3 hours",
  inviteForm: () => `<form data-invite><input type="email" data-invite-email></form>`,
  route: (re, view) => routes.push([re, view]),
  load: (k, d) => stored[k] ?? d,
  save: (k, v) => { stored[k] = v; },
  drafts: {}, sidebarDrawn: 0,
  sidebar: () => { sandbox.sidebarDrawn++; },
  location: { hash: "" },
  App: { me: { id: "ann" }, tasks: [], threads: [], schedules: [] },
};
sandbox.globalThis = sandbox;
vm.runInNewContext(readFileSync(join(here, "../ui/onboarding.js"), "utf8") + "\n;globalThis.Guide = Guide;", sandbox);
const G = sandbox;

const app = (name, title, state = "available", extra = {}) => ({ name, title, state, ready: true, for_you: true, listing: { publisher: title }, ...extra });
const catalog = () => [
  app("google-gmail", "Gmail"), app("google-calendar", "Google Calendar"), app("google-drive", "Google Drive"),
  app("microsoft-outlook", "Outlook"), app("microsoft-calendar", "Outlook Calendar"),
  app("slack", "Slack"), app("github", "GitHub"), app("vercel", "Vercel"),
];
const facts = (over = {}) => ({ connectors: catalog(), workspaces: [], schedules: [], threads: [], mayStart: true, home: null, asked: null, ...over });

// Nothing done yet: the first step is next, and the four platforms are there with their own apps.
{
  const f = facts(), s = G.guideSteps(f);
  assert.equal(s.done, 0);
  assert.deepEqual(JSON.parse(JSON.stringify(s.apps.platforms.map(p => [p.key, p.apps.map(c => c.name)]))), [
    ["google", ["google-gmail", "google-calendar", "google-drive"]], ["microsoft", ["microsoft-outlook", "microsoft-calendar"]], ["slack", ["slack"]], ["github", ["github"]],
  ], "the platforms do not hold their own apps");
  const html = G.guideHtml(f, { busy: null, notes: {} });
  assert.match(html, /class="guide-step next" data-step="apps"/, "the first step is not marked as next");
  assert.match(html, /aria-valuenow="0"/);
  assert.match(html, /0 of 3 done/);
  assert.equal((html.match(/data-act="connect" data-app=/g) || []).length, 7, "every offered app has its own Connect");
  assert.doesNotMatch(html, /data-app="vercel"/, "an app outside the four platforms is in the guide");
  assert.match(html, /data-guide="hide"/, "the guide cannot be hidden");
}

// Each step is done by what the server says, in any order.
{
  const apps = catalog();
  apps[0] = app("google-gmail", "Gmail", "connected", { account: "ann@acme.example" });
  const f = facts({ connectors: apps, schedules: [{ id: "sc1", title: "Morning email", daily_at: "08:00", next: "2026-11-16T16:00:00Z" }] });
  const s = G.guideSteps(f);
  assert.deepEqual([s.apps.done, s.team.done, s.workflow.done, s.done], [true, false, true, 2]);
  const html = G.guideHtml(f, { busy: null, notes: {} });
  assert.match(html, /class="guide-step next" data-step="team"/, "the next step is not the one left");
  assert.match(html, /Connected<\/span> as ann@acme\.example/, "a connected app does not say as whom");
  assert.match(html, /Scheduled: <a[^>]*data-open="schedule:sc1"[^>]*>Morning email<\/a> · Every day at 08:00 · next run in 3 hours/, "the scheduled workflow is not shown");
  assert.match(html, /2 of 3 done/);
}

// What an app says in each state: signing in, signed out, not set up here, not theirs.
{
  const apps = [
    app("google-gmail", "Gmail"),
    app("google-drive", "Google Drive", "needs_attention"),
    app("microsoft-outlook", "Outlook", "available", { ready: false }),
    app("slack", "Slack", "available", { for_you: false }),
    app("github", "GitHub", "connected"),
  ];
  G.Connecting["google-gmail"] = "https://accounts.google.example/auth";
  const threads = [{ thread: "th-gh", title: "What I could take over in GitHub", opener: { app: "github", app_title: "GitHub" } }];
  const html = G.guideHtml(facts({ connectors: apps, threads }), { busy: null, notes: {} });
  delete G.Connecting["google-gmail"];
  const row = name => html.match(new RegExp(`<li class="guide-app" data-app="${name}"[\\s\\S]*?</li>`))?.[0] || "";
  assert.match(row("google-gmail"), /data-state="connecting"[\s\S]*Finish signing in on Gmail's page[\s\S]*data-act="connect-page" data-url="https:\/\/accounts\.google\.example\/auth"/, "signing in does not say where to finish");
  assert.doesNotMatch(row("google-gmail"), /data-act="connect"/, "an app being signed in to offers Connect again");
  assert.match(row("google-drive"), /stopped working[\s\S]*Sign in again/);
  assert.match(row("microsoft-outlook"), /Not set up on this server yet\. Ask your organisation.s owner\./);
  assert.doesNotMatch(row("microsoft-outlook"), /data-act="connect"/, "an app not set up here offers Connect");
  assert.equal(row("slack"), "", "an app blocked for the person is offered");
  assert.match(html, /Slack is not offered on this server\./);
  assert.match(row("github"), /href="#\/chat\/th-gh">See what Magnus could take over/, "a connected app does not lead to what Magnus found there");
}

// Team chat: start one by its name, or join by link; once in, invite people and set how the agent takes part.
{
  const start = G.guideHtml(facts(), { busy: null, notes: {} });
  assert.match(start, /data-guide-form="team"[\s\S]*Create workspace/);
  const join = G.guideHtml(facts({ mayStart: false }), { busy: null, notes: {} });
  assert.match(join, /data-guide-form="join"[\s\S]*Paste your invite link/);
  assert.doesNotMatch(join, /data-guide-form="team"/, "someone who cannot start a workspace is offered to");
  const busy = G.guideHtml(facts(), { busy: "team", notes: { team: { kind: "bad", text: "That name is taken." } } });
  assert.match(busy, /disabled[^>]*>Creating…/, "creating does not show that it is under way");
  assert.match(busy, /role="alert" data-guide-note="team">That name is taken\./, "a failure is not said where the step is");
  const home = { id: "w1", role: "owner", agent: { name: "Magnus" }, channels: [{ id: "d1", name: "", direct: { agent: true }, mode: "mentions" }, { id: "c1", name: "general", mode: "when_helpful" }] };
  const inside = G.guideHtml(facts({ workspaces: [{ id: "w1", name: "Acme", role: "owner" }], home }), { busy: null, notes: { team: { kind: "ok", text: "Created Acme." } } });
  assert.match(inside, /You are in <a href="#\/team\/w1">Acme<\/a>/);
  assert.match(inside, /data-invite/, "a Team Admin cannot invite from the guide");
  assert.match(inside, /data-guide-mode data-ws="w1" data-channel="c1"[\s\S]*value="when_helpful" selected/, "the agent's setting is not the channel's own");
  assert.match(inside, /role="status" data-guide-note="team">Created Acme\./);
  const member = G.guideHtml(facts({ workspaces: [{ id: "w1", name: "Acme", role: "member" }] }), { busy: null, notes: {} });
  assert.doesNotMatch(member, /data-invite|data-guide-mode/, "a member is offered a Team Admin's controls");
}

// No step is a form of its own: each form is one field and one button.
{
  const html = G.guideHtml(facts(), { busy: null, notes: {} });
  for (const form of html.match(/<form class="guide-form"[\s\S]*?<\/form>/g) || []) {
    assert.equal((form.match(/<input/g) || []).length, 1, `a guide form asks for more than one thing: ${form}`);
    assert.doesNotMatch(form, /<select|type="checkbox"/);
  }
}

// The first workflow is asked for in the person's words; suggestions follow what they connected.
{
  assert.deepEqual([...G.guideSuggestions([])], ["Every Monday at 9am, send me a short plan for my week."]);
  const s = [...G.guideSuggestions(["google-gmail", "slack", "github", "google-calendar", "google-drive"])];
  assert.equal(s.length, 4, "too many suggestions");
  assert.match(s[0], /email/); assert.match(s[1], /calendar/); assert.match(s[2], /Slack/); assert.match(s[3], /pull requests/);
  const asked = G.guideHtml(facts({ asked: "Every Friday, tidy my inbox" }), { busy: null, notes: {} });
  assert.match(asked, /You asked Magnus: “Every Friday, tidy my inbox”\. When it proposes the schedule in your conversation, confirm it there/);
  G.guideAsk("Every weekday at 9am, list the pull requests waiting for my review.");
  assert.equal(G.App.appSearch, "Every weekday at 9am, list the pull requests waiting for my review.", "the sentence does not reach the conversation");
  assert.equal(G.location.hash, "#/chat/new");
  assert.equal(stored["guide-asked"], G.App.appSearch);
}

// All done: it says so, and offers to start working instead of hiding.
{
  const apps = catalog(); apps[5] = app("slack", "Slack", "connected");
  const html = G.guideHtml(facts({ connectors: apps, workspaces: [{ id: "w1", name: "Acme", role: "member" }], schedules: [{ id: "s", title: "Weekly plan" }] }), { busy: null, notes: {} });
  assert.match(html, /You are set up/);
  assert.match(html, /aria-valuenow="3"/);
  assert.match(html, /data-guide="finish">Start working/);
  assert.doesNotMatch(html, /data-guide="hide"/);
}

// Invite links as people paste them.
assert.equal(G.guideInviteCode("https://magnus.acme.example/join?code=abc123"), "abc123");
assert.equal(G.guideInviteCode("https://magnus.acme.example/#/join/abc123"), "abc123");
assert.equal(G.guideInviteCode("  abc123 "), "abc123");
assert.equal(G.guideInviteCode("hello there"), "");

// The navigation offers it until it is done or hidden.
{
  G.Guide.done = 1; G.Guide.counted = Date.now();
  assert.match(G.guideNavItem("#/home"), /href="#\/start"[\s\S]*Get started, 1 of 3 done[\s\S]*1\/3/);
  assert.match(G.guideNavItem("#/start"), /aria-current="page"/);
  G.Guide.done = 3;
  assert.equal(G.guideNavItem("#/home"), "", "a finished guide stays in the navigation");
  G.Guide.done = 1; stored.guide = { hidden: true };
  assert.equal(G.guideNavItem("#/home"), "", "a hidden guide stays in the navigation");
  delete stored.guide;
}

// It opens by itself once, for a newcomer, on this device.
{
  G.Guide.counted = Date.now(); G.guideRecount = async () => {};
  G.location.hash = "";
  G.App.tasks = [{ id: "t1" }];
  await G.guideFirstRun();
  assert.equal(G.location.hash, "", "someone with work here was taken to the guide");
  G.App.tasks = []; G.App.threads = [{ thread: "o1", opener: {} }];
  await G.guideFirstRun();
  assert.equal(G.location.hash, "#/start", "a newcomer's first sign-in does not open the guide");
  G.location.hash = "";
  await G.guideFirstRun();
  assert.equal(G.location.hash, "", "the guide opened by itself a second time");
}

assert.ok(routes.some(([re]) => re.test("#/start")), "the guide has no address");
console.log("onboarding check passed");
