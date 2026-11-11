// The computer's desktop follows the person's app theme (spec rev 145, The
// desktop: "light and dark versions that follow the person's app theme").
// The app tells the server when its look and the person's computer differ,
// once, and never for a team's computer.
import assert from "node:assert/strict";
import { load, settle } from "./fake-screen.mjs";

const { calls, sandbox } = load({ reply: () => ({}) });
const puts = () => calls.filter(c => c.startsWith("PUT "));
sandbox.App.computers = [
  { id: "c", owner: { person: "alice" }, role: { kind: "computer" }, theme: "dark" },
  { id: "t", owner: { workspace: "w" }, role: { kind: "computer" }, theme: "dark" },
  { id: "b", owner: { person: "alice" }, role: { kind: "identity", site: "x" }, theme: "dark" },
];

sandbox.Theme = { look: { mode: "dark" } };
sandbox.followDesktopLook();
await settle();
assert.deepEqual(puts(), [], "the computer was told a look it already has");

sandbox.Theme.look.mode = "light";
sandbox.followDesktopLook();
sandbox.followDesktopLook();
await settle();
assert.deepEqual(puts(), ["PUT /v1/computers/c/theme"], "the person's computer was not told once of the light look, or another was told");

console.log("desktop look ok");
