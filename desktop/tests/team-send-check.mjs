// What a shared conversation's composer sends, with team-chat.js and
// team-conversation.js as they are
// (decisions/2026-11-23-one-request-shape-for-shared-conversations.md):
// - a whisper carries its files to the person's own Magnus as a personal
//   chat's files (their library), and never puts them in the shared Files;
// - a message to the channel puts its files in the shared Files, names them,
//   and keeps an image's preview under its shared project, so the same name
//   in another one shows its own;
// - both send the model as picked, its reasoning included;
// - Stop (and Esc) stops only the viewer's own requests, never another
//   person's.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const code = ["team-chat.js", "team-conversation.js"].map(f => readFileSync(join(here, "../ui", f), "utf8")).join("\n");

/** A page with one composer, `whisper` ticked or not. */
function page({ whisper = false, project = "p1" } = {}) {
  const calls = [];
  const originals = new Map();
  const ctx = {
    console, JSON, Map, Set, Promise, Uint8Array, Date,
    enc: encodeURIComponent,
    esc: s => String(s ?? ""), chip: st => st, statusLineSlot: () => "", firstLine: s => String(s || "").split("\n")[0], teamHref: () => "#",
    toast: () => {},
    $: sel => sel === "[data-whisper]" ? { checked: whisper } : null,
    chosenModel: () => ({ adapter: "a", model: "m", reasoning: "high" }),
    FileReader: class { readAsDataURL(f) { this.result = `data:${f.type};base64,AAAA`; this.onload(); } },
    MessageDrafts: { originals },
    Team: { home: { me: "sam", role: "member", agent: { id: "agent:me", name: "My Magnus", mention: "Magnus" }, members: [], channels: [{ id: "c1", project }, { id: "c2", project: "p2" }], projects: [] } },
    TeamChat: { refresh: async () => {} },
    Conversation: { track: () => {}, stop: async () => {} },
    upload: async list => list.map(f => { calls.push(["library", f.name]); return { id: `file_${f.name}` }; }),
    Magnus: { putBytes: async path => { calls.push(["shared", path]); } },
    Server: { channels: { post: async (ws, ch, body) => { calls.push(["post", ws, ch, body]); return {}; } } },
  };
  vm.createContext(ctx);
  vm.runInContext(`${code}\nthis.__x = { teamComposer, teamRunningIds, teamPreviewKey, teamWork };`, ctx);
  const s = { ws: "w1", channel: "c1", files: {} };
  const composer = ctx.__x.teamComposer(s, null);
  /** Attach `file` as the composer does: upload, then the original kept. */
  const attach = async file => {
    const ref = await composer.upload(file, {});
    const uid = `u-${file.name}`;
    originals.set(uid, file);
    return { ...ref, uid, state: "ready" };
  };
  return { calls, composer, attach, s, x: ctx.__x };
}
const file = (name, type = "image/png") => ({ name, type, size: 4, arrayBuffer: async () => new ArrayBuffer(4) });

// A whisper's file goes to the person's own library, and nowhere shared.
{
  const p = page({ whisper: true });
  const f = await p.attach(file("payslip.png"));
  assert.deepEqual(p.calls, [], "a file went somewhere before the message was sent");
  await p.composer.send({ text: "what do I owe?", files: [f], form: {}, record: {} });
  assert.ok(!p.calls.some(c => c[0] === "shared"), `a whisper's file reached the shared Files: ${JSON.stringify(p.calls)}`);
  assert.deepEqual(p.calls[0], ["library", "payslip.png"]);
  const [, , , body] = p.calls.find(c => c[0] === "post");
  assert.equal(body.whisper, true);
  assert.deepEqual([...body.files], ["file_payslip.png"], "the whisper did not carry its file");
  assert.equal(body.model.reasoning, "high", "the whisper lost the reasoning picked");
  assert.deepEqual(Object.keys(p.s.files), [], "a whisper's image was shown as a shared file");
}

// A message to the channel shares its files, names them, and keeps each
// image's preview under its shared project.
{
  const p = page();
  const f = await p.attach(file("chart.png"));
  await p.composer.send({ text: "here it is", files: [f], form: {}, record: {} });
  assert.deepEqual(p.calls[0], ["shared", "/v1/workspaces/w1/files/chart.png?project=p1"]);
  const [, , , body] = p.calls.find(c => c[0] === "post");
  assert.equal(body.text, "here it is\n📎 chart.png");
  assert.equal(body.files, undefined, "a channel message sent files of the person's own");
  assert.equal(body.model.reasoning, "high", "the channel message lost the reasoning picked");
  assert.ok(p.s.files[p.x.teamPreviewKey("w1", "p1", "chart.png")], "the image's preview was not kept");
  assert.equal(p.s.files["chart.png"], undefined, "a preview was kept under its bare name");
  assert.notEqual(p.x.teamPreviewKey("w1", "p1", "chart.png"), p.x.teamPreviewKey("w2", "p1", "chart.png"), "two shared projects' files of one name share a preview");
  assert.notEqual(p.x.teamPreviewKey("w1", "p1", "chart.png"), p.x.teamPreviewKey("w1", "p2", "chart.png"));
}

// Stop stops the viewer's own requests only.
{
  const { x } = page();
  const r = { me: "sam", ws: "w1", channel: "c1", name: id => id };
  const work = (id, requester, status = "running") => ({ work: x.teamWork({ task_id: id, title: "t", text: "t", status, requester, steps: 0, waiting_on: null, status_line: null, doing: null, long: false, thread: null, started: "" }, r) });
  const entries = [work("mine", "sam"), work("dana's", "dana"), work("done", "sam", "completed"), work("mine", "sam")];
  assert.deepEqual([...x.teamRunningIds(entries, "sam")], ["mine"], "Stop would stop another person's request");
}

console.log("shared conversation sends: whisper files private, channel files shared per project, reasoning kept, Stop only one's own passed");
