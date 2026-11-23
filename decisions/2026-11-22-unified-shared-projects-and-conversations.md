# Unify Magnus Teams into Shared Projects, Shared Conversations, and People

Date: 2026-11-22

## Choice
Retire the separate 3-island `#/team/:id` Slack-clone shell (`.team-shell`, `.team-rail`, `.team-side`, `.nav-teams`, video-call chrome, pinned-only toggle, archive room, `#channel` prefixes, nested thread drawers, and second `⌘K` palette) and unify multiplayer collaboration directly inside the single Magnus Stage + Bento + Canvas + Composer shell as **Shared Projects**, **Shared Conversations**, and **People**.

Keep the existing `/v1/workspaces/...` backend isolation, audit, shared computer, routines, delegations, missions, release airlock, whispers, and `tests/invariants/tests/inv8_team.rs` completely intact.

## Key details
1. **Unified sidebar & rail:** Removed `.nav-teams` from the left rail. Merged private and shared projects into one `Projects` tree (shared projects carry a `Shared` badge and expand to list their shared conversations), and replaced the separate `Teams` section with a lightweight `People` section for 1:1/group DMs.
2. **One Stage, one Canvas, one `⌘K`:** Registered `#/team/:ws(/.*)?` with `workspace = true` so shared conversations mount directly as `#main > section.conv` and share the unified `#detail` Canvas and `⌘K` jump palette. Legacy `#/team/:ws/{people,p/:id,activity,search}` deep links resolve seamlessly into the shared conversation with the matching Canvas view open.
3. **Minimal work-focused messenger:** Kept `@mentions`, member faces, **Whispers** (`Only my Magnus sees this`), and reactions. Default Magnus behavior in shared conversations is `@Magnus` mentions with a quiet header toggle for `Auto-join` (`mentions` ↔ `when_helpful`). Bound the header Computer chip to the workspace's Shared Computer (`home.computer`).
4. **Native Bento & Context-Aware Canvas (`desktop/ui/shared-canvas.js`):** Private **Release Airlock** drafts waiting on the current user appear as urgent `needs` Bento tiles above the composer (strictly owner-only) alongside active **Missions** (`run`). The header `Views` popover adapts to shared conversations to list **Missions, Tasks, Files, Routines, Knowledge, and Project settings**.
5. **Clean-room `Share` promotion:** Private conversations and private projects offer a quiet **Share** button (`data-conv-share` / `data-project-share`) that opens a clean-room dialog defaulting to an editable summary/deliverable only (or explicitly checked messages), never silently exposing private history.

## Reason
Users do not want a separate Slack clone bolted onto their autonomous digital worker. Treating shared work as an extension of the same Projects, Conversations, Bento, and Canvas primitives removes duplicated chrome, preserves strict private-vs-shared isolation (`inv8_team.rs`), and makes moving from solo work to team work effortless and safe.
