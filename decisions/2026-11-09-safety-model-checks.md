# What comes in, what the monitor sees, and a second try at a blocked message

This replaces the inbound screening and monitor scope in `2026-11-09-safety-checks.md`.

Incoming content is screened by a model when `[safety] inbound_model` is set. Without it only the fixed phrases are looked for. Page text, pictures, files and connector results labelled untrusted are screened, and so is what `sandbox_exec` and `sandbox_read_file` return, because that is how a downloaded file reaches the model. Content the screen flags, or cannot judge, is held back and the agent is told. Reason: every other check fails closed when it gets no answer, and text only flagged with a warning has still entered the model's context.

The monitor reads each model reply once, with its tool calls and what the model last read, before any of those calls runs. It used to see only sends that reached Auto Review. A block refuses every call in the reply. Review and pause stop the run on an inbox item the person answers, and a pause also stops the computer and revokes its grants. A reply with no tool calls has nothing to stop, so the person is told.

A blocked message goes out only changed and approved by a person. The same message sent again stays blocked. Reason: the spec's "revised and resubmitted for approval"; a reviewer that blocked once should not then be the only judge of the rewrite. What was blocked is remembered in memory, so after a restart a rewrite goes back to the reviewer, which can still block it.

A computer that has run 24 hours without a stop hibernates even if busy, unless a person holds it (`[computer] max_run_minutes`). An admin can stop any computer but cannot delete a person's volume: the spec says destroying never loses it unless the person asks.
