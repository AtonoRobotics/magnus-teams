# Tasks, schedules and suggestions: one model

Spec rev 140, Permissions and tasks: "Asking grants the work. A request in chat, a schedule the person set up, or a suggestion they accepted authorizes the work it describes and everything that work plainly needs", and checking is internal: the person sees results, not criteria or contracts. The owner: rebuild to it and delete the old paths rather than layering on them. This record replaces decisions/2026-11-10-create-task-one-line-goal.md (rev 138), whose content is kept below.

## Tools

Before: `create_task`, `create_schedule` (a card the person confirmed), `propose_task` (a proposal card in chat or a `task_proposal` inbox item, with "handle things like this"), `propose_schedule` (a `schedule_proposal` inbox item from a task) and `suggest` (chat only).

After, one tool for each thing:

- `create_task`: what the person asked for, started at once. It takes a one-line `goal`, a `title` and an optional `grant`.
- `create_schedule`: a schedule the person asked for in chat, **set up at once** with no card to confirm. It takes `title`, `goal` and its timing (`every_minutes` or `daily_at`, `until`, `max_runs`). The reason is that a schedule the person set up grants its work and never asks again. A confirmation would be a second ask for something they already asked for. Each run takes the server's per-task budget when it fires and starts with the taint the turn had, so its boundary actions are still reviewed against the request and the person's rules. Its goal is the scope each run keeps to: "The person set up this schedule to do this; keep to it: ...". Sub-agents keep the same scope, because they take their parent's authority. Its runs post in the conversation it was set up in (decisions/2026-11-10-schedules-post-where-set-up.md). A sub-agent of a scheduled run cannot suggest another schedule either. The thread shows the schedule as a `schedule_card` whose state is confirmed. A card id that was already set is answered 409, so it cannot be put off or declined as a suggestion.
- `suggest`: Magnus's own idea, from any run. With timing it is a schedule. In chat it is a card, and from a task or a review, which have no thread, it is a `suggestion` inbox item. Either way the person accepts it **with one action**, and that acceptance starts the task or sets up the schedule. It takes `goal` and no longer `plan`. A scheduled run cannot suggest another schedule.

Deleted: `propose_task` and `propose_schedule`, with their handlers (`propose_task`, `chat_run`, `inbox_proposal`, `proposal_data`, `proposal_parts`, `ProposalParts`), the confirm path for schedule cards, "handle things like this" (`handle_defaults`, `app_of`, the `handle` decision and its UI), `task_budget::under_grant`, the proposal and change-schedule UI in the desktop app, and the `task_proposal` and `schedule_proposal` inbox kinds. A standing grant comes only from `propose_grant`, which this change leaves as it was.

Readers for old events, because an event kind is never edited:

- A recorded `proposal_card` reads as a `suggestion_card`. Its `text` becomes the plan, and its `needs.permissions` become the permissions (`chat::card_kind`).
- `task_proposal` and `schedule_proposal` items read as `suggestion` (`inbox::inbox_kind`). Accepting one starts the task or sets up the schedule.
- A recorded `create_schedule` card that was confirmed reads as a schedule card.
- `goal` falls back to the old `text` field in `create_task` and `create_schedule`, and to `plan` in `suggest`, under the same one-line rule.

## Checking is internal

- **Criteria are frozen.** Once work starts, a `task_contract` call is refused with `run::FROZEN`: finish against the frozen criteria, or call `task_fail` saying what is not done. The outcome returns to the conversation, which can try another way. Before, a change after the freeze went to the person as a `contract_change` item to approve. Rev 140 puts no criteria to the person, and refusing is the safer reading: an agent cannot rewrite what counts as done after the fact, with or without a click.
- **Old change requests still resolve.** A `contract_change` item already in a log resolves whatever the decision, and its run resumes with the refusal.
- **The person's views lose the checking UI.** The contract and criteria sections, the checks pill, the Verification section and the done-line are gone from the task view. The request and the plan remain.
- **The verifier is unchanged and internal.** The API task JSON keeps `contract` and `criteria` for the dev view and the run inspector, and invariant 7 reads them.

## Retries and sub-agents

Neither asks. A retry starts with `create_task` within the authority and budget of the task that failed, "and needs no new approval" (`outcomes.rs`). A sub-agent holds at most its parent's authority and a share of its parent's budget (`sub_agents.rs`). Every task's budget is the server's per-task limit. Nothing needed to change, so no test was added for this.

## create_task takes a one-line goal (rev 138, kept)

- **One sentence.** `create_task` takes `goal` (one line, one sentence), `title` and an optional `grant`. A goal with a line break, or longer than `GOAL_MAX_CHARS` (200 characters), is refused, and nothing starts. The refusal says the task already carries the conversation. The reason is that a rule the description states but the server does not check is only a suggestion.
- **The server carries the conversation.** The task gets the thread so far, the message that started it and what Magnus said in that turn. Its context names the goal beside the conversation: "This task was started from a chat with the person, to this goal: ... The conversation it came from follows; the goal does not restate it."
- **The description states no prohibitions.** It says when to start a task, to give a one-line goal, and that the conversation is carried and not to be restated. It lists no rules or limits, because the server enforces them. `chat.md` says the same, and a test pins the schema and the description.

## Tests

Each test below fails without the change:

- `task_goal::tests`: the three create_task tests.
- `schedule_request::tests`: `the_proposal_tools_are_gone`, `create_schedule_sets_up_what_was_asked`, `old_proposals_read_as_suggestions` and `a_frozen_contract_is_not_put_to_the_person`.
- The scratch suites, rewritten to the new flow: `task_proposals`, `chat`, `schedules`, `proactive`, `suggestion_cards`, `standing_authority`, `contract_freeze`, `cold_start_proposals`, `autonomy`, `ownership_review`, `live_cards`, `schedule_threads`, `task_volume`, `no_silent_failures`, `looks` and `desktop_app`.
- The browser tests, which now assert that no checks are shown.

The safety assertions are kept:

- only the person a suggestion was put to can accept it;
- six concurrent accepts start one task;
- nothing starts before acceptance;
- the server's budget applies;
- the suggester's taint carries;
- a swapped-in check never runs and nothing leaves.

## For the owner

- **`docs/proposals/invariant-tests-tightened.patch`.** Its contract-change test for invariant 7 encodes the old approval flow, so that hunk should not be applied. No protected invariant encodes `propose_task`, `propose_schedule` or schedule confirmation, so no invariant patch is needed for this slice.
- **The `review` inbox item.** Invariant 7 (`a_criterion_without_a_check_goes_to_the_person_for_review`) still puts criteria that no check can run in front of the person. Rev 140's "checking is internal" reads against it. The item belongs to the asks and cards slice, and changing it needs an edit to `tests/invariants/` by the owner.
