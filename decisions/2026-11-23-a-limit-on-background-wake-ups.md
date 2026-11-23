# A limit on background wake-ups

**Choice.**
- A person has a budget of review wake-ups: at most 10 in any hour and 50 in any day (`wake_budget.rs`, `MAX_WAKES_PER_HOUR`, `MAX_WAKES_PER_DAY`). The numbers are the ones `AmbientQuota` promised.
- It is checked in `start_review`, the one place every review starts: new items in a watched app, an ended task, a coming-up schedule, a due commitment, a waiting approval, the cadence and the overnight review. A wake past the budget starts nothing; a watched read's baseline then does not advance, so the items wake a review once the budget allows, as a dropped wake already did.
- The wakes are counted from the log: the inbox fold keeps each person's `ReviewWoken` times of the last day. A restart replays them, so it forgets none.
- `AmbientQuota`, an in-memory counter used only by its own test, is removed; its limits live in `wake_budget`.

**Reason.** Each new mail in a watched mailbox started a review on a full task budget, held back only by one review at a time and a 30-second gap. A stranger mailing once a minute could spend a person's month. The cap the code described was never applied.
