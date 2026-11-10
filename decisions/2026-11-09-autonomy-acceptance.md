# The five autonomy scenarios run as acceptance

Spec rev 133, "Autonomy is tested like safety": each release candidate's
acceptance runs five scenarios end to end, and a release that fails one does
not ship.

**The suite.** `tests/scratch/tests/autonomy.rs` holds exactly five tests, one
per scenario. Each starts a real server with the scripted model and fake apps,
and asserts on what the person sees:

1. A chat request (read the CFO's mail, save it in the notes app) is done in
   the turn: the note exists, the thread shows the message and the answer, and
   there is no task and no approval.
2. A one-minute schedule (`max_runs` 1) fires while nothing calls the server:
   the test waits on the model alone until the run and its outcome turn are
   over. The result and the agent's report are in `schedule-{id}`, which shows
   no message nobody wrote; the conversation it was set up in gets none of it.
3. A task under a grant fails. Its outcome returns as a turn in the thread,
   which retries twice (the default `[outcomes] max_retry_count`), each attempt
   acting under the same grant with no approval and no more than its budget;
   at the limit `create_task` is refused and the agent reports. The thread
   shows the person's message once and never the server's turn text.
4. The mail app pushes (`POST /v1/push/...`). The first push's read only
   learns the inbox; after a new message and a second push, the server reads
   at once, wakes a review naming "1 new in mail", and the review proposes a
   task citing the new message. It suggests rather than acts: mail is outside
   text, and after untrusted and private reads the Rule of Two refuses a
   background change and asks no one (the kernel tells the review to suggest
   instead). That floor is right, so the scenario takes the spec's
   "suggestion" branch.
5. Magnus's own review reads the inbox and proposes a task whose evidence
   resolves to that read. The proposal waits in the inbox; nothing starts.

Each test ends by checking that the plumbing stays hidden: the person's tasks
(reviews and polls excluded, as the app shows them), inbox and conversations
hold exactly what the scenario is about. The proactive loop is on in all five,
as it ships, so background reviews run beside each scenario and must leave no
trace the person sees.

The scripted model also plays the independent checks a boundary action meets:
the outgoing classifier says honestly what an action carries (a revenue figure
is private business data), and Auto Review allows what plainly serves the
request. Without them every app write in chat became an approval, because a
check that cannot answer asks the person.

**What it found, fixed with it.**
- *A failed attempt notified the person while the agent handled it.* A task
  whose outcome returns to its thread put "Failed: ..." in the inbox for every
  attempt, beside the agent's own report. Now a task started in a
  conversation that is still there (`outcomes::outcome_returns`) puts no
  notice of its own when it fails: the thread shows the result and the agent
  tells the person there (K24 holds through the thread). A task whose
  conversation is gone keeps its notice, and so does a failed scheduled run
  (`no_silent_failures.rs`): nobody is in a schedule's thread when it fails,
  so the safer reading keeps the inbox telling them.
- *The server's turn text leaked into the inbox.* At the retry limit, a
  refused `create_task` in an outcome turn raised "Not done: This turn is not
  a message from the person..." as something the person asked for. An outcome
  turn's refused retry is the limit at work, reported in its answer, so it
  raises no "Not done" notice.
- *The owner got the same tool review several times.* Runs that start
  together (the first reviews for each person) each read an app's tools as
  not pinned yet, and each put a "Review the tool descriptions" item before
  the owner. Pinning now takes one lock (`run/pins.rs`, moved out of
  `run/mod.rs`, which may only shrink). Test:
  `tests/scratch/tests/tool_review_once.rs`.

The first two are caught by scenario 3 itself, which fails without them.

**Where it runs.** No list of acceptance suites existed: AGENTS.md names the
heavy suites but nothing ran them together. `ci/acceptance.sh` is that list
(autonomy, injection, update, the full invariants) and runs any of them by
name, after building the programs the suites start. It runs on Strix Beta
through `ci/on-strix-beta.sh acceptance ci/acceptance.sh [SUITE...]`, never
from `fast.yml`. It touches each crate's root before building: Strix Beta's
checkouts share one build directory, and cargo copies a program there only
when it compiles one, so a checkout whose build was already fresh ran the
programs another checkout built last (seen here: a run of the fixed code
tested the server a revert proof had built). Reason for acceptance, not fast: the scenarios wait on real schedules and the push
gap, minutes each, which the five-minute fast tier cannot hold.

**Not fixed here, failing before this change too** (the same on the
unchanged checkout, on Strix Beta): `chat.rs` (7 of 16), `standing_authority.rs`
(4 of 7) and `proactive.rs`'s `a_review_proposes_a_task_the_person_approves`
(the proposal item no longer names its review under `task`).

**Scripted, not live.** The model is the scripted fake, so a run proves the
server's side of each scenario: what it offers, allows, wakes and shows. It
does not prove a live model chooses those steps. A live run needs a provider
key on Strix Beta, which the test harness does not have.
