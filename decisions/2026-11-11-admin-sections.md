# The Admin sections: Overview, People, Connectors, Rules, Security, Operations

Spec rev 141 (Administration) moves what the console did into the app's Admin section. These six are built on the guard and step-up in `decisions/2026-11-11-admin-in-the-app.md`: server code in `crates/magnus/src/admin_app/` (`overview.rs`, `people.rs`, `remove.rs`, `roles.rs`, `connectors.rs`, `rules.rs`, `settings.rs`, `security.rs`, `operations.rs`, `invite_page.rs`), pages in `desktop/ui/admin-*.js`. Where a control route already did the work, the app mounts it in-process (`mount::control`), so a change has one implementation and one audit entry. Agents, Models, Spend and the kill switch levels are built beside these and are not part of this decision.

## Roles

"Owner, admin, member and guest (read and chat only)." A person's server role is kept in `Ev::RolesSet`, and the people directory's own admin or member is kept in step with it: owner and admin are directory admins, member and guest are members. Only an owner grants or takes away the owner role, or removes an owner. The directory's guard that the last admin cannot be demoted still applies, under the server's own guard that the last owner cannot be demoted, turned off or removed ("the last owner cannot be removed"). Both refusals come back as 409 with words the page shows.

A guest's limit is enforced where every action is checked, not in the page: `person_rules` puts a block on every boundary action first for a guest (`kernel:guest_reads_and_chats_only`), so a guest's agent can read and chat and nothing it does leaves the computer. Why there: the boundary is the one place no route can miss.

## Removing a person

"Removing a person transfers or deletes their projects and agents and deletes their data." The admin chooses transfer (to a named person) or delete. The team projects they own and the team agent's routines they set up go to that person, or are deleted. A workspace they own with others in it always passes to the named person, even when the admin chose delete, because a workspace with members is never left without an owner; removal is refused with `workspace_owner` until someone is named. They leave every workspace. Their personal projects, their own agent's work, and their chats, tasks, files and memory are then deleted by the control route that deleted people before, audited as `user_deleted`: a personal project holds what is theirs alone, so it is never handed to someone else. Someone cannot remove themselves. People from the config file can only be turned off, as before: the config, not the app, is where they are defined.

## Reading a person's data

"Every admin read of a person's data is logged, and the person is told." The page says so before showing anything, and reads only when the admin chooses what to open. The server writes the audit entry (`person_data_read`) first and then calls the control route that tells the person who looked and at what. If the audit write fails, nothing is read.

## Workspace rules and Auto Review

A workspace rule is added to every person's own rules when their rules are compiled (`Kernel::rules_for`). The cap needs no new logic because the existing order already gives it: a block wins over an ask, and an ask wins over an allow. So a workspace block or ask first beats anyone's own allow, and a workspace allow does not override anyone's own block. Each rule names "the workspace's rule" when it is the reason. The rules and the Auto Review switch are kept in a new event kind, `AdminSetting`, because they are the workspace's and not one person's. Auto Review uses this setting when the workspace has made one, and the server's config otherwise. Limits: `WORKSPACE_RULES_MAX` (200) and `RULE_WORDS_MAX_CHARS` (500). The Cedar layers stay where they were, folded below the rules.

## Invitations

An invitation gives back a link to pass on, and is emailed when the admin asks and the server's mailbox is set up. The link opens a page on the server (`/invite`), outside the guard, where the person sets their password. The token is the invitation's own, and it is single-use and expires as before.

## Security

- The sign-in policy is the control route the console used; the page keeps fields it does not show, such as permanent lockout.
- The audit log can be filtered and exported as CSV. The server returns the file's name and text as JSON, so the step-up and errors work as for any other admin call. The export is itself audited.
- How long things are kept is shown, not set. It comes from the config's trash and backup windows, plus the log, which is the permanent record. Setting retention in the app would be a new promise about deleting people's data, with choices the spec does not settle, so it waits for the owner.
- Developer access lists each computer session where a person let a developer look, with whether it is still open.

## Operations and Overview

- Updates and the desktop channel are the console's routes.
- Backups show the in-memory restore check of the latest backup, whether the copy off the server failed, and a button to back up now.
- Computers show live against capacity, through a new computerd request, `Capacity`. A computerd that predates it answers with an error, and the page says the service is not answering, until computerd is released.
- Health shows the services, load, memory, and disk, with an alert at `DISK_ALERT_FRACTION` (85%).
- Overview shows what is running, computers against capacity, and what needs an admin, with each item linking to the section that fixes it. Spend and stopped agents belong to other sections. They add a card through `registerOverviewCard(name, render)` in `admin-overview.js`, and the server's overview carries `spend: null` until Spend fills it.

## Connectors

The allow-list, limiting a connector to groups, the organisation's key (into the vault, never echoed), which workspaces have it connected, and who has connected it. Suspending a connector at once is the kill switch's connector level, and this page points there instead of keeping a second switch.
