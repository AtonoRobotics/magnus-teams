import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
const source = readFileSync(new URL("../ui/team-chat.js", import.meta.url), "utf8");
const draftCode = source.slice(source.indexOf("const TeamDrafts ="), source.indexOf("const TeamChat ="));
const saved = new Map([["team-draft:w:c", "old secret"], ["theme", "dark"]]);
const storage = {
  get length() { return saved.size; }, key: i => [...saved.keys()][i],
  getItem: k => saved.get(k), removeItem: k => saved.delete(k),
  setItem() { throw new Error("draft text must never reach persistent storage"); },
};
const App = { appliance: "one", me: { id: "alice" } };
const make = () => vm.runInNewContext(`${draftCode}\nTeamDrafts`, { App, localStorage: storage });
const drafts = make();
drafts.set("w", "c", null, "private text");
assert.equal(drafts.get("w", "c"), "private text");
assert.equal(saved.has("team-draft:w:c"), false);
assert.equal(saved.get("theme"), "dark");
App.me = { id: "bob" };
assert.equal(drafts.get("w", "c"), "");
drafts.set("w", "c", null, "bob's text");
App.appliance = "two";
assert.equal(drafts.get("w", "c"), "");
drafts.set("w", "c", null, "unsent");
assert.equal(make().get("w", "c"), "", "restart recovered another session's content");
drafts.clear();
assert.equal(drafts.get("w", "c"), "");
const app = readFileSync(new URL("../ui/app.js", import.meta.url), "utf8");
assert.match(app, /TeamDrafts\.clear\(\)/);
console.log("Team drafts stay within the current account and session.");
