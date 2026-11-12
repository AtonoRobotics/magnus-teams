# The tester program: joined only by accepting the Tester Terms

The owner, 2026-11-11: "Create tester group and share full data with
developer have terms and condition that must be accepted to joint tester
program which explains anonymous usage data will be collected."

Choice:

- A person joins only by accepting the Tester Terms in their own app
  (Settings › Agreements, or the invitation an admin sends, shown when they
  next open Magnus). Joining is `legal` event `tester_joined` with the version
  of the Tester Terms shown; leaving is `tester_left`, naming the version they
  had accepted; an admin's invitation is `tester_invited`, and declining it
  `tester_declined`. An admin cannot make someone a tester.
- The group is `testers` (`legal::TESTERS_GROUP`). `Kernel::person` puts a
  person in it exactly while they are a tester of the Tester Terms as they read
  now, and takes it out of whatever the config or an identity provider says,
  so the group cannot be joined any other way.
- For a tester, developer views show their work in full
  (`2026-11-11-anonymous-developer-data.md`). Leaving applies from then on:
  their usage-data answer applies again.
- When the Tester Terms change (their version is their date and a digest of
  the text as this server renders it), a tester is not seen in full until they
  accept the new text; the app asks them, with leaving as the other answer.
- Admin › People lists testers and invitations (`/v1/admin/testers`, behind
  the admin guard; an invitation takes the step-up like other changes).

The Tester Terms say plainly: what developers see (conversations, tasks,
tool inputs and results, files tasks touch, web addresses reached, screens of
the Magnus computer during tasks); that anonymous usage data is collected too;
who the developers are (the people the operator, named from `[legal]
operator`, gives the developer role); that developers read the data where it
already is and the server makes no copy; that anything a developer copies out
is kept at most `[legal] tester_copy_days` days (the code keeps no such copy,
so this is the operator's promise and a required config value); that leaving
is in Settings at any time and what was already seen cannot be unseen; and
that Magnus is test software that may fail.

No openly licensed beta or tester agreement fit (searched 2026-11-11: the
templates found were commercial or unlicensed), so the Tester Terms are new
text on the same Basecamp CC BY 4.0 base, attributed as such.
