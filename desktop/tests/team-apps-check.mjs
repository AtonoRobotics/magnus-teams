import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

async function connect(returnedState) {
  const calls = [], hosts = [], notices = [];
  const app = { name: "crm", title: "CRM", state: "available", roles: [], signs_in: true, ready: true };
  const row = { dataset: { teamApp: "crm" }, querySelectorAll: selector => selector === "input:checked" ? [{ value: "member" }, { value: "owner" }] : [{ disabled: false }] };
  const sandbox = {
    enc: encodeURIComponent, esc: String, teamRoleLabel: String, Team: { home: { name: "Launch" } },
    document: { createElement: () => { const host = { remove() {} }; hosts.push(host); return host; } },
    $: () => ({ append() {} }), toast: m => notices.push(m), onThisComputer: () => false,
    connectScreen: async terms => { assert.match(terms.title, /Launch/); return true; },
    api: async (method, path, body) => {
      calls.push({ method, path, body });
      if (path.endsWith("/apps")) return [app];
      if (path.endsWith("/terms")) return { title: "CRM", terms: "version" };
      if (path.endsWith("/sign-in")) return { state: "expected", authorize_url: "https://crm.example/authorize" };
      if (path.endsWith("/complete")) app.state = "connected";
      return {};
    },
    Magnus: {
      loopbackOpen: async () => "http://127.0.0.1:1234/callback", openExternal: async () => {},
      loopbackWait: async () => ({ state: returnedState, code: "code", iss: "https://crm.example" }),
    },
  };
  vm.runInNewContext(readFileSync(new URL("../ui/team-apps.js", import.meta.url), "utf8"), sandbox);
  await sandbox.teamApps({}, "workspace");
  await hosts[0].onclick({ target: { closest: s => s === "[data-team-connect]" ? {} : s === "[data-team-app]" ? row : null } });
  assert(calls.every(c => c.path.startsWith("/v1/workspaces/workspace/")), "a workspace connected a personal account");
  const grant = calls.find(c => c.path.endsWith("/connectors"));
  assert.deepEqual(Array.from(grant.body.roles), ["member", "owner"]);
  return { calls, notices };
}
assert((await connect("expected")).calls.some(c => c.path.endsWith("/sign-in/complete")));
const mismatch = await connect("another-flow");
assert(!mismatch.calls.some(c => c.path.endsWith("/sign-in/complete")));
assert(mismatch.notices.some(s => s.includes("another request")));
console.log("Workspace OAuth uses workspace routes, selected roles and a bound callback.");
