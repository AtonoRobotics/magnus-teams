// Fast-tier tests for Tier 2 Phase 1 (Takeover Ergonomics & Adaptive Launchpads):
// 1. RFB bidirectional clipboard sync (ServerCutText -> onCut, cut() -> ClientCutText type 6).
// 2. Takeover controlStrip renders Paste button when user holds control.
// 3. Sub-agent rows render inline Stop button for active sub-agents and omit it when ended.
// 4. Local computer offline files disable Download/Delete buttons and quick-add folder chips exist.
// 5. Nova Home launchpads adapt to connected apps (GitHub/Mail/Slack vs universal fallbacks).
import assert from "node:assert/strict";
import { readAppBundle } from "./app-bundle.mjs";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);

// 1. RFB ClientCutText & ServerCutText byte-level protocol verification.
{
  const ctx = { Uint8Array, DataView, TextEncoder, Uint8ClampedArray };
  ctx.globalThis = ctx;
  vm.runInNewContext(ui("rfb.js"), ctx);
  const sent = [];
  let cutReceived = null;
  const rfb = new ctx.Rfb({ send: b => sent.push(b), onCut: t => { cutReceived = t; } });
  // ClientCutText: type 6, 3 padding bytes, 4-byte BE length, Latin-1 payload.
  rfb.cut("Hello!");
  assert.equal(sent.length, 1);
  const pkt = sent[0];
  assert.equal(pkt[0], 6, "ClientCutText message type must be 6");
  assert.equal(new DataView(pkt.buffer, pkt.byteOffset, pkt.byteLength).getUint32(4), 6, "ClientCutText length must match payload");
  assert.equal(String.fromCharCode(...pkt.subarray(8)), "Hello!", "ClientCutText payload must match text");

  // ServerCutText: type 3, 3 padding bytes, 4-byte BE length, Latin-1 payload.
  rfb.step = rfb.message;
  const srv = new Uint8Array(8 + 4);
  srv[0] = 3;
  new DataView(srv.buffer).setUint32(4, 4);
  srv.set([84, 101, 115, 116], 8); // "Test"
  rfb.feed(srv);
  assert.equal(cutReceived, "Test", "ServerCutText must decode payload and invoke onCut");
}

// 2. Takeover controlStrip renders Paste button only when user holds control.
{
  const ctx = { esc, enc: encodeURIComponent, App: { me: { id: "u1" }, tasks: [] }, document: { addEventListener() {} }, PANEL_TITLES: {}, PANELS: {}, when: () => "now" };
  ctx.globalThis = ctx;
  vm.runInNewContext(ui("screen.js") + "\nglobalThis.__strip = controlStrip;", ctx);
  const held = ctx.__strip({ status: "running", controller: "u1" }, true, false);
  const idle = ctx.__strip({ status: "running", controller: null }, false, false);
  assert.match(held, /data-paste-clipboard/, "controlStrip must include Paste button when user holds control");
  assert.doesNotMatch(idle, /data-paste-clipboard/, "controlStrip must omit Paste button when not holding control");
}

// 3. Sub-agents inline Stop button.
{
  const ctx = { esc, enc: encodeURIComponent, firstLine: s => String(s || "").split("\n")[0], when: () => "now" };
  ctx.globalThis = ctx;
  vm.runInNewContext(ui("subagents.js") + "\nglobalThis.__row = subAgentRow;", ctx);
  const active = ctx.__row({ run: "r1", task_id: "t1", status: "running", goal: "Check pricing", depth: 1, at: "2026-11-27T00:00:00Z" });
  const done = ctx.__row({ run: "r2", task_id: "t2", status: "completed", goal: "Done work", depth: 1, at: "2026-11-27T00:00:00Z" });
  assert.match(active, /data-stop-subagent="t1"/, "active sub-agent row must include inline Stop button");
  assert.doesNotMatch(done, /data-stop-subagent/, "completed sub-agent row must not include Stop button");
  assert.match(readAppBundle(), /data-stop-subagent[\s\S]*Server\.tasks\.cancel\(id\)/, "app.js must wire data-stop-subagent to Server.tasks.cancel");
}

// 4. Local computer quick-add folder chips & offline file buttons disabled.
{
  const compSrc = ui("computers.js");
  assert.match(compSrc, /data-act="local-add-folder"/, "computers.js must provide quick-add folder chips");
  assert.match(compSrc, /const id = esc\(c\.id\), dis = online \? "" : " disabled";/, "madeThere must disable file actions when computer is offline");
}

// 5. Adaptive Nova launchpads respond to connected apps.
{
  const uxSrc = ui("nova-ux.js");
  assert.match(uxSrc, /function adaptiveLaunchpads\(\)/, "nova-ux.js must compute adaptive launchpads from App.connectors");
  assert.match(uxSrc, /Review open PRs/, "adaptive launchpads must include GitHub-specific prompt");
  assert.match(uxSrc, /Catch up on quiet Slack threads/, "adaptive launchpads must include Slack-specific prompt");
}

console.log("tier 2 phase 1 check ok");
