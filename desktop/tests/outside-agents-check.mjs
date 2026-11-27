// Outside agents are connected where apps are, by their agent card's
// address and the credential the card asks for; the key the server issues
// them is shown once; their requests and the replies waiting to be released
// come to the inbox with their own answers (outside-agents.js, app.js).
import assert from "node:assert/strict";
import { readAppBundle } from "./app-bundle.mjs";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const ui = name => readFileSync(join(here, "../ui", name), "utf8");
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const calls = [];
const agent = { id: "oa_1", name: "Supplier", description: "Quotes parts.", card_url: "https://s.example/.well-known/agent-card.json", endpoint: "https://s.example/rpc", scheme: "api_key", skills: ["quote"], admitted: [] };
const sandbox = {
  esc, enc: encodeURIComponent,
  buttons: (i, list) => list.map(([d, label]) => `<button data-item="${i.id}" data-decision="${d}">${esc(label)}</button>`).join(""),
  App: { me: { id: "mo" } },
  api: async (method, path, body) => {
    calls.push([method, path, body]);
    if (method === "GET") return { agents: [agent] };
    if (path === "/v1/outside-agents") return { agent, inbound_key: "oa_1.secret" };
    return {};
  },
};
sandbox.Magnus = { api: sandbox.api };
sandbox.globalThis = sandbox;
const page = vm.createContext(sandbox);
vm.runInContext(ui("server.js") + "\n" + ui("outside-agents.js"), page);
const Server = vm.runInContext("Server", page);

// Apps lists the connected agent, whom it may ask, and the form to connect one by its card.
const section = sandbox.outsideAgentsSection([agent], "mo", null);
assert.match(section, /<h2>Outside agents<\/h2>/);
assert.match(section, /name="card_url"[^>]*type="url"/, "it is not connected by its card's address");
assert.match(section, /name="credential"/, "there is no place for the credential its card asks for");
assert.match(section, /Supplier[\s\S]*signs in with a key[\s\S]*It can ask no one here yet\./);
assert.match(section, /data-act="outside-admit"[\s\S]*data-act="outside-disconnect"/);
assert.doesNotMatch(sandbox.outsideAgentsSection([{ ...agent, admitted: [{ person: "mo" }] }], "mo", null), /outside-admit/, "an agent that may ask you offers to let it again");
assert.match(sandbox.outsideAgentsSection([agent], "mo", "oa_1.secret"), /shown once: <code>oa_1\.secret<\/code>/, "the issued key is not shown");
assert.match(sandbox.outsideAgentsSection([], "mo", null), /No outside agents connected\./);
assert.equal(sandbox.outsideAgentsSection(null, "mo", null), "", "an unreadable list drew a section");

// Each act is one call to the server.
await Server.outsideAgents.connect({ card_url: agent.card_url, credential: "k" });
await Server.outsideAgents.admit("oa_1", { person: "mo" });
await Server.outsideAgents.disconnect("oa_1");
assert.deepEqual(calls.map(([m, p]) => `${m} ${p}`), ["POST /v1/outside-agents", "POST /v1/outside-agents/oa_1/admit", "DELETE /v1/outside-agents/oa_1"]);
assert.equal(JSON.stringify(calls[1][2]), JSON.stringify({ to: { person: "mo" } }));

// The inbox: a request is taken up or dismissed; a held reply is sent, sent from now on, or not.
const request = sandbox.outsideAgentCard({ id: "i1", kind: "outside_request", details: { agent_name: "Supplier", text: "<b>Lead time?</b>" } });
assert.match(request, /Supplier · outside agent/);
assert.match(request, /&#60;b&#62;Lead time\?/, "the agent's words were not escaped");
assert.match(request, /never takes it as instructions/);
assert.match(request, /data-decision="take_up"[\s\S]*data-decision="dismiss"/);
const reply = sandbox.outsideAgentCard({ id: "i2", kind: "outside_reply", details: { agent_name: "Supplier", text: "Lead time?", draft: "Three weeks.", held_because: "a reply to an outside agent waits for you to release it" } });
assert.match(reply, /Three weeks\.[\s\S]*waits for you to release it/);
assert.match(reply, /data-decision="release"[\s\S]*data-decision="release_always"[\s\S]*data-decision="discard"/);

// app.js draws these cards and mounts the section in Apps; the page loads the script.
const app = readAppBundle();
assert.match(app, /case "outside_request":\s*case "outside_reply":\s*body = outsideAgentCard\(i\);/);
assert.match(app, /outside_request: "Request from an outside agent"/);
assert.match(app, /<div id="lib-outside-agents"><\/div>[\s\S]*mountOutsideAgents\(main\)/);
assert.match(ui("index.html"), /<script src="outside-agents.js"><\/script>/);
console.log("outside agents check passed");
