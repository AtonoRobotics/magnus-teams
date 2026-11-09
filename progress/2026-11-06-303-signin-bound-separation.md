# Sign-in wait test separates the deadline from the regression (2026-11-06)

Follow-up to the PR #283 review: the sign-in wait fixture now goes silent at 12 seconds with a 16-second bound, so the test tolerates loaded runners and still catches the per-request-timeout regression it guards.
