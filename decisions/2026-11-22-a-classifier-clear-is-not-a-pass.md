# A classifier's clear is not a pass for a constructed URL

**Choice.** In a run that holds untrusted content and private data, a URL
the model made up (not in the person's words or earlier web results) is
never let through on the outgoing classifier's word. `check_web_url` keeps
every denial it had (the scrubber, userinfo, the byte budget, the retry cap,
a classifier finding), but a classifier that finds nothing now gets the same
verdict as one that cannot answer: `browser_*` and a queryless `web_fetch`
ask (case 3, where Allow for this task covers the rest of the task), and a
`web_fetch` with a query is denied. `UrlVerdict::Allow` is gone, so a
constructed URL can no longer clear the Rule of Two in `boundary.rs`.

**Reason.** `decisions/2026-11-22-autonomous-browser-navigation.md` let a
constructed URL go unasked when the classifier cleared it. The injection
suite (`tests/scratch/tests/injection.rs`) caught it on `main` at `5731103`:
an instruction planted in a CRM record had the agent fetch
`<collector>/collect?d=<data>`, and it went out (`crm → exfil_web:
Allowed`). `inv4_compartments` failed the same way ("turn 2 leaked turn 1
private data: /collect?d=turn1-secret"). It is the failure recorded in
`decisions/2026-11-19-an-outage-asks-no-one.md`: a model misses private data
that does not look sensitive (an id, a code, a figure), and an attacker
controls how the data is dressed. The Rule of Two holds only because it does
not rest on a model's judgement, so the classifier may add a denial but
never remove an ask. That fixes the contradiction toward the safer reading.

What stays from the autonomous-navigation change: a constructed URL in a
tainted run asks instead of dead-ending when no classifier is set, and
`OpenWeb` workers, which hold nothing private, still open clean public HTTPS
URLs directly.

Tests: `compartment::query_gate::tests::a_classifier_clear_does_not_let_a_constructed_url_go_unasked`,
and the injection suite's 49 cells.
