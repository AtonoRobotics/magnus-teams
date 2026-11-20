// The page's one client of its server (desktop/ui/server.js): every named
// call sends its method, path and body, with ids encoded; and the one error
// policy — a step-up asked once and the change sent once more, a spent
// limit's offer, the update screen on 426, "Outcome not confirmed" for a
// change whose reply was lost, and a read for a view no longer on screen.
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { loadServer, refused } from "./server-harness.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const read = f => readFileSync(join(root, f), "utf8");
const plain = v => JSON.parse(JSON.stringify(v ?? null));

// Wiring: loaded right after the transport, before any script that calls it,
// and served by the server.
const scripts = [...read("desktop/ui/index.html").matchAll(/<script src="([^"]+)"><\/script>/g)].map(m => m[1]);
assert.equal(scripts.indexOf("server.js"), scripts.indexOf("bridge.js") + 1, "index.html loads server.js right after bridge.js");
assert.ok(read("crates/magnus/src/ui/assets.rs").includes(`("server.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/server.js"))`), "the server serves server.js");
assert.match(read(".github/workflows/fast.yml"), /node desktop\/tests\/server-check\.mjs/, "the fast tier runs this check");
// The transport carries requests; the policy is server.js's alone.
const bridge = read("desktop/ui/bridge.js"), app = read("desktop/ui/app.js");
assert.ok(!bridge.includes("426") && !bridge.includes("step_up"), "bridge.js holds error policy of its own");
// Every script asks through Server: none spells out a request of its own.
for (const f of readdirSync(join(root, "desktop/ui")).filter(f => f.endsWith(".js") && f !== "bridge.js" && f !== "server.js")) {
  const src = read(`desktop/ui/${f}`);
  assert.ok(!/(^|[^.\w])(api|adminApi)\(/m.test(src) && !/Magnus\.api\(/.test(src), `${f} sends a request around server.js`);
}
assert.ok(!/step_up|Outcome not confirmed|offerMoreSpend\(/.test(app), "app.js holds error policy of its own");
assert.ok(!read("desktop/ui/admin.js").includes("adminApi"), "admin.js keeps a second step-up of its own");

// Every named call: [name, args, method, path, body].
const W = "w 1", C = "c/1", M = "m?1";
const w = "/v1/workspaces/w%201", c = `${w}/channels/c%2F1`, m = `${c}/messages/m%3F1`;
const table = [
  ["stops.list", [], "GET", "/v1/stops", undefined],
  ["me", [], "GET", "/v1/me", undefined],
  ["version", [], "GET", "/v1/version", undefined],
  ["search", ["a b&c"], "GET", "/v1/search?q=a%20b%26c", undefined],
  ["speak", ["Hello."], "POST", "/v1/speak", { text: "Hello." }],
  ["homeOpened", [], "POST", "/v1/home/opened", {}],
  ["tasks.get", ["t 1"], "GET", "/v1/tasks/t%201", undefined],
  ["tasks.timeline", ["t1"], "GET", "/v1/tasks/t1/timeline", undefined],
  ["tasks.action", ["t1", 2, { fence: "f", values: {} }], "POST", "/v1/tasks/t1/actions/2", { fence: "f", values: {} }],
  ["tasks.action", ["t1", 0], "POST", "/v1/tasks/t1/actions/0", undefined],
  ["tasks.verdict", ["t1", "right", "ok"], "POST", "/v1/tasks/t1/verdict", { verdict: "right", note: "ok" }],
  ["inbox.opened", [["i1", "i2"]], "POST", "/v1/inbox/opened", { ids: ["i1", "i2"] }],
  ["inbox.answer", ["i 1", { answer: "yes" }], "POST", "/v1/inbox/i%201", { answer: "yes" }],
  ["ui.app", ["a 1"], "GET", "/v1/ui/apps/a%201", undefined],
  ["ui.appRequest", ["a1", { tool: "t", arguments: {} }], "POST", "/v1/ui/apps/a1/request", { tool: "t", arguments: {} }],
  ["ui.catalog", [], "GET", "/v1/ui/catalog", undefined],
  ["ui.setCatalog", [{ version: "v", elements: ["stat"] }], "PUT", "/v1/ui/catalog", { version: "v", elements: ["stat"] }],
  ["files.list", [], "GET", "/v1/files", undefined],
  ["files.list", [{ q: "", project: null }], "GET", "/v1/files", undefined],
  ["files.list", [{ q: "a b", project: "p1" }], "GET", "/v1/files?q=a%20b&project=p1", undefined],
  ["files.info", ["f 1"], "GET", "/v1/files/f%201/info", undefined],
  ["files.upload", [{ name: "a.md", media_type: "text/markdown", data_b64: "" }], "POST", "/v1/files", { name: "a.md", media_type: "text/markdown", data_b64: "" }],
  ["files.move", ["f1", null], "POST", "/v1/moves", { item: "f1", project: null }],
  ["trash.list", [], "GET", "/v1/trash", undefined],
  ["trash.add", ["f1"], "POST", "/v1/trash", { item: "f1" }],
  ["trash.restore", ["f 1"], "POST", "/v1/trash/f%201/restore", {}],
  ["trash.remove", ["f1"], "DELETE", "/v1/trash/f1", undefined],
  ["trash.empty", [], "POST", "/v1/trash/empty", {}],
  ["projects.get", ["p 1"], "GET", "/v1/projects/p%201", undefined],
  ["projects.create", [{ name: "P", instructions: "" }], "POST", "/v1/projects", { name: "P", instructions: "" }],
  ["projects.update", ["p1", { instructions: "x" }], "PUT", "/v1/projects/p1", { instructions: "x" }],
  ["models.list", [], "GET", "/v1/models", undefined],
  ["models.choose", ["anthropic", "m"], "PUT", "/v1/model", { adapter: "anthropic", model: "m" }],
  ["models.signIn", ["open ai"], "POST", "/v1/adapters/open%20ai/sign-in", {}],
  ["models.completeSignIn", ["a", { flow: "f", input: "x" }], "POST", "/v1/adapters/a/sign-in/complete", { flow: "f", input: "x" }],
  ["models.conformance", ["a"], "POST", "/v1/adapters/a/conformance", {}],
  ["sessions.list", [], "GET", "/v1/sessions", undefined],
  ["sessions.end", ["s 1"], "DELETE", "/v1/sessions/s%201", undefined],
  ["preferences.get", [], "GET", "/v1/preferences", undefined],
  ["preferences.set", [{ notify: true }], "PUT", "/v1/preferences", { notify: true }],
  ["memory.deletionRequests", [], "GET", "/v1/deletion-requests", undefined],
  ["memory.askToDelete", ["my address"], "POST", "/v1/deletion-requests", { what: "my address" }],
  ["memory.relationship", [], "GET", "/v1/relationship", undefined],
  ["memory.note", [{ action: "remove", note: "n1" }], "POST", "/v1/relationship", { action: "remove", note: "n1" }],
  ["memory.disagreements", [], "GET", "/v1/disagreements", undefined],
  ["memory.commitments", [], "GET", "/v1/commitments", undefined],
  ["memory.updateCommitment", ["c 1", { status: "done" }], "PUT", "/v1/commitments/c%201", { status: "done" }],
  ["memory.removeCommitment", ["c1"], "DELETE", "/v1/commitments/c1", undefined],
  ["memory.grants", [], "GET", "/v1/grants", undefined],
  ["memory.setGrant", ["g 1", { until: "x" }], "PUT", "/v1/grants/g%201", { until: "x" }],
  ["memory.removeGrant", ["g1"], "DELETE", "/v1/grants/g1", undefined],
  ["connectors.get", ["my crm"], "GET", "/v1/connectors/my%20crm", undefined],
  ["connectors.push", ["crm"], "GET", "/v1/connectors/crm/push", undefined],
  ["connectors.setToken", ["crm", "tok"], "PUT", "/v1/connectors/crm/token", { token: "tok" }],
  ["connectors.removeGenerated", ["crm"], "DELETE", "/v1/connectors/crm/generated", undefined],
  ["connectors.disconnect", ["crm"], "DELETE", "/v1/connectors/crm/connection", undefined],
  ["connectors.signIn", ["crm", "http://127.0.0.1:9/cb"], "POST", "/v1/connectors/crm/sign-in", { redirect_uri: "http://127.0.0.1:9/cb" }],
  ["connectors.completeSignIn", ["crm", { state: "s", code: "c", iss: "i" }], "POST", "/v1/connectors/crm/sign-in/complete", { state: "s", code: "c", iss: "i" }],
  ["tasks.cancel", ["t 1"], "POST", "/v1/tasks/t%201/cancel", {}],
  ["tasks.resume", ["t1"], "POST", "/v1/tasks/t1/resume", {}],
  ["tasks.steer", ["t 1", "use staging"], "POST", "/v1/tasks/t%201/steer", { text: "use staging" }],
  ["tasks.stopAll", [], "POST", "/v1/tasks/stop-all", {}],
  ["schedules.create", [{ text: "Check inbox", daily_at: "08:00" }], "POST", "/v1/schedules", { text: "Check inbox", daily_at: "08:00" }],
  ["schedules.remove", ["s/1"], "DELETE", "/v1/schedules/s%2F1", undefined],
  ["schedules.resume", ["s1"], "POST", "/v1/schedules/s1/resume", {}],
  ["schedules.pause", ["s1"], "POST", "/v1/schedules/s1/pause", {}],
  ["chat.send", [{ text: "hi", thread: "th" }], "POST", "/v1/chat", { text: "hi", thread: "th" }],
  ["chat.threads", [], "GET", "/v1/chat", undefined],
  ["chat.entry", ["th 1", "e/1", "resend", { text: "again" }], "POST", "/v1/chat/th%201/entries/e%2F1/resend", { text: "again" }],
  ["chat.entry", ["th", "e", "save"], "POST", "/v1/chat/th/entries/e/save", {}],
  ["chat.answerCard", ["k 1", { decision: "yes" }], "POST", "/v1/chat/cards/k%201", { decision: "yes" }],
  ["directory", [], "GET", "/v1/directory", undefined],
  ["rules.list", [], "GET", "/v1/rules", undefined],
  ["rules.accept", ["r 1"], "POST", "/v1/rules/r%201/accept", undefined],
  ["rules.remove", ["r1"], "DELETE", "/v1/rules/r1", undefined],
  ["rules.save", [null, { when: "x" }], "POST", "/v1/rules", { when: "x" }],
  ["rules.save", ["r 1", { when: "x" }], "PUT", "/v1/rules/r%201", { when: "x" }],
  ["staged.list", [], "GET", "/v1/staged", undefined],
  ["staged.resolve", ["st 1", { outcome: "sent", scope: "recipient" }], "POST", "/v1/staged/st%201", { outcome: "sent", scope: "recipient" }],
  ["connectors.terms", ["my crm"], "GET", "/v1/connectors/my%20crm/terms", undefined],
  ["connectors.consent", ["crm", "v2"], "POST", "/v1/connectors/crm/consent", { terms: "v2" }],
  ["account.password", [], "GET", "/v1/account/password", undefined],
  ["account.setPassword", ["old", "new"], "POST", "/v1/account/password", { current_password: "old", new_password: "new" }],
  ["account.totp", [], "GET", "/v1/account/totp", undefined],
  ["account.beginTotp", [], "POST", "/v1/account/totp/begin", {}],
  ["account.confirmTotp", ["123456"], "POST", "/v1/account/totp/confirm", { code: "123456" }],
  ["account.removeTotp", [], "DELETE", "/v1/account/totp", undefined],
  ["account.exportData", [], "GET", "/v1/account/export", undefined],
  ["account.deleteAccount", ["DELETE"], "DELETE", "/v1/account", { confirm: "DELETE" }],
  ["account.mail", [], "GET", "/v1/mail", undefined],
  ["account.testMail", [], "POST", "/v1/mail/test", {}],
  ["account.setSecret", ["smtp/pw", "x"], "PUT", "/v1/secrets/smtp%2Fpw", { value: "x" }],
  ["notices", [null], "GET", "/v1/team/notices", undefined],
  ["notices", ["2026-11-14T10:00:00.000Z"], "GET", "/v1/team/notices?after=2026-11-14T10%3A00%3A00.000Z", undefined],
  ["legal.status", [], "GET", "/v1/legal", undefined],
  ["legal.terms", [{ terms: "t1", privacy: "p1" }], "POST", "/v1/legal/terms", { terms: "t1", privacy: "p1" }],
  ["legal.usage", [{ share: false, version: "u1" }], "POST", "/v1/legal/usage", { share: false, version: "u1" }],
  ["legal.testers", ["join", { version: "v" }], "POST", "/v1/legal/testers/join", { version: "v" }],
  ["legal.testers", ["leave", { version: "v" }], "POST", "/v1/legal/testers/leave", { version: "v" }],
  ["legal.testers", ["decline", { version: "v" }], "POST", "/v1/legal/testers/decline", { version: "v" }],
  ["payments.get", [], "GET", "/v1/payments", undefined],
  ["payments.link", [], "POST", "/v1/payments/link", undefined],
  ["payments.unlink", [], "DELETE", "/v1/payments/link", undefined],
  ["payments.removeCard", [], "DELETE", "/v1/payments/card", undefined],
  ["payments.setCard", [{ number: "4242", exp_month: 1 }], "PUT", "/v1/payments/card", { number: "4242", exp_month: 1 }],
  ["desktop.devices", [], "GET", "/v1/desktop/devices", undefined],
  ["desktop.allowProgram", ["desk 1", "blender", true], "PUT", "/v1/desktop/devices/desk%201/programs/blender", { allowed: true }],
  ["spend.ask", [5, "more"], "POST", "/v1/spend/requests", { usd: 5, reason: "more" }],
  ["computers.list", [], "GET", "/v1/computers", undefined],
  ["computers.get", ["pc 1"], "GET", "/v1/computers/pc%201", undefined],
  ["computers.screen", ["pc"], "GET", "/v1/computers/pc/screen", undefined],
  ["computers.activity", ["pc"], "GET", "/v1/computers/pc/activity", undefined],
  ["computers.start", ["pc 1"], "POST", "/v1/computers/pc%201/start", {}],
  ["computers.hibernate", ["pc"], "POST", "/v1/computers/pc/hibernate", {}],
  ["computers.wake", ["pc"], "POST", "/v1/computers/pc/wake", {}],
  ["computers.pause", ["pc"], "POST", "/v1/computers/pc/kill", { mode: "pause" }],
  ["computers.reset", ["pc", true], "POST", "/v1/computers/pc/reset", { keep_files: true }],
  ["computers.takeover", ["pc"], "POST", "/v1/computers/pc/takeover", {}],
  ["computers.handback", ["pc"], "POST", "/v1/computers/pc/handback", {}],
  ["computers.handbackCloses", ["pc"], "GET", "/v1/computers/pc/handback/closes", undefined],
  ["computers.setTheme", ["pc", "dark"], "PUT", "/v1/computers/pc/theme", { theme: "dark" }],
  ["computers.listing", ["pc", ""], "GET", "/v1/computers/pc/listing?path=", undefined],
  ["computers.listing", ["pc", "/home/a b/"], "GET", "/v1/computers/pc/listing?path=%2Fhome%2Fa%20b%2F", undefined],
  ["computers.file", ["pc", "/a&b.txt"], "GET", "/v1/computers/pc/files?path=%2Fa%26b.txt", undefined],
  ["computers.writeFile", ["pc", "/a.txt", "aGk="], "PUT", "/v1/computers/pc/files", { path: "/a.txt", data_b64: "aGk=" }],
  ["computers.removeFile", ["pc", "/a.txt"], "DELETE", "/v1/computers/pc/files?path=%2Fa.txt", undefined],
  ["computers.secureForm", ["id 1", { target: "#p", value: "x", submit: true, save: false }], "POST", "/v1/identities/id%201/secure-form", { target: "#p", value: "x", submit: true, save: false }],
  ["local.list", [], "GET", "/v1/local/computers", undefined],
  ["local.resume", ["l 1"], "POST", "/v1/local/computers/l%201/resume", {}],
  ["local.remove", ["l1"], "DELETE", "/v1/local/computers/l1", undefined],
  ["local.removeFile", ["l1", "/x y"], "DELETE", "/v1/local/computers/l1/files?path=%2Fx%20y", undefined],
  ["workspaces.list", [], "GET", "/v1/workspaces", undefined],
  ["workspaces.create", ["Ops"], "POST", "/v1/workspaces", { name: "Ops" }],
  ["workspaces.remove", [W], "DELETE", w, undefined],
  ["workspaces.join", ["a/b"], "POST", "/v1/invites/a%2Fb/join", {}],
  ["workspaces.home", [W], "GET", `${w}/home`, undefined],
  ["workspaces.activity", [W], "GET", `${w}/activity`, undefined],
  ["workspaces.panel", [W], "GET", `${w}/panel`, undefined],
  ["workspaces.panel", [W, "p 1"], "GET", `${w}/panel?project=p%201`, undefined],
  ["workspaces.search", [W, "a b&c"], "GET", `${w}/search?q=a%20b%26c`, undefined],
  ["workspaces.direct", [W, { agent: true }], "POST", `${w}/direct`, { agent: true }],
  ["workspaces.start", [W, { kind: "channel", name: "x" }], "POST", `${w}/start`, { kind: "channel", name: "x" }],
  ["workspaces.invite", [W, "a@b.c"], "POST", `${w}/invites`, { email: "a@b.c" }],
  ["workspaces.invite", [W], "POST", `${w}/invites`, {}],
  ["workspaces.setLimit", [W, { usd: 3 }], "POST", `${w}/limit`, { usd: 3 }],
  ["workspaces.people", [W], "GET", `${w}/people`, undefined],
  ["workspaces.setMember", [W, "ada", "admin"], "POST", `${w}/members`, { person: "ada", role: "admin" }],
  ["workspaces.removeMember", [W, "a d"], "DELETE", `${w}/members/a%20d`, undefined],
  ["workspaces.grantApp", [W, "github", ["member"]], "POST", `${w}/connectors`, { app: "github", roles: ["member"] }],
  ["workspaces.appSignIn", [W, "a b", "http://127.0.0.1:9/cb"], "POST", `${w}/connectors/a%20b/sign-in`, { redirect_uri: "http://127.0.0.1:9/cb" }],
  ["workspaces.appSignInComplete", [W, "a b", { state: "s", code: "c" }], "POST", `${w}/connectors/a%20b/sign-in/complete`, { state: "s", code: "c" }],
  ["workspaces.appDisconnect", [W, "a b"], "DELETE", `${w}/connectors/a%20b/connection`, undefined],
  ["workspaces.addTask", [W, { title: "T", project: "p" }], "POST", `${w}/tasks`, { title: "T", project: "p" }],
  ["workspaces.updateTask", [W, "t 1", { status: "done" }], "PUT", `${w}/tasks/t%201`, { status: "done" }],
  ["workspaces.removeTask", [W, "t1"], "DELETE", `${w}/tasks/t1`, undefined],
  ["workspaces.routines", [W], "GET", `${w}/routines`, undefined],
  ["workspaces.addRoutine", [W, { channel: "c1", text: "Hi", when: "09:00" }], "POST", `${w}/routines`, { channel: "c1", text: "Hi", when: "09:00" }],
  ["workspaces.updateRoutine", [W, "r 1", { stopped: false }], "PATCH", `${w}/routines/r%201`, { stopped: false }],
  ["workspaces.removeRoutine", [W, "r 1"], "DELETE", `${w}/routines/r%201`, undefined],
  ["workspaces.setProjectMembers", [W, "p 1", ["ada"]], "PUT", `${w}/projects/p%201`, { members: ["ada"] }],
  ["workspaces.removeFile", [W, "a b.md"], "DELETE", `${w}/files/a%20b.md`, undefined],
  ["workspaces.knowledge", [W, "p1"], "GET", `${w}/knowledge?project=p1`, undefined],
  ["workspaces.addKnowledge", [W, { statement: "s" }], "POST", `${w}/knowledge`, { statement: "s" }],
  ["workspaces.correctKnowledge", [W, "k 1", "s"], "POST", `${w}/knowledge/k%201/correct`, { statement: "s" }],
  ["workspaces.flagKnowledge", [W, "k1", "why"], "POST", `${w}/knowledge/k1/flag`, { why: "why" }],
  ["workspaces.retireKnowledge", [W, "k1"], "POST", `${w}/knowledge/k1/retire`, {}],
  ["workspaces.shareAnswer", [W, "a 1"], "POST", `${w}/answers/a%201/share`, {}],
  ["workspaces.shareWhisper", [W, "h1"], "POST", `${w}/whispers/h1/share`, {}],
  ["channels.view", [W, C], "GET", `${c}/view`, undefined],
  ["channels.whispers", [W, C], "GET", `${c}/whispers`, undefined],
  ["channels.read", [W, C], "POST", `${c}/read`, {}],
  ["channels.typing", [W, C], "POST", `${c}/typing`, {}],
  ["channels.post", [W, C, { text: "hi" }], "POST", `${c}/messages`, { text: "hi" }],
  ["channels.archive", [W, C, true], "POST", `${c}/archive`, { on: true }],
  ["channels.clear", [W, C], "POST", `${c}/clear`, {}],
  ["channels.act", [W, C, "rename", { name: "x" }], "POST", `${c}/rename`, { name: "x" }],
  ["channels.react", [W, C, M, "👍", true], "POST", `${m}/react`, { emoji: "👍", on: true }],
  ["messages.edit", [W, C, M, "new"], "PUT", m, { text: "new" }],
  ["messages.retry", [W, C, M], "POST", `${m}/retry`, {}],
  ["messages.share", [W, C, M, "ada"], "POST", `${m}/share`, { person: "ada" }],
  ["messages.save", [W, C, M], "POST", `${m}/save`, {}],
  ["messages.pin", [W, C, M, false], "POST", `${m}/pin`, { pinned: false }],
  ["messages.remove", [W, C, M], "DELETE", m, undefined],
  ["dev.runs", [new URLSearchParams({ status: "failed", ids: "a,b" })], "GET", "/v1/dev/runs?status=failed&ids=a%2Cb", undefined],
  ["dev.runs", [new URLSearchParams()], "GET", "/v1/dev/runs?", undefined],
  ["dev.run", ["r 1"], "GET", "/v1/dev/runs/r%201", undefined],
  ["dev.runLog", ["r1", 40, 200], "GET", "/v1/dev/runs/r1/log?after=40&limit=200", undefined],
  ["dev.experiments", [], "GET", "/v1/dev/experiments", undefined],
  ["dev.experiment", ["x 1", new URLSearchParams()], "GET", "/v1/dev/experiments/x%201", undefined],
  ["dev.experiment", ["x1", new URLSearchParams({ k: "3", base: "m" })], "GET", "/v1/dev/experiments/x1?k=3&base=m", undefined],
  ["dev.metrics", [], "GET", "/v1/dev/metrics", undefined],
  ["dev.policy", [], "GET", "/v1/dev/policy", undefined],
  ["dev.toolScreens", [], "GET", "/v1/dev/tool-screens", undefined],
  ["dev.playbook", [], "GET", "/v1/dev/playbook", undefined],
  ["dev.computers", [], "GET", "/v1/dev/computers", undefined],
  ["dev.pauseComputer", ["pc 1"], "POST", "/v1/dev/computers/pc%201/pause", {}],
  ["dev.startComputer", ["pc"], "POST", "/v1/dev/computers/pc/start", {}],
  ["dev.egress", ["pc", 7], "GET", "/v1/dev/computers/pc/egress?after=7", undefined],
  ["admin.standing", [], "GET", "/v1/admin/standing", undefined],
  ["admin.stepUp", ["pw", null], "POST", "/v1/admin/step-up", { password: "pw", code: null }],
  ["admin.overview", [], "GET", "/v1/admin/overview", undefined],
  ["admin.killSwitch", [], "GET", "/v1/admin/kill-switch", undefined],
  ["admin.killSwitches", [], "GET", "/v1/admin/kill-switches", undefined],
  ["admin.press", ["server", null], "POST", "/v1/admin/kill-switch/press", {}],
  ["admin.press", ["agent", "a 1"], "POST", "/v1/admin/kill-switch/agent/a%201/press", {}],
  ["admin.release", ["person", "ada"], "POST", "/v1/admin/kill-switch/person/ada/release", {}],
  ["admin.release", ["server", null], "POST", "/v1/admin/kill-switch/release", {}],
  ["admin.agents", [], "GET", "/v1/admin/agents", undefined],
  ["admin.pauseAgent", ["a 1"], "POST", "/v1/admin/agents/a%201/pause", {}],
  ["admin.resumeAgent", ["a1"], "POST", "/v1/admin/agents/a1/resume", {}],
  ["admin.connectors", [], "GET", "/v1/admin/connectors", undefined],
  ["admin.updateConnector", ["git hub", { enabled: true }], "PATCH", "/v1/admin/connectors/git%20hub", { enabled: true }],
  ["admin.setConnectorCredential", ["gh", "k"], "PUT", "/v1/admin/connectors/gh/credential", { value: "k" }],
  ["admin.updates", [], "GET", "/v1/admin/updates", undefined],
  ["admin.install", [], "POST", "/v1/admin/updates/install", {}],
  ["admin.rollback", [], "POST", "/v1/admin/updates/rollback", {}],
  ["admin.setUpdateChannel", ["server", "beta"], "PUT", "/v1/admin/updates/server-channel", { channel: "beta" }],
  ["admin.setUpdateChannel", ["desktop", "stable"], "PUT", "/v1/admin/updates/desktop-channel", { channel: "stable" }],
  ["admin.backups", [], "GET", "/v1/admin/backups", undefined],
  ["admin.backUp", [], "POST", "/v1/admin/backups", {}],
  ["admin.computers", [], "GET", "/v1/admin/computers", undefined],
  ["admin.health", [], "GET", "/v1/admin/health", undefined],
  ["admin.people", [], "GET", "/v1/admin/people", undefined],
  ["admin.addPerson", [{ name: "A", email: "a@b", role: "member", send: true }], "POST", "/v1/admin/people", { name: "A", email: "a@b", role: "member", send: true }],
  ["admin.setRole", ["a 1", "admin"], "PATCH", "/v1/admin/people/a%201", { role: "admin" }],
  ["admin.resendInvite", ["a1"], "POST", "/v1/admin/people/a1/invite", { send: true }],
  ["admin.cancelInvite", ["a1"], "POST", "/v1/admin/people/a1/cancel-invite", {}],
  ["admin.signOut", ["a1"], "POST", "/v1/admin/people/a1/sign-out", {}],
  ["admin.disable", ["a1"], "POST", "/v1/admin/people/a1/disable", {}],
  ["admin.enable", ["a1"], "POST", "/v1/admin/people/a1/enable", {}],
  ["admin.removePerson", ["a1", { keep: "b" }], "DELETE", "/v1/admin/people/a1", { keep: "b" }],
  ["admin.setTeamAdmin", ["a1", true], "PUT", "/v1/admin/people/a1/team-admin", { on: true }],
  ["admin.workspaces", [], "GET", "/v1/admin/workspaces", undefined],
  ["admin.policy", [], "GET", "/v1/admin/policy", undefined],
  ["admin.policyHistory", ["org"], "GET", "/v1/admin/policy/history?layer=org", undefined],
  ["admin.draftPolicy", ["org", "permit"], "POST", "/v1/admin/policy/draft", { layer: "org", cedar: "permit" }],
  ["admin.applyPolicy", ["org", "permit"], "POST", "/v1/admin/policy/apply", { layer: "org", cedar: "permit" }],
  ["admin.rules", [], "GET", "/v1/admin/rules", undefined],
  ["admin.addRule", [{ effect: "deny" }], "POST", "/v1/admin/rules", { effect: "deny" }],
  ["admin.removeRule", ["r 1"], "DELETE", "/v1/admin/rules/r%201", undefined],
  ["admin.setAutoReview", [false], "PUT", "/v1/admin/auto-review", { acts: false }],
  ["admin.signInPolicy", [], "GET", "/v1/admin/sign-in-policy", undefined],
  ["admin.setSignInPolicy", [{ mfa: true }], "PUT", "/v1/admin/sign-in-policy", { mfa: true }],
  ["admin.audit", [], "GET", "/v1/admin/audit", undefined],
  ["admin.audit", ["who=ada"], "GET", "/v1/admin/audit?who=ada", undefined],
  ["admin.auditExport", [""], "GET", "/v1/admin/audit/export", undefined],
  ["admin.auditExport", ["who=ada"], "GET", "/v1/admin/audit/export?who=ada", undefined],
  ["admin.retention", [], "GET", "/v1/admin/retention", undefined],
  ["admin.setRetention", [{ trash_days: 30, backup_days: 7 }], "PUT", "/v1/admin/retention", { trash_days: 30, backup_days: 7 }],
  ["admin.developerAccess", [], "GET", "/v1/admin/developer-access", undefined],
  ["admin.identityProvider", [], "GET", "/v1/admin/identity-provider", undefined],
  ["admin.testIdentityProvider", [{ issuer: "x" }], "POST", "/v1/admin/identity-provider/test", { issuer: "x" }],
  ["admin.pollIdentityProviderTest", [], "POST", "/v1/admin/identity-provider/test/poll", {}],
  ["admin.setIdentityProvider", [null], "PUT", "/v1/admin/identity-provider", { provider: null }],
  ["admin.spend", [], "GET", "/v1/admin/spend", undefined],
  ["admin.spend", ["2026-11"], "GET", "/v1/admin/spend?month=2026-11", undefined],
  ["admin.setSpendLimit", [{ server_usd: 5 }], "POST", "/v1/admin/spend/limits", { server_usd: 5 }],
  ["admin.answerSpendRequest", ["q 1", { approve: true, usd: 5, note: "" }], "POST", "/v1/admin/spend/requests/q%201", { approve: true, usd: 5, note: "" }],
  ["admin.models", [], "GET", "/v1/admin/models", undefined],
  ["admin.providers", [], "GET", "/v1/admin/providers", undefined],
  ["admin.updateProvider", ["open ai", { enabled: false }], "PATCH", "/v1/admin/providers/open%20ai", { enabled: false }],
  ["admin.testProvider", ["p1"], "POST", "/v1/admin/providers/p1/test", {}],
  ["admin.setModelAccess", [{ person: "ada", models: null }], "POST", "/v1/admin/models/access", { person: "ada", models: null }],
  ["admin.testers", [], "GET", "/v1/admin/testers", undefined],
  ["admin.inviteTester", ["ada"], "POST", "/v1/admin/testers/invite", { person: "ada" }],
];

{
  const { Server, calls } = loadServer();
  const at = name => name.split(".").reduce((o, k) => o?.[k], Server);
  for (const [name, args, method, path, body] of table) {
    const before = calls.length;
    assert.equal(typeof at(name), "function", `Server.${name} is not a call`);
    await at(name)(...args);
    assert.equal(calls.length, before + 1, `Server.${name} sent ${calls.length - before} requests`);
    const [m, p, b] = calls.at(-1);
    assert.deepEqual([m, p, plain(b)], [method, path, plain(body)], `Server.${name}(${JSON.stringify(args)})`);
  }
  // Every named call is in the table, so none goes unchecked.
  const named = [];
  const walk = (o, prefix) => { for (const [k, v] of Object.entries(o)) { if (typeof v === "function") named.push(prefix + k); else if (v && typeof v === "object") walk(v, `${prefix}${k}.`); } };
  walk(Server, "");
  const unchecked = named.filter(n => n !== "call" && !table.some(([t]) => t === n));
  assert.deepEqual(unchecked, [], `named calls this check does not send: ${unchecked.join(", ")}`);
}

const stepUpRefusal = () => refused(403, { error: "step_up" });

// A change held for the password: the sheet opens once, and the change is sent once more.
{
  let asked = 0;
  const { Server, calls } = loadServer({ "PATCH /v1/admin/people/ada": [stepUpRefusal(), { ok: true }] }, { stepUp: async () => { asked++; return true; } });
  assert.deepEqual(plain(await Server.admin.setRole("ada", "admin")), { ok: true });
  assert.equal(asked, 1, "the step-up sheet did not open once");
  assert.equal(calls.length, 2, "the change was not sent again");
  assert.deepEqual(plain(calls[1]), ["PATCH", "/v1/admin/people/ada", { role: "admin" }], "the change sent again is not the same change");
}
// Cancelled, nothing more is sent and the error says it was cancelled.
{
  const { Server, calls } = loadServer({ "PATCH /v1/admin/people/ada": stepUpRefusal() }, { stepUp: async () => false });
  await assert.rejects(Server.admin.setRole("ada", "admin"), e => e.cancelled === true && e.message === "The change was not made.");
  assert.equal(calls.length, 1, "a cancelled step-up sent the change anyway");
}
// Asked once only: a second step-up refusal is the answer, not a second sheet.
{
  let asked = 0;
  const { Server, calls } = loadServer({ "POST /v1/admin/backups": stepUpRefusal() }, { stepUp: async () => { asked++; return true; } });
  await assert.rejects(Server.admin.backUp(), e => e.status === 403 && e.body.error === "step_up");
  assert.equal(asked, 1, "the step-up sheet opened twice for one change");
  assert.equal(calls.length, 2);
}
// A plain refusal never asks for the password.
{
  let asked = 0;
  const { Server } = loadServer({ "GET /v1/admin/people": refused(403, { error: "this needs an owner or admin" }) }, { stepUp: async () => { asked++; return true; } });
  await assert.rejects(Server.admin.people(), /owner or admin/);
  assert.equal(asked, 0, "a plain refusal asked for the password");
}

// A spent limit of the person's own offers to ask for more, and still fails.
{
  const offered = [];
  const body = { error: "spend_limit", spend_limit: { scope: "person" } };
  const { Server } = loadServer({ "POST /v1/chat": refused(402, body) }, { offerMoreSpend: b => offered.push(b) });
  await assert.rejects(Server.chat.send({ text: "hi" }), e => e.status === 402);
  assert.deepEqual(plain(offered), [body], "a spent limit did not offer to ask for more");
  const other = [];
  const { Server: s2 } = loadServer({ "POST /v1/chat": refused(402, { error: "payment" }) }, { offerMoreSpend: b => other.push(b) });
  await assert.rejects(s2.chat.send({ text: "hi" }), e => e.status === 402);
  assert.equal(other.length, 0, "a 402 without a spend limit offered to ask");
}

// 426: the app is older than the server takes; the update screen opens, from a poll too.
{
  const events = [];
  const window = { dispatchEvent: e => events.push(e) };
  class CustomEvent { constructor(type, init) { this.type = type; this.detail = init?.detail; } }
  const body = { update_required: true, min_app: "0.3.0" };
  const { Server } = loadServer({ "GET /v1/workspaces": refused(426, body), "GET /v1/stops": refused(426, body) }, { window, CustomEvent });
  await assert.rejects(Server.workspaces.list(), e => e.status === 426);
  await assert.rejects(Server.stops.list({ background: true }), e => e.status === 426);
  assert.deepEqual(events.map(e => [e.type, plain(e.detail)]), [["magnus-update-required", body], ["magnus-update-required", body]]);
}

// A change whose reply was lost, or failed on the server: its outcome is unknown.
{
  const lost = Object.assign(new Error("Cannot reach the server"), { status: undefined });
  const { Server } = loadServer({ "POST /v1/tasks/t/cancel": lost, "DELETE /v1/schedules/s": refused(502, { error: "bad gateway" }), "GET /v1/workspaces": refused(500, { error: "boom" }), "POST /v1/tasks/t/resume": refused(409, { error: "already running" }) });
  await assert.rejects(Server.tasks.cancel("t"), e => e.message === "Outcome not confirmed. Cannot reach the server" && e.status === undefined);
  await assert.rejects(Server.schedules.remove("s"), e => e.message === "Outcome not confirmed. bad gateway" && e.status === 502 && e.body.error === "bad gateway");
  await assert.rejects(Server.workspaces.list(), e => e.message === "boom", "a read's failure is not a change's");
  await assert.rejects(Server.tasks.resume("t"), e => e.message === "already running", "a refused change is not an unknown one");
}

// A server that cannot read the session (its database, say) answers 503: not
// a sign-out. The error carries the server's message, the person stays signed
// in, and nothing opens sign-in; only a 401 ends a session
// (decisions/2026-11-14-a-database-fault-is-not-a-sign-out.md).
{
  const unread = "The server could not check your sign-in just now. You are still signed in; try again in a moment.";
  const App = { me: { id: "sam" }, appliance: "a", sessionGeneration: 1, seq: 1 };
  let signIn = 0;
  const { Server } = loadServer({ "GET /v1/workspaces": refused(503, { error: unread }), "POST /v1/workspaces": refused(503, { error: unread }) }, { App, signInScreen: () => { signIn++; } });
  await assert.rejects(Server.workspaces.list(), e => e.status === 503 && e.message === unread);
  await assert.rejects(Server.workspaces.create("Ops"), e => e.status === 503 && e.message === `Outcome not confirmed. ${unread}`);
  assert.equal(signIn, 0, "a 503 opened sign-in");
  assert.equal(App.me?.id, "sam", "a 503 dropped the person");
  for (const [at, line] of [...app.matchAll(/signInScreen\("Your session ended[^\n]*/g)].map(m => [m.index, m[0]]))
    assert.match(app.slice(Math.max(0, at - 120), at), /status\s*===\s*401/, `app.js ends a session on more than a 401: ${line}`);
  const start = app.slice(app.indexOf("async function start()"));
  assert.ok(start.indexOf("r.unavailable") > 0 && start.indexOf("r.unavailable") < start.indexOf("signInScreen("), "app start opens sign-in when the server could not answer");
  assert.match(bridge, /r\.status >= 500\) return \{ unavailable/, "the browser's restore reads a 503 as signed out");
  const restore = read("desktop/src-tauri/src/lib.rs").split("async fn restore(")[1].split("\n}\n")[0];
  const fault = restore.slice(restore.indexOf("is_server_error()"));
  assert.ok(restore.includes("is_server_error()") && !fault.slice(0, fault.indexOf("Ok(r) =>")).includes("clear_token"), "the app's restore clears the session on a server fault");
}

// A read answered after the person, the server or the view changed belongs to no view on screen.
{
  const App = { me: { id: "sam" }, appliance: "a", sessionGeneration: 1, seq: 1 };
  const { Server } = loadServer({ "GET /v1/workspaces": () => { App.seq++; return { workspaces: [] }; }, "POST /v1/workspaces": () => { App.seq++; return { id: "w" }; }, "GET /v1/stops": () => { App.seq++; return { stops: [] }; } }, { App });
  await assert.rejects(Server.workspaces.list(), e => e.cancelled === true && e.message === "This view changed.");
  assert.deepEqual(plain(await Server.workspaces.create("Ops")), { id: "w" }, "a change's answer was dropped with the view");
  assert.deepEqual(plain(await Server.stops.list({ background: true })), { stops: [] }, "a poll's answer was dropped with the view");
  const { Server: still } = loadServer({ "GET /v1/workspaces": { workspaces: [1] } }, { App });
  assert.deepEqual(plain(await still.workspaces.list()), { workspaces: [1] });
}

// A poll is sent as it is: no step-up, no offer.
{
  let asked = 0;
  const { Server } = loadServer({ "GET /v1/admin/standing": stepUpRefusal() }, { stepUp: async () => { asked++; return true; } });
  await assert.rejects(Server.admin.standing({ background: true }), e => e.status === 403);
  assert.equal(asked, 0, "a background poll opened the step-up sheet");
}

console.log(`server check passed: ${table.length} calls and the error policy`);
