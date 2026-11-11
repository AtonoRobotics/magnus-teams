# Drafts go as the agent wrote them (2026-11-11)

The kernel no longer rewrites draft bodies (#178, "scoped drafting") and the
send gate no longer waives the untrusted-input ask for a reply sent to its
thread's participants ("content provenance").

**Why.** Spec rev 140, Permissions and tasks, "replaces every earlier approval
and task rule". #178 came from the earlier approval model. Rev 140 asks the
person for "private data to a new destination after untrusted input unless a
rule covers it", with no rewrite and no content-provenance waiver. The rewrite
also broke work: run_cf3c9786… wrote a self-addressed test email; the rewrite,
given no thread, wrote the body from the person's chat request instead, never
told the agent, and the agent's read-back check failed. The owner asked why
the kernel rewrites anything at all; under rev 140 it has no reason to.

**What changed.** `boundary.rs` drops `scoped_draft`, its prompt and the
waiver. The `draft_scoped` event kind keeps its reader (an event kind is never
deleted); nothing writes it any more. `tests/scratch/tests/scoped_drafts.rs`
now checks that a reply holds the run's own body, no rewriting step runs, and
its send after untrusted mail asks.

**Also fixed with it** (same failed run):
- `login_codes::strip` read a connector's one-line JSON result as one line, so
  "verification" in a mail body redacted the address and the date beside it.
  Each JSON string is now read on its own, and an email address is never a code.
- `suggest` evidence refused `{ "call": "functions.<tool>", "task": "" }`, the
  shape a strict-schema model writes. An empty field now names nothing, and a
  tool's name stands for its latest successful result in the run.
