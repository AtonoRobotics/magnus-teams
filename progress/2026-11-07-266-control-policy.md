# #266: task and schedule control follows the layered policy

## What changed

- `crates/magnus/src/policy.rs`: `control_allowed` evaluates a control
  operation against the layered policy (org permits, lower layers forbid
  only), with `DEFAULT_CONTROL` (own, owner, admin) appended to the org
  layer; `partition_layer` splits each layer's rules by named action via
  the Cedar Template API so tool and control evaluations never cross-read
  attributes; `decide` partitions the tool side too (skipped when no
  control action is named, so existing layers evaluate byte-identically).
- `crates/magnus/src/policy_admin.rs`: schema gains `Task`/`Schedule`
  entities under `Person` and the five control actions with their own
  applies-to block (`is_owner`, `is_admin` context); the console check
  judges each rule against its own side's half-schema (places stay true
  by filtering on policy id) and refuses an unqualified forbid that
  would also fit control, with guidance to name the actions.
- `crates/magnus/src/controls.rs`: `cancel_task` and `schedule_owned`
  (pause/resume/edit/cancel) check `control_check` after the existence
  gate; new `Refused::Denied` for honest policy refusals.
- `crates/magnus/src/api.rs`, `grants.rs`, `commitments.rs`: `Denied`
  maps to HTTP 403.
- `tests/scratch/tests/chat_controls.rs`:
  `an_org_policy_can_stop_admins_cancelling_others_tasks`.

## Verification

- New integration test fails without (admin cancel goes through) and
  passes after; new unit tests fail without (`control_allowed` is new).
- CI caught the first partition judging unqualified rules against tasks
  (`admin_security` sales draft); fixed by judging each rule against its
  own half-schema, with the sales case pinned by
  `an_unqualified_rule_validates_as_a_tool_rule` (red before, green
  after).
- `magnus` lib 102/102, `chat_controls` 7/7, `app` 17/17,
  `task_policy` 1/1, `admin_security` 1/1 locally; touched files
  fmt-clean, no new clippy lints (the `adapters.rs` clippy error and
  `accounts.rs` fmt drift are pre-existing, other hands).
