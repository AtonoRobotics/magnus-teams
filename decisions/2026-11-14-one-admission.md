# One rule for what a run is offered and what it may call: `run::admission`

**Choice.** What a run may use is decided in one place, `Admission::admits(tool) -> Result<(), Refusal>` in `run/admission.rs`. It is built over the state and a small `Setting` read beforehand (the apps a workspace may use, whether this server has computers). Offering is the run's catalog filtered by it (`Kernel::tool_catalog`, then `Kernel::admitted`). The boundary's policy stage calls the same `Kernel::admits`, and a run record call does too (`record_call`). `Refusal` is `Halted { reason, said }`, which suspends the run as before, or `Denied { why, rule }`, which carries the boundary's existing words and its `hard_denial` rule tag.

The rules it applies, in order, each now written once:
- The switches: `halts::halt` with the tool (the server's, a person's, an agent's, a connector's).
- Where the run is: `chat_work::placement`. It replaces `offered_in_chat`, `in_any_run` and `misplaced`. A chat turn may use any tool but a task's records. The chat tools are for chat, except `create_schedule` and `list_schedules`. `suggest` is not a chat tool.
- A run record is the run's own bookkeeping, not an action. Past this point it is admitted, as before: `show` is chat's, the rich-answer tools are any run's, and a task's records are for runs that do not converse.
- A sub-agent's limits: `sub_agents.refusal`, unchanged.
- A team run's scope: `team::refusal`. It replaces both `team_refusal` and `team_scoped`.

The catalog keeps what exists rather than what is allowed: the built-ins this server can carry out, `project_read` only with reference files, the person's families, their connected apps (past `LOAD_ALL`, those found), and the team tools. `Prohibited` and the identity-provider groups stay where they were. The catalog leaves out an app tool that fails them, and the boundary refuses one by the same two functions, which are not copies of each other.

**Kept on purpose.** The switches are still checked at identify and again just before an app call (`boundary::call`, `Phase::Call`). Those are fences at the time of use, not a second copy of the rule. `chat_schedule` still refuses a schedule set up under a schedule, and `show` still refuses outside chat. These are fences behind the rule.

**Drift found, each read the safer way and tested** (`run/admission_tests.rs`, `team/admission_tests.rs`, `chat_work::tests`):
- A scheduled run was not offered `create_schedule`, but the boundary let it through to policy, review and approval. Only the schedule's own executor refused it. It is now refused at the policy stage.
- A team request was not offered a task's records, but a record call it made anyway was carried out, so `task_complete` would end it. It is now refused, in the words chat gets. The doc comment on `offers_records` that said "handled as before" is corrected.
- A team run whose requester had left the workspace, or was a guest, was offered the web, the computer and the team's tools. The boundary refused every one of them. It is now offered none.
- A sub-agent of a team run had its limits applied before the team tools were added, so it was offered team tools it was not given. The boundary refused them. It is now offered only what it was given.
- `team_read_knowledge` was offered only to someone in a workspace, but was let through for anyone. It is now refused for someone in no workspace.
- A whole-run hold (the server's switch, a person's or an agent's) took nothing from the offer, because only a connector's switch filtered it. The boundary suspended every call. A held run is now offered nothing. The loop already stops such a run before it asks the model.
- One drift went toward offering, because a stated rule settles it. Offering cut every `team_`-prefixed name outside a team run, which included a person's own app named `team_drive`. `team_only` and its test say that app is the person's to use anywhere, and the boundary let it through. It is now offered.

**Order.** The policy stage now runs this rule before `Prohibited`, `hard_denial` and the groups. Before, the chat placement check came after them. Only which refusal's words a call gets changes, and only when it fails two checks at once.

**Test.** `a_run_is_offered_a_tool_exactly_when_its_call_would_be_admitted` covers a chat turn, a task, a scheduled run, a review, a sub-agent with limits and a team request. For each, the tools offered must be exactly the catalog's tools that `Kernel::admits` lets through, in the catalog's order. A filter added to offering alone fails it. The boundary asks the same function, so it cannot let through what is not offered.
