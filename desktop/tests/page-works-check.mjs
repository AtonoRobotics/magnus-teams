// The page tells the desktop app it works only when its scripts loaded
// without an error and its first screen is drawn. The app installs a
// downloaded release on its own when no page says so (update.rs,
// watch_page): 0.2.6-test.380's page stopped while loading, drew an empty
// window, and so never offered "Restart to update" for the release that
// mended it.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const ui = join(dirname(fileURLToPath(import.meta.url)), "../ui");
const bridge = readFileSync(join(ui, "bridge.js"), "utf8");

/** bridge.js in a window of its own, in the desktop app; `before` runs
 * before DOMContentLoaded, as a later script failing to load would. */
function load(/** @type {(fire: (type: string, e: any) => void) => void} */ before) {
  /** @type {Record<string, Function[]>} */
  const on = {};
  const listen = (/** @type {string} */ t, /** @type {Function} */ f) => { (on[t] ||= []).push(f); };
  const unlisten = (/** @type {string} */ t, /** @type {Function} */ f) => { on[t] = (on[t] || []).filter(g => g !== f); };
  const fire = (/** @type {string} */ t, /** @type {any} */ e) => { for (const f of [...(on[t] || [])]) f(e); };
  /** @type {string[]} */
  const invoked = [];
  const window = {
    addEventListener: listen, removeEventListener: unlisten,
    __TAURI__: { core: { invoke: async (/** @type {string} */ cmd) => { invoked.push(cmd); } }, event: { listen: async () => () => {} } },
  };
  const document = { addEventListener: (/** @type {string} */ t, /** @type {Function} */ f) => listen(`doc:${t}`, f) };
  const ctx = vm.createContext({ window, document, atob, btoa, console });
  vm.runInContext(bridge, ctx, { filename: "bridge.js" });
  before(fire);
  fire("doc:DOMContentLoaded", {});
  return { Magnus: /** @type {any} */ (window).Magnus, invoked, fire };
}

{
  const { Magnus, invoked } = load(() => {});
  Magnus.pageWorks();
  assert.deepEqual(invoked, ["page_works"], "a page that loaded whole tells the app it works");
}
{
  const { Magnus, invoked } = load(fire => fire("error", { message: "filesView is not defined" }));
  Magnus.pageWorks();
  assert.deepEqual(invoked, [], "a page whose script failed while loading never says it works");
}
{
  const { Magnus, invoked } = load(fire => fire("unhandledrejection", { reason: new Error("boom") }));
  Magnus.pageWorks();
  assert.deepEqual(invoked, [], "a rejected promise while loading counts as a failed load");
}
{
  const { Magnus, invoked, fire } = load(() => {});
  fire("error", { message: "a later error, after the page loaded" });
  Magnus.pageWorks();
  assert.deepEqual(invoked, ["page_works"], "an error after the page loaded is the page's own business");
}

// Each first screen says so once drawn: the signed-in shell, sign-in, and
// the update-required screen.
const boot = readFileSync(join(ui, "app-boot.js"), "utf8");
assert.match(boot, /\n {2}shell\(\);\n {2}adminStart\(\);\n {2}Magnus\.pageWorks\(\);/, "boot says the page works right after drawing the shell");
assert.match(boot, /id="ur"><\/div><\/div>`;\n {2}Magnus\.pageWorks\(\);/, "the update-required screen says the page works");
assert.match(readFileSync(join(ui, "signin.js"), "utf8"), /id="si"><\/div><\/div>`;\n {2}Magnus\.pageWorks\(\);/, "the sign-in screen says the page works");
// The web page is the server's own version: it has no app to tell.
assert.match(bridge, /\n {2}pageWorks\(\) \{\},/, "the browser's bridge has pageWorks, doing nothing");
console.log("ok - the page tells the app it works only when it loaded whole and drew its first screen");
