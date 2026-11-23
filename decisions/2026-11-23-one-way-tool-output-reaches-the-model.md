# One way tool output reaches the model

**Choice.** `Kernel::planner_text` (quarantine_runtime) is the one way a
tool's output becomes what the model sees. The boundary's `record_from`
(now in `boundary/record.rs`) hands every result, failure and denial to
it. In a quarantined run, output under an untrusted label reaches the
planner only as typed fields and an opaque handle, whether or not it is an
error. A failed call is told that it failed and nothing of what was said.

This covers an app's `isError` answer, a read's answer that breaks its
schema (`CallError::BadAnswer`), the kernel's failure messages that quote
the app (`NotDone`, `Stopped`, `Unknown`, which carry the tool's label),
and an HTTP error body quoted from an app (`apps.rs`, untouched: it reaches
the model only through those messages). A denial whose reason may quote
what the app answered the gate's destination read is recorded under the
app's label too (`deny_as`), so it is quarantined the same way. The
kernel's own denials keep `PRINCIPAL` and stay readable.

**Reason.** `boundary.rs` quarantined only results that were not errors, so
a page or app that wanted to steer a quarantined routine only had to answer
with an error. Third review, 2026-11-23, item 4.

`boundary.rs` was at 787 lines; the recording code moved to its own module
so the file stays under 800.
