# A once rule stays out of the task's envelope

**Choice.** `ensure_envelope` no longer folds a once allow rule into the write envelope.
The envelope is compiled once per task and kept, so a once rule in it covered every
later send to its destinations in that task, long after the rule was spent. A once rule
is now checked per action as the person's answer (`person_cover` → `covering_rule`,
which reads `live()`), and the action it covers spends it. Standing allow rules still
enter the envelope as before.

**Reason.** inv4 `auto_review_alone_cannot_release_private_data_after_untrusted_input`:
with a once Allow for Carol, both drafts to her went out after untrusted input. The
second one was never case 3, so Auto Review alone released private data.
