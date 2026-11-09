# Admin console

Status: superseded. The owner's "Magnus Admin Console Spec"
(https://claude.ai/artifact/3Ej2KCssZbFYfR4p1LEwGP) is what is built, as
written (owner, 2026-11-01: "all specs are for implementation not
proposal"), amended only by the owner's later rulings in
`spec-decisions.md` (among them no passkeys: admins sign in and step up
with their password and code). This file is kept for its history. Owner's request (2026-10-30):
"There should be an admin account that has access to everything in the
organization. It should be laid out to be admin and not the normal app.
Reconciling issues and solving problems should be easy to do in bulk."

This follows "each person's things are their own" (#33): the normal app
shows a person only their own tasks, schedules, chats and inbox, the owner
included. The admin console is the one place with reach across the
organization.

## Who

- A role, `admin`. The owner always has it. Others get it from Keycloak
  (a group mapped to the role, accounts step 4) or from the config.
- An admin uses the normal app for their own work, and opens the console
  from a separate entry, **Admin**, shown only to admins.

## Layout

Its own screen, not the app's: a left list of sections, and in each one a
dense table with filters, a checkbox per row, and a bar of bulk actions
that appears once rows are picked. Every bulk action first shows how many
items it will touch and what will happen, then asks to confirm.

| Section | Rows | Filters | Bulk actions |
|---|---|---|---|
| **Problems** (opens first) | Each kind of issue the appliance found, with a count | none | Fix all of one kind, or open its rows |
| **People** | Person, roles, last sign-in, running tasks, open items, schedules, spend (7 days) | role, signed in recently | Sign out everywhere; suspend their schedules |
| **Tasks** | Task, person, status, waiting on, age, cost, schedule | person, status, age, schedule, cost | Cancel, trash, resume |
| **Schedules** | Schedule, person, rule, ends, runs, skips, failures, paused, budget | person, paused, failing, no end | Pause, resume, stop, set an end, reassign |
| **Inbox** | Item, kind, person, task, age | kind, person, age | Decline, dismiss notices, reassign approvals |
| **Log** | Every admin action: who, what, which items, when | admin, action, date | none |

**Problems** lists what needs someone, each as a saved filter of the
tables above:
- schedules skipping, failing or paused;
- tasks waiting on someone for more than `stale_days` (default 3);
- tasks failed in the last 7 days;
- runs stopped by their budget;
- open items whose person cannot act on them (a person removed, or a role
  taken away);
- schedules and running tasks of people who are disabled (once accounts
  step 5 exists).

## What an admin can see

Proposed default: everything a person's own app shows about their tasks,
schedules and items, but chats only as titles and dates, with the words
opened per conversation and each opening logged. **Owner's call:** full
reading of everyone's chats, titles only, or this middle way.

## Every admin action is a record

- Each action is an event with `actor: "admin:<person>"` and the reason the
  admin typed (optional), in the Log section.
- The affected person gets one inbox notice per bulk action ("An admin
  stopped 3 of your schedules"), unless the owner turns that off.
  **Owner's call:** notify by default, yes or no.

## API

- `/v1/admin/...`, refused without the `admin` role. The normal routes keep
  their per-person rule unchanged.
- Bulk actions take a list of ids or a filter, have a `preview: true` form
  that returns what would change, and answer per item (done, or why not:
  a running task cannot be trashed, and so on).

## Build order

1. The role, the API with preview, the Log, and Tasks and Schedules tables
   with their bulk actions (enough to clean up today's case in two clicks).
2. Problems.
3. Inbox and People.
4. Hooks into accounts: disabled people, Keycloak group to `admin`.

## Tests

Real appliance, real browser, as for everything else:
- A person who is not an admin gets 403 on every admin route, and never sees
  Admin.
- An admin stops 50 schedules from three people in one action. The preview
  count matches, each person gets one notice, and the Log has the action.
- A Problems row for a skipping schedule appears, and Fix all clears it.
- A bulk trash that includes a running task trashes the rest and names the
  running one.

## Owner's calls

1. What an admin can read of other people's chats: everything, titles only,
   or titles with logged opening (proposed).
2. Whether affected people are told of each admin action (proposed: yes).
3. Whether admin is a separate account or a role on a person (proposed: a
   role, so the owner signs in once).
