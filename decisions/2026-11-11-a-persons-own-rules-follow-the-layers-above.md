# A person's own rules are theirs, and change only what the layers above leave open

The owner, 2026-11-11: "users should be isolated from each other securely", and on rules: "I think the hierarchy you stated should determine the answer. The rule change should be allowed by inherited policy. The always restricted rules like payments and irreversible actions should not be changed [by] anyone except through deliberate change to the system itself."

## The hierarchy

The kernel's floor, then the organisation's layer (`org`), a group's (`group/<g>`), a person's own (`user/<id>`), a task's (`task/<t>`).

- **Who writes each.** `org` is the owner's. A group's is the owner's and the holders of `policy:group:<g>`. A person's own layer is theirs: no role is needed (the `policy:user` role no longer gates anything) and nobody else reads it, writes it or accepts a proposal for it, the owner included. A task's layer is its person's (`policy_layers::may_write`, used by `inbox::may_set_policy`, `PUT /v1/policies/...` and the inbox's `policy_proposal`).
- **Open and fixed.** Only `org` widens. What `org` permits is open to every layer below, which may forbid within it; anything `org` does not permit, or forbids, is fixed for every layer below. That was already how the kernel decides (`policy::decide`, `control_allowed`, `loop_allowed` add a permit-all to each lower layer, so only its forbids count), so a lower-layer permit could never override an org-level ask or forbid; it did nothing, silently. Now a permit below `org` is refused when it is written (`PUT /v1/policies`, 422), drafted or applied in the admin's editor (a group's layer), proposed by the agent, or accepted from a proposal, with a reason that names the layers above it (`policy_layers::tightens_only`). Tightening (a forbid) is always allowed. Chosen over judging each permit against the layers above: a lower permit can only ever be redundant or a failed loosening, so refusing all of them is exact and tells the person where the change belongs.
- **What the server sees.** `GET /v1/policies` shows the owner `org` and the groups' layers, never another person's or their tasks'. The admin's editor (`/v1/admin/policy`, draft, apply, history, test) lists and changes only `org` and group layers; its test leaves out person and task layers and so answers what the server's layers allow. A restriction the owner once wrote into someone's `user/<id>` layer is now that person's to change: a restriction meant to bind a person goes in `org` (`forbid(principal == Person::"carol", ...)`) or a group's layer.

## The floor

These hold whatever any layer says, the owner's included, and change only with the code in a release. Each was already compiled into the kernel and none lives in editable text:

- A `prohibited` tool never runs (`boundary::policy_stage`; invariant `prohibited_tools_never_run_even_if_policy_permits_everything`).
- Every payment asks its person, one action at a time; no rule, grant or schedule covers it (`payments::money_ask`, `person_cover`, `what_covers`; `grant_cedar` excludes spending in every grant).
- Every boundary action, irreversible ones included, goes to Auto Review or the person's own answer. A policy's ask or permit does not skip it (spec rev 140: "only its denials stand here").
- Private data to a new destination after untrusted input asks (the rule of two), and the hard denials stand.

`DEFAULT_ORG`'s text asks for irreversible and financial actions, and the owner can overwrite it; that is safe because a policy's allow skips none of the above.

## Tests

`policy_layers` (unit and route tests: mo writes their own layer with no role, the owner and an admin neither read nor write it, a permit in a person's or a group's layer is refused naming `org`); `tests/scratch/tests/task_policy.rs` (the owner and the admin's editor no longer reach carol's task layer). The invariants `inv6_policy::people_set_policy_only_within_their_role` and two assertions of `inv8_layers` encode the old model; the patch `invariants-own-rules.patch` changes them.
