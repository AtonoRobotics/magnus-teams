# Observations clear all at once, and a dismissed one is not an approval

The owner, 2026-11-27: "The observations show up as approvals when I
dismiss them." Also: "Observations should have a clear all."

Choice:

- The Approvals history lists only items that asked the person to
  decide: an approval, a question, a review, a blocking notice and the
  other decision kinds (`wasAsk`, the same rule that decides what waits in
  the queue). A dismissed observation, message or suggestion is not listed
  there.
- The Updates section has a Clear all button whenever observations or
  interruptions are open. It dismisses every open one through
  `/v1/inbox/batch`, fifty per call, which is the server's limit. Dismissing
  hides an observation and deletes nothing.

Reason: an observation is news, not a decision, so listing it among
approvals misled the person. With many open, dismissing them one at a time
was the only way out.
