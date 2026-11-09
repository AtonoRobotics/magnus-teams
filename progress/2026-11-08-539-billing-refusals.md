# #539 billing refusals stop at once (2026-11-08)

`classify()` in `crates/adapter-openai` sends billing refusals to
`InvalidRequest` with the cause in the message; the kernel's notice for a
rejection includes the provider's reason.

Tests: unit tests in `adapter-openai` (billing codes, and limits that reset
stay usage limits); `tests/scratch/tests/models.rs`
`a_billing_refusal_pauses_at_once_and_says_why`, which fails without the fix
because the provider is asked again.

GLM on strix-alpha still fails: the account answers 1113 on both the
pay-as-you-go and Coding Plan endpoints.
