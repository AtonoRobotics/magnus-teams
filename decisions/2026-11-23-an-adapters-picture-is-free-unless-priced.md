# A picture a model adapter makes is $0 unless priced

**Choice.** Pictures now go through the one paid call (2026-11-23,
every-charged-token-is-booked). With no `[images] fast_usd` /
`premium_usd`, a picture made through a model adapter is priced at $0
and still recorded on its run and the month; a picture from an images
endpoint (`url` and `key`) with no price is still made only where no
spend limit applies. A price set in the config always wins.

**Reason.** The adapter route is the ChatGPT sign-in
(2026-11-11-images-through-the-model-adapter), which bills no picture.
Production names no price and every run has a dollar budget, so with the
unpriced rule alone every picture there would have been refused. An
endpoint is billed per picture, so it keeps the stricter rule.
