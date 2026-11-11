// The computer's installed apps and the person's own desktop (spec: Installing
// and using apps; Work on the user's desktop). The apps the agent installed
// are listed from the manifest; this computer's mode (ask every time, allow,
// never) and folders can be changed after connecting, not only at first; and
// while the person uses the mouse or keyboard the page says Magnus waits.
import assert from "node:assert/strict";
import { load, settle } from "./fake-screen.mjs";

let known = [];
const { page, sandbox } = load({ reply: (method, path) => (path === "/v1/local/computers" ? known : {}) });

// Apps, from the manifest, with where each came from and what it may reach.
const apps = sandbox.computerApps({ apps: [{ name: "gimp", version: "2.10", source: "apt: <archive>", hosts: ["*.gimp.org"] }], replay_error: null });
assert.match(apps, /data-app="gimp"/, "an installed app is not listed");
assert.match(apps, /2\.10/);
assert.match(apps, /apt: &#60;archive&#62;/, "the source was not escaped");
assert.match(apps, /\*\.gimp\.org/, "what the app may reach is not shown");
assert.match(apps, /Magnus installs an app itself/);
assert.match(sandbox.computerApps({ apps: [] }), /No apps installed yet/);
assert.match(sandbox.computerApps({ apps: [], replay_error: "hash mismatch" }), /Putting the apps back failed: hash mismatch/);

// This computer, connected: its mode and folders can be changed in place.
const started = [];
sandbox.Magnus.localState = async () => ({ running: true, name: "desk", paused: false });
sandbox.Magnus.localStart = async how => { started.push(how); };
known = [{ computer: { id: "l1", name: "desk", folders: ["/home/a/Projects"], mode: "allow" }, online: true, in_use: true }];
page.body.innerHTML = await sandbox.thisComputerSection();
const html = page.body.innerHTML;
assert.match(html, /<option value="allow" selected>/, "the mode in force is not the one shown");
assert.match(html, /Ask me every time/);
assert.match(html, /\/home\/a\/Projects<\/textarea>/, "the granted folders are not shown to change");
assert.match(html, /data-act="local-change"/, "the mode cannot be changed after connecting");
// Yielding: while the person is at the machine, Magnus waits, and they can let it go on.
assert.match(html, /You are using it/);
assert.match(html, /Magnus waits while you use the mouse or keyboard/);
assert.match(html, /data-act="local-resume"/);
assert.match(html, /data-mode>Allowed</, "the person's own mode is not shown");
// A team's or admin's ceiling makes it stricter, and the page says the one that holds.
known = [{ ...known[0], in_force: "ask" }];
assert.match(await sandbox.thisComputerSection(), /data-mode>Asks each time: your team or admin allows no more than this</);

page.els["#lc-mode"].value = "never";
page.els["#lc-folders"].value = "/home/a/Projects\n/home/a/Docs\n";
const changed = await sandbox.thisComputerAction({ dataset: { act: "local-change", name: "desk" } }, page.body);
await settle();
assert.equal(changed, true);
assert.equal(JSON.stringify(started), JSON.stringify([{ name: "desk", mode: "never", folders: ["/home/a/Projects", "/home/a/Docs"] }]),"the change did not restart this computer's connection with it");
console.log("computer desk ok");
