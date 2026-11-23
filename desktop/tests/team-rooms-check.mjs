// Fast-tier tests for Phase 3 Team Rooms UI (user-avatar.js & team-rooms.js):
// - UserAvatar renders default initials, preset avatars, and photos safely;
// - TeamRooms strictly hides private drafts from non-owners and shows Share with Room to the owner;
// - Extensible Room slots (`headerActions`, `stageSlot`) mount future features like Video Call;
// - Member capability list shows advertised app names with avatars.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => `&#${c.charCodeAt(0)};`);
const ctx = { esc, enc: encodeURIComponent };
vm.createContext(ctx);
vm.runInContext(readFileSync(join(here, "../ui/user-avatar.js"), "utf8") + "\nthis.UserAvatar = UserAvatar;", ctx);
vm.runInContext(readFileSync(join(here, "../ui/team-rooms.js"), "utf8") + "\nthis.TeamRooms = TeamRooms;", ctx);
const { UserAvatar, TeamRooms } = /** @type {any} */ (ctx);

// 1. UserAvatar: default initials, preset, and photo.
assert.match(UserAvatar.html("Bob Martinez", { kind: "default" }), />BM</);
assert.match(UserAvatar.html("Alice Chen", { kind: "preset", id: "aurora-indigo" }), /is-preset/);
assert.match(UserAvatar.html("Carol", { kind: "photo", blob_id: "img 1" }), /\/v1\/files\/img%201/);
assert.match(UserAvatar.pickerHtml("Bob", { kind: "default" }), /data-avatar-preset="aurora-emerald"/);

// 2. Work panel strict privacy: Alice must NOT see Bob's private draft; Bob sees Share with Room.
const missions = [{
  id: "m1",
  goal: "Friday Board Update",
  complete: false,
  released_count: 1,
  total_lanes: 2,
  deliverable: "Q3 Pipeline: $4.2M",
  lanes: [
    { id: "l1", kind: "member_agent", owner: "alice", objective: "Q3 pipeline", status: "released", contribution: "$4.2M active" },
    { id: "l2", kind: "member_agent", owner: "bob", objective: "Release blockers", scope: "weekly_blockers", status: "waiting_release", private_draft: "SECRET_BOB_DRAFT" },
  ],
}];
const names = id => ({ alice: "Alice", bob: "Bob" }[id] || id);
const aliceHtml = TeamRooms.workPanelHtml(missions, "ws1", "alice", names);
assert.doesNotMatch(aliceHtml, /SECRET_BOB_DRAFT/, "peer must never see owner's private draft");
assert.doesNotMatch(aliceHtml, /Share with Room/, "peer must not get owner's Share button");
assert.match(aliceHtml, /Waiting for Bob to share/);

const bobHtml = TeamRooms.workPanelHtml(missions, "ws1", "bob", names);
assert.match(bobHtml, /SECRET_BOB_DRAFT/, "owner must see their own private draft");
assert.match(bobHtml, /Share with Room/, "owner must see Share with Room CTA");
assert.match(bobHtml, /Always share weekly_blockers/);

// 3. Extensible Room slots (e.g. Video Call plugin).
TeamRooms.registerSlot("headerActions", () => `<button data-join-call>Join Call</button>`);
TeamRooms.registerSlot("stageSlot", () => `<section data-video-stage>Video</section>`);
assert.match(TeamRooms.renderSlot("headerActions", {}), /data-join-call/);
assert.match(TeamRooms.renderSlot("stageSlot", {}), /data-video-stage/);

// 4. People & Capabilities list renders avatars and advertised apps.
const capsHtml = TeamRooms.peopleCapabilitiesHtml([
  { id: "alice", name: "Alice Chen", avatar: { kind: "preset", id: "aurora-indigo" }, apps: [{ name: "salesforce", title: "Salesforce" }] },
], "bob");
assert.match(capsHtml, /Alice Chen/);
assert.match(capsHtml, /Salesforce/);
assert.match(capsHtml, /is-preset/);

// 5. Unified Stage + Canvas architecture: 3-island .team-shell and body[data-team] are retired; #/team/:ws mounts on the unified workspace shell.
const stageCss = readFileSync(join(here, "../ui/aurora-stage.css"), "utf8");
const teamsCss = readFileSync(join(here, "../ui/teams.css"), "utf8");
const teamsJs = readFileSync(join(here, "../ui/teams.js"), "utf8");
const avatarJs = readFileSync(join(here, "../ui/aurora-avatar.js"), "utf8");
assert.doesNotMatch(stageCss, /body\[data-team\]/, "aurora-stage.css has no separate body[data-team] frame override");
assert.doesNotMatch(teamsCss, /\.team-shell\b|\.team-rail\b|\.room-video-/, "teams.css has no 3-island .team-shell or call stage");
assert.match(teamsJs, /route\(\/\^#\\\/team\\\/\(\[\^\/\]\+\)\(\\\/\.?\*\)\?\$\/,[\s\S]*?,\s*true\)/, "#/team/:ws routes mount with workspace=true on the unified Stage + Canvas");
assert.doesNotMatch(avatarJs, /\.team-rail-head\s+\.room-orb|\.room-video-listening\s+\.room-orb/, "aurora-avatar.js has no retired .team-rail or call orb hooks");

console.log("team rooms check passed");
