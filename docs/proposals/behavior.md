# Behavior: substance and character

Status: plan, being built. Source: the owner's spec "Magnus Desktop App — Behavior Spec"
(2026-10-31, updated 2026-11-02; https://claude.ai/artifact/RJUmBfvY58fTNwj9EwRZ13). This plan
maps the spec onto what Magnus already has and orders the rest into steps,
each shipped and tested on its own. Where the spec gives a number (60-minute
review, 3 interruptions a day), the number is the spec's.

## What exists already

| Spec item | Where it is today |
| --- | --- |
| Cedar policy, org layer first, lower layers only narrow | `crates/magnus/src/policy.rs` |
| Approvals rendered from the action payload, not model prose | `boundary.rs` (summary and details from the tool input), app "What will happen" |
| Kill switch halts everything | `api.rs` `/v1/kill-switch`; schedules, runs, vault, updates |
| Schedule proposal cards with accept and decline | `chat.rs` `decide_card`, `Ev::CardDeclined` |
| Per-person memory, playbook, library, company knowledge | `state.rs` `MemoryEntry`, `run/memory.rs`, `library.rs`, `knowledge.rs` |
| Every reply logged with model, instructions version, tools hash | `events.rs` `ModelCall.context`, `ModelReply` |
| Five providers on two adapters, one system string each | `config/providers.toml`, `adapter-anthropic`, `adapter-openai` |
| Experiments with pass@k, per-task verdicts | `dev.rs`, `Ev::Verdict`, `eval/` |
| Kokoro read-aloud, one voice per appliance | `speech.rs`, `[read_aloud] voice` |

Missing: identity and character, relationship memory, disagreement records,
message reactions, general suggestion and observation cards with a ledger,
the periodic review, the interruption budget, quiet hours, the Cedar
suggest/interrupt/act permissions.

## Steps

The character is part of Magnus: it speaks on every model, with no test
gate (owner, 2026-10-31: "The character is integral. It is not a switch or
something to be turned off by the models. All the testing of the models
with the personality is over engineering.").

The spec was updated on 2026-11-02 (task proposals, standing authority, the
commitment ledger, reply composition, tasks and schedules in conversation);
the owner ruled the same day that people may grant standing authority over
their own work within limits, with invariant 6 reworded and its test
updated (below), and that the behavior spec comes before the rest of the
admin console.

1. **Identity and the character.** Done (DECISIONS.md, "Identity and
   characters").
2. **Tasks and schedules in conversation.**
   - a. Control in conversation, and composed replies. Magnus has
     `list_tasks`, `list_schedules`, `cancel_task`, `pause_schedule`,
     `resume_schedule`, `edit_schedule` and `cancel_schedule`, the same
     operations as the app's buttons (`controls.rs`); a person controls
     their own, the owner and admins anyone's. A reply is composed, not
     routed: one reply can answer, start a task, propose a schedule, act on
     tasks and schedules, or ask, together. A cancel says what was already
     done, the changes that cannot be undone first.
   - b. Live cards. A task card (running, waiting, done or failed; Cancel,
     Open) and a schedule card (next run, latest run and its result; Pause,
     Resume, Cancel, Open) render from the object's current state, with
     direct buttons. A one-off task posts its result in its conversation,
     outcome first, as a new message quoting the request when the person has
     moved on; each schedule run posts to the schedule's own thread, and the
     originating card shows the latest run.
3. **Relationship memory and reactions.**
   - The person's tone instructions ("fewer jokes"), what they delegated and
     standing decisions are kept as relationship memory, always in context
     (not searched for), and never shown to anyone else. Done (#232,
     `decisions/2026-11-06-232-relationship-memory.md`).
   - The model sees its recent messages to the person, so it does not repeat
     itself. Done (#232).
   - Any message can get a "that was great" or "that was annoying" reaction,
     logged. Done (#233, `decisions/2026-11-06-233-reactions-disagreements.md`).
   - Disagreements are logged: Magnus's position, the person's decision, and
     later the outcome. Done (#233).
4. **Suggestions and proposals.**
   - Two schema-validated cards. An automation suggestion: title, plan,
     evidence, permissions; Accept (creates the task or schedule), Not now,
     Decline with a reason. A task proposal: noticed (what and where),
     outcome, why now, needs (permissions, estimated cost, input); Approve
     (starts the task), Dismiss with a reason, Handle things like this
     (opens a standing-authority grant). The automation suggestion is done
     (#253, `decisions/2026-11-06-253-suggestion-cards.md`); the task proposal
     is done with Approve and Dismiss (#254,
     `decisions/2026-11-06-254-task-proposals.md`). Its "Handle things like
     this" opens a grant, so it is built in step 5.
   - The validator refuses a card whose evidence does not point at something
     real: an event in the log, or an item in a connected app that Magnus
     actually read (recorded when read). Schedule cards become suggestions.
     Done for suggestions (#253) and task proposals (#254): a tool result
     the turn read, or a task of the person's.
   - Every card and its answer go in the suggestion ledger, which the model
     reads before it suggests again; a declined one comes back only when
     circumstances changed, saying what changed. Done (#253).
   - At cold start, and for each newly connected app, Magnus looks at what it
     may see and proposes two or three things it could take over. Done (#255,
     `decisions/2026-11-06-255-cold-start-proposals.md`): a conversation Magnus
     opens itself, whose cards the empty Tasks and Schedules list.
5. **Standing authority.**
   - A grant comes only from "Handle things like this" or the person asking
     in conversation; approving one proposal never creates one. The button
     goes on the task proposal card here, with grants (moved from step 4,
     #254). Done (#235, `decisions/2026-11-06-235-standing-authority.md`):
     "Handle things like this" on a task proposal, or a grant card
     (`propose_grant`) the person accepts.
   - A grant is a Cedar policy in the person's layer: a category of work, a
     budget per period (spend and model cost), and excluded actions that
     still need approval. Excluded by default: sending on the person's
     behalf, spending money, deleting, and committing them to anything; the
     person can change these per grant. Done (#235), with the budget in
     model cost per rolling day, week or month; spending money stays
     excluded in every grant (the kernel cannot measure an amount), and
     deleting and committing are one exclusion (the kernel's one class,
     `irreversible`).
   - Within limits (owner's ruling, 2026-11-02): a person grants only over
     their own work, and only actions they could approve themselves; the
     organisation's policy still bounds every grant. Invariant 6 is reworded
     to say so ("autonomy comes from policy: the owner's for the
     organisation, each person's own grants over their own work within it"),
     and its test is updated with it, both in this step. Done (#235).
   - Work under a grant runs as tasks with their usual checks; grants are
     listed with approvals, editable and revocable there or by telling
     Magnus; Magnus reports every action taken under one. Done (#235): in
     Settings, under "What Magnus may do on its own", and by `revoke_grant`
     in conversation; each covered action is a `grant_used` event listed
     under its grant.
6. **Commitment ledger.** What the person and Magnus said they would do, to
   whom, by when, from where, and its status (open, done, overdue,
   released), recognised by the model in conversations and connected data.
   It is in context for the review and the wakes it bears on; the person can
   see, correct and remove entries. Done (#236,
   `decisions/2026-11-06-236-commitments.md`): `record_commitment` records
   one only when its quote is found in the person's own words or in an app
   read it names; every run carries the open ones and their taint; overdue
   is derived from the date, and raising it is step 7's; Settings, "What you
   and Magnus said you'd do".
7. **The proactive loop.**
   - The kernel wakes a review run for each person every 60 minutes in their
     working hours and once overnight, and on events: a task done or failed,
     an approval left waiting, a schedule coming up, new items in a
     connected app, a commitment coming due.
   - The review decides: nothing (the usual), queue an observation for the
     next session, interrupt now, propose an automation or a task, or act
     within a grant.
   - Cedar gets three actions per resource: suggest, interrupt, act. Acting
     without a grant becomes an approval request.
   - Interruptions: at most 3 per working day by default, a setting each
     person can change. Over-budget items wait for the next session. None
     outside working hours. Delivering requested results does not count, and
     quiet hours delay the notification, not the post. The kill switch stops
     the loop.
   - Opening the app shows what changed and the queued observations.
   - Tests: the budget, quiet hours and the kill switch hold.

Each step lands as its own pull request (or two), with its tests, in this
order: steps 4 and 5 speak through step 3's memory, and step 7 needs the
cards, grants and ledger before it.

## Open

- The character. The spec recommends the chief-of-staff draft in its
  appendix and leaves the choice to the owner. Step 1 ships that draft; alpha
  feedback will show whether it lands.
- Cost: the review loop makes about 10 model calls per person per working
  day, plus one per event that wakes it.
- Review (2026-10-31): card evidence may be a connected-app item Magnus
  read; the interruption cap is per working day and each person's to
  change; the traits are written as identity, not commands. The spec's
  eval suites as gates are dropped (owner, above).
