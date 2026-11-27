// Fast-tier tests for UserAvatar and unified Stage + Canvas (team-rooms.js and
// Missions UI are retired).
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const ctx = { esc, enc: encodeURIComponent };
vm.createContext(ctx);
vm.runInContext(readFileSync(join(here, "../ui/user-avatar.js"), "utf8") + "\nthis.UserAvatar = UserAvatar;", ctx);
const { UserAvatar } = /** @type {any} */ (ctx);

// 1. UserAvatar: default initials, preset, and photo.
assert.match(UserAvatar.html("Bob Martinez", { kind: "default" }), />BM</);
assert.match(UserAvatar.html("Alice Chen", { kind: "preset", id: "aurora-indigo" }), /is-preset/);
assert.match(UserAvatar.html("Carol", { kind: "photo", blob_id: "img 1" }), /\/v1\/files\/img%201/);
assert.match(UserAvatar.pickerHtml("Bob", { kind: "default" }), /data-avatar-preset="aurora-emerald"/);

// 2. Retired team-rooms.js and Mission polling are gone.
assert.equal(existsSync(join(here, "../ui/team-rooms.js")), false, "team-rooms.js should be retired");
const teamsJs = readFileSync(join(here, "../ui/teams.js"), "utf8");
assert.doesNotMatch(teamsJs, /Server\.workspaces\.missions/, "teams.js must not poll retired missions endpoint");

// 3. Unified Stage + Canvas architecture: 3-island .team-shell and body[data-team] are retired; #/team/:ws mounts on the unified workspace shell.
const stageCss = readFileSync(join(here, "../ui/aurora-stage.css"), "utf8");
const teamsCss = readFileSync(join(here, "../ui/teams.css"), "utf8");
const avatarJs = readFileSync(join(here, "../ui/aurora-avatar.js"), "utf8");
assert.doesNotMatch(stageCss, /body\[data-team\]/, "aurora-stage.css has no separate body[data-team] frame override");
assert.doesNotMatch(teamsCss, /\.team-shell\b|\.team-rail\b|\.room-video-/, "teams.css has no 3-island .team-shell or call stage");
assert.match(teamsJs, /route\(\/\^#\\\/team\\\/\(\[\^\/\]\+\)\(\\\/\.?\*\)\?\$\/,[\s\S]*?,\s*true\)/, "#/team/:ws routes mount with workspace=true on the unified Stage + Canvas");
assert.doesNotMatch(avatarJs, /\.team-rail-head\s+\.room-orb|\.room-video-listening\s+\.room-orb/, "aurora-avatar.js has no retired .team-rail or call orb hooks");

console.log("team rooms check passed");
