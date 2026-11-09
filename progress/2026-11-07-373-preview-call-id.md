# Distinct call ID for the preview audit (#373)

Review finding on #372 (Codex P2): reusing the send's call ID for the
preview Decision merged both into one trace action. The preview now
audits under `preview:{call_id}` with its own issuer label. Regression
pins both trace rows: the preview with its destinations decision and no
result, the send with its own result.
