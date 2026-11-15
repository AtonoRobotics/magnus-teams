# Policy layers live with policy: one `Layer`, one `stack`

**Choice.** The policy code is one directory, `policy/`: `mod.rs` (the decisions: `decide`, `control_allowed`, `loop_allowed`, grants, `may_use_app`), `layers.rs` (the `Layer` enum and `tightens_only`), `sides.rs` (which evaluations a rule bears on, and each side's permit-all), `admin.rs` (the admin's editor, from `policy_admin.rs`). `policy_layers.rs` and `policy_admin.rs` are gone.

- A layer's name is read in one place: `Layer::parse` gives `Org`, `Group(g)`, `User(u)` or `Task(t)`, each name below `org` non-empty and without `/`. `kept_by_the_server`, `above` and `proposable_by` are its methods. `people::Rights::may_write_policy_layer`, the `GET /v1/policies` filter, the `PUT /v1/policies/{layer}` validity check and the admin's editor (`MAX_LAYER_NAME_BYTES`, 106) all match on it.
- The hierarchy rule "only `org` widens" is written once, `policy::stack`: the organisation's layer (with the control and loop defaults beneath it) decides what is permitted, and each lower layer, with its side's permit-all added, only forbids. `decide`, `control_allowed` and `loop_allowed` read their answers from what it returns, keeping each one's own wording: `decide` puts a deny down to the organisation only when one of its rules matched, and to the first lower forbid otherwise; a control or loop refusal names the last lower layer that forbade.
- Pass-throughs deleted: `inbox::may_set_policy` (callers ask `people::rights(p).may_write_policy_layer`), `policy_admin::layer_ok`, and `boundary::may_use` over `generated::may_use`, which are now one `policy::may_use_app`.

A golden test (`policy/stack_tests.rs`, `stack_golden.txt`) was committed first and pins every outcome and message of the three over 37 layer stacks, three tools, taint, task, the loop's actions and four controls by three people; it passes unchanged after the move.

**Reason.** The same rule spelled three times, and a layer's name read five ways, are where a later change fixes one copy and not the others. The refusal wording the invariants match (`inv6_policy`, `inv8_layers`) is unchanged.

**Changed with it.** Toward the stricter reading, for names no route can write:
- A run may no longer propose to `group/` (empty) or `group/a/b`: `proposable` took any `group/` prefix. Neither layer can be set or read.
- `may_write_policy_layer` no longer answers yes for a group name holding `/` (`group/a/b`), which `PUT /v1/policies` already refused as no layer.
- `tightens_only` on a name that is no layer names every layer above it (the task's words) instead of reading a `group/` or `user/` prefix; only an inbox proposal stored before this change could carry one.
