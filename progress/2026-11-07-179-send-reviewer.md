# The send reviewer (#179)

## What changed

- New `crates/magnus/src/run/reviewer.rs`: `review_send` (one `review`
  tool call judging recipient, content and the person's ask) and
  `reviewer_on_completion` (per-criterion holds), both through a
  budget-guarded `reviewer_call` emitting `ReviewerReply`.
- `crates/magnus/src/boundary.rs`: step 4b runs the reviewer on sends
  policy does not ask about; block denies plus a notice item, flag posts
  an approval card (`reviewer:flag`) and suspends. Approval matching
  extracted to `covering_approval`, shared with the approval stage.
- `crates/magnus/src/run/mod.rs`: completion sends verifier leftovers
  (check-less only) to the reviewer before the person.
- `events.rs` (`ReviewerReply`), `state.rs` (`Run.reviewed` + handler),
  `config.rs` (`[reviewer]`, per-model switch), `kernel.rs`
  (`notice_event` visible in-crate), `dev.rs` (registry + acted).
- Tests: `tests/scratch/tests/reviewer.rs` (block, flag-then-approve,
  kernel-ask-still-asks, criteria hold/flag) and the ablation
  `the_reviewer_blocks_a_send_to_the_wrong_recipient` in `modules.rs`.

## Verification

- reviewer: 4 passed. modules: 4 passed. lib: 112 passed. inv7: 5
  passed, untouched (reviewer defaults off).
- The modules verifier test fails on this branch alone with the #375
  signature (reads denied as recursion-limit): the new frames tip the
  same floor. With #375's fix applied it passes. This PR lands after
  #376.
- `slack_posts_as_alice...` times out identically on the pristine tree:
  pre-existing environmental failure, not this change.
