# A Magnus admin supports accounts and the server, and reads no one's data

The owner, 2026-11-11: "Magnus admin provides support for Magnus user accounts, server, and server related things. Users are isolated and private parties unless they agree to other agreements or share in a team workspace. Anything put in a team workspace is public to that team. The team should be isolated from other teams. Team Admin owns the workspace." And earlier the same day: "users should be isolated from each other securely unless they choose to share something in teams. Then only what they share should be shared through teams. The user should still own it."

This replaces every spec sentence and decision that gave the server's owner or its admins access to people's data or to a team's content, owner's ruling 14 (2026-11-01: "by default a person's data is the organisation's") among them.

## What a Magnus admin keeps

Inviting people, password resets, turning accounts off, signing them out, removing people, the server's settings, the organisation's connectors and its policy and group layers, updates, backups, the kill switch, pausing a person's computer or agents, cancelling a person's task or pausing their schedule for the server's safety (the person is told, as before), spend limits, and making someone a Team Admin. Counts and health that carry no one's words stay: the Agents list (kind of work, who asked, spend), the Overview, the people list.

## What a Magnus admin loses

- **A person's data.** `POST /v1/admin/people/{id}/data/{what}` and the console's `/v1/people/{id}/data/{what}` are gone, and `read_person_data` refuses anyone but the person (`GET /v1/people/{id}/data/{what}` now answers the owner's own only). The data-access switch is fixed at private: `GET /v1/data-access` says `private: true`, and `PUT` with `false` is refused (409). `Ev::DataAccessSet` still replays and nothing reads it.
- **An agent's activity.** `GET /v1/admin/agents/{agent}/activity` is refused for anyone's agent but one's own, and for the team agent (a team's work is its team's).
- **Other people's task words in chat.** `list_tasks`, `get_task`, `list_schedules` and `get_schedule` still name other people's tasks and schedules to the owner and admins, so they can stop one, and carry none of their words: no summary, review, or what the run did (`controls::as_policy_allows`). No data read is recorded, because none happens.
- **Names that are a person's or a team's in Spend.** Admin › Spend no longer breaks spend down by project (a project's name is its person's or its team's) and shows a workspace's total and limit without its channels' names; a workspace's Team Admin sets channel limits in the workspace (`POST /v1/workspaces/{ws}/limit`, as before).
- **Another person's own rules** (`2026-11-11-a-persons-own-rules-follow-the-layers-above.md`).
- **Workspaces**: see `2026-11-11-team-admin.md`.
- The Admin section's "Their data" and "What it did" buttons are gone.

## Support that needs a person's data

There is no session in which a person grants a Magnus admin a look at their data; the only person-granted session is developer access to their computer (Security). So that capability is left out, not built as a way around the rule. Building it is a choice for later: a session the person starts, scoped, timed, logged and visible to them.

## Spec sentences that now differ (spec rev 145)

- Administration › People: "Every admin read of a person's data is logged, and the person is told." Now there is no admin read of a person's data.
- Administration › Agents: "An admin sees what an agent did, from the audit log. Seeing a person's content follows the person-data rule above." Now an admin sees the list of what each agent is running, without words, and not what an agent did.
- Administration › Spend: "Spend is broken down by person, agent, model and project." Now by person, agent and model.
- Administration › Spend: "A monthly limit for the server, a default limit per person, overrides for individuals, and limits per channel for the team agent." A channel's limit is set by its workspace's Team Admin; the admin keeps the server's, the defaults, the overrides and each workspace's total.
- Team shared workspace › Roles and Workspace experience › Creating and joining: see `2026-11-11-team-admin.md`.
- Administration: "An owner or admin sees an Admin section". Unchanged, but the section holds support tools only.

## Tests

`admin_app::sections_tests::no_admin_reads_a_persons_data` (the routes are gone or refused, the switch cannot open, others' task rows carry no words, Spend names no project or channel); `tests/scratch/tests/data_access.rs`, `relationship_memory.rs`, `reactions_disagreements.rs`, `content_walls.rs`, `chat_controls.rs` updated to the rule; `desktop/tests/admin-people-check.mjs`, `admin-agents-check.mjs` and `spend-check.mjs` check the buttons and tables are gone.
