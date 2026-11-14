# The desktop page has one client of its server

**Choice.** `desktop/ui/server.js` (`Server`) is the page's one client of its server. Each request is a named call grouped by what it acts on: `Server.schedules.remove(id)`, `Server.tasks.cancel(id)`, `Server.admin.standing()`, `Server.workspaces.addTask(ws, body)`. It builds every path, encodes every id and holds the one error policy. It sends through the host's transport, `Magnus.api` in `bridge.js`: the app's Rust side, or the browser's fetch. Checks put an answer table in that place (`desktop/tests/server-harness.mjs`).

- About 280 call sites in 30 scripts each spelled out a method, a path, the encoding and a body. The same route was built in several places, and personal-chat.js sent a schedule's id unencoded where app.js encoded it.
- The error policy was in three places: the app bridge (426), app.js `api()` (402, step-up, "Outcome not confirmed", a view that changed) and admin.js `adminApi` (step-up again). It is now in `Server.call` alone. `api()` and `adminApi()` are gone: every script asks through a named call. `Resources.read` in app.js keeps its path-keyed cache and reads with `Server.call("GET", path)`.
- `desktop/tests/server-check.mjs` sends every named call through a fake transport and checks its method, path and body. It fails when a named call is missing from its table. It also checks the error policy. The fast tier runs it.

**Behaviour kept, and what changed.**
- `adminApi` is gone. `api()` already asked for the step-up and sent the change again, so `adminApi` asked a second time only when the server refused a change for a step-up straight after one was confirmed. Now the sheet opens once per change, and a second refusal is shown as the answer.
- The change sent again after a step-up goes through the rest of the policy. A lost reply to it now says "Outcome not confirmed", as a lost reply to any change does. Before, it showed the bare error.
- 426 moved from the app bridge to `Server.call`. The server sends 426 only to an app that names its version, so the browser sees no change.
- A poll (`{ background: true }`: the kill switch banner and the admin standing at sign-in) is sent as it is, as admin.js did with `Magnus.api`. It gets no step-up, no spend offer and no "This view changed".

**The installed app sends PATCH.** The Tauri `api` command sent only GET, POST, PUT and DELETE. Changing a role, a connector and a provider failed in the installed app and worked in a browser. The methods are one list (`desktop/src-tauri/src/methods.rs`). `desktop/tests/methods-check.mjs` fails when the page names a method that list lacks.

**Reason.** A route, its encoding and what a failure means are each decided in one place, and tested as a table with no server. A script says what it wants done, not how the request is spelled. A new method or route cannot work in the browser and fail in the app without a check failing.
