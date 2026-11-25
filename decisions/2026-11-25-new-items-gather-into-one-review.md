# New items in a person's apps gather into one review

## Decision
After a review of new items in a person's apps wakes, further new items
gather for `[proactive] arrival_gather_minutes` (default
`wake_budget::ARRIVAL_GATHER_MINUTES`, 10) before the next such review may
wake. Nothing is lost: a wake turned away advances no baseline, so the
first read after the window finds every item that arrived meanwhile and
wakes one review for them. An item triage marks urgent (a CI failure, a
blocker) wakes a review at once. The hourly and daily wake budget still
holds for every review, urgent or not.

## Reason
The owner, 2026-11-25, chose to batch these reviews after seeing their
cost: one review per new email or message was the largest spender in
production, 94M of a week's 215M input tokens, 1,032 reviews at about 90k
tokens each. 124 of 667 arrival reviews woke within five minutes of the
one before and 291 more within fifteen; a ten-minute window folds about
45% of them into a neighbour. A few minutes' delay on non-urgent mail was
the trade the owner accepted.
