# Handle things like this on an approval card; Allow always beside grants; what delegate = false costs

The owner's standard (2026-11-19) is to protect the person, their data and
Magnus as autonomously as possible with the least asked of the person. The
approvals review (`docs/proposals/2026-11-19-approvals-review.md`, "Make
what already exists visible" and item 10) found three things the person
could not see or reach. Each follows from that standard and from decisions
already made, so they are decided here.

**"Handle things like this" on an approval card.** Grants (the person's
standing authority over a kind of their work,
`2026-11-06-235-standing-authority.md`) were reachable only from a task
proposal or a grant card, both retired since. An ask about what cannot be
undone can be allowed for one action or one task, never always
(`2026-11-19-allow-covers-the-task.md`), so a person who trusts Magnus with
an irreversible kind of work had no way to stop being asked about it.

- Offered (`asks::handle_offered`) on a card where Allow always is not
  offered, for an ask Auto Review raised (case 1, the reviewer's flag
  included), about what cannot be undone (`irreversible`), in an app
  (`app__tool`) that is one of the person's, to the person whose work it
  is. Never a payment (a grant never covers spending money), case 3 (a
  grant never covers the kernel's own asks), an ask-first rule (it wins
  over any allow), or an action outside an app (a program on their
  computer, the web): no grant could cover those, so offering it would
  promise what it cannot do. The person's apps are checked where they are
  known (`ask_grants::handle_prefill`, in `GET /v1/inbox`); the card drops
  the offer otherwise.
- It opens the existing grant form (app.js `grantForm`, the one the
  proposal card's Handle and Settings use) in place on the card, starting
  from the task's title, the action's app checked, "Delete or commit me to
  anything" lifted, "Send on my behalf" lifted when the action reaches
  beyond the person (audience not private, as `policy::grant_cedar` counts
  a send), a week, and the server's limit for one task (as
  `2026-11-09-server-task-budgets.md` set Handle's budget). The person
  changes any of it before choosing.
- Choosing it is `POST /v1/inbox/{id}` with `decision: approve`,
  `scope: handle` and `grant: {title, apps, allow, period, max_usd}`. The
  fields are checked as any grant's are (`grant_fields`; 400 with the
  reason), and in one write (#194) the grant is set (`grant_set`, the event
  `set_grant` emits, now shared), the card resolves with scope `handle`, and
  an approval of scope `task` allows the waiting action and the rest of
  this task, as Allow for this task does. A title left as the card offered
  it is the task's words and carries the run's taint, as a grant card's
  did; one the person wrote is theirs.
- Later tasks are covered only as any grant covers them: tasks started
  under it (`create_task` with `grant`), within its budget, where the
  person could approve the risk themselves (`what_covers`,
  `covering_grant`). The chat's system context already lists each live
  grant (`grant_section`), so Magnus starts matching work under it. The
  current task is not moved under the grant: it was not started under one,
  and its Allow for this task already covers it.

**Allow always beside grants.** Settings, Data and privacy, "What Magnus may
do on its own" now lists, beside the grants, the person's accepted rules
that allow (every Allow always among them: "Always allow: …"), with what each
matches and how often it was used, and one-tap Remove (no confirmation:
removing only makes Magnus ask again). It uses the existing rules API
(`GET /v1/rules`, the caller's own; `DELETE /v1/rules/{id}`, 404 for anyone
else's); no route was added. Rules that ask or block, words alone, and
suggestions not yet accepted stay on the Rules page only.

**`[auto_review] delegate = false` says what it costs (item 10).** The
switch is shown in Admin › Rules ("Auto Review clears actions on its own for
the workspace"). Its help now says that off, every action Auto Review would
allow becomes an approval card for the person unless something they set up
covers it, so each review is asked twice. It is not documented elsewhere.

Tests: `asks_tests::handle_is_offered_where_allow_always_is_not` (and the
card's offers in `the_card_is_one_sentence_three_answers_and_no_reasoning`),
`ask_grants::tests::handling_an_ask_sets_the_grant_and_allows_the_task`
(needs the test Postgres), `desktop/tests/asks-check.mjs` (the form in
place, its answer carrying the grant), `desktop/tests/allow-rules-check.mjs`
(new, in the fast tier) and `desktop/tests/admin-rules-check.mjs`. Not run
in the installed app.
