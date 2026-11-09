# Billing refusals stop at once and name their cause (2026-11-08, #539)

Z.ai sends billing refusals as 429, so they were retried four times and
reported as a usage limit.

- The adapter reports Z.ai 1113, 1309, 1311, 1314, 1315 and Meta's
  `billing_error` as `InvalidRequest`. Reason: that class already stops at
  once. Z.ai's other 429s (1302, 1308, 1310, 1316-1321) reset, so they stay
  usage limits. Source: https://docs.z.ai/api-reference/api-code
- No new `ErrorClass`. Reason: the class is written into every `model_error`,
  and the log reader refuses an unknown value inside a known kind, so a kernel
  rolled back one release could not read its log.
- A rejection's notice carries the provider's reason. Reason: "rejected the
  request" has many causes and the person cannot act on it without that one.
- No Coding Plan preset, though the owner chose to add one (2026-11-08). Reason:
  measured first with `magnus conformance` on strix-alpha's real key, the
  Coding Plan endpoint answers 1113 too, for glm-5.3 and glm-5.3-flash, so the
  preset would not make GLM work. The finding is its own issue.
