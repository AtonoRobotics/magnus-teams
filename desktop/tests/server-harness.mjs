// server.js on an answer table in place of the host's transport (bridge.js's
// `Magnus.api`): the page's one client of its server, run without a server.
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
export const serverSource = readFileSync(join(here, "../ui/server.js"), "utf8");

/** An error as the transport throws it. @param {number} status @param {any} body */
export const refused = (status, body) => Object.assign(new Error(body?.message || body?.error || `request failed (${status})`), { status, body });

/** Load server.js with the transport answering from `answers`, keyed
 * "METHOD path": a value, an Error (thrown), a list (one per call, in turn)
 * or a function of the body. `globals` adds names the page would have
 * (App, stepUp, offerMoreSpend, window).
 * @param {Record<string, any>} [answers] @param {Record<string, any>} [globals] */
export function loadServer(answers = {}, globals = {}) {
  /** @type {[string, string, any][]} */
  const calls = [];
  const ctx = {
    console,
    ...globals,
    Magnus: {
      api: async (m, p, b) => {
        calls.push([m, p, b]);
        let a = answers[`${m} ${p}`];
        if (Array.isArray(a)) a = a.length > 1 ? a.shift() : a[0];
        if (a instanceof Error) throw a;
        return typeof a === "function" ? a(b) : (a ?? {});
      },
    },
  };
  vm.createContext(ctx);
  vm.runInContext(serverSource, ctx);
  return { Server: vm.runInContext("Server", ctx), calls, ctx };
}
