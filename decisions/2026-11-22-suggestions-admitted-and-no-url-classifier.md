# Accepted suggestions are admitted; constructed URLs skip the classifier

**Choice.** Accepting a suggestion goes through `admit_run`, the same as every other start. The person's allowed models and month spend limit apply, and so does their escalation access. A constructed URL in a tainted Private run is refused by a plain function, with no outgoing-classifier call and no byte charge.

**Reason.** The suggestion path wrote `TaskCreated` itself and skipped both limits. The URL check paid for a classifier call whose answer could not change the verdict: since 2026-11-22-a-classifier-clear-is-not-a-pass, every constructed URL there is denied and sent to `research_web`.
