# Target the named composer textbox (2026-11-07, #478)

- Existing browser flows locate the Message textbox by role and exact accessible name. Required Message actions and More message actions labels intentionally share the word, so a substring-only label locator is ambiguous.
- Keep the product names, actual message input and all sign-in/approval/evidence assertions unchanged. This fixes the concrete #473 CI failures from current master as repository rule 6 requires.
