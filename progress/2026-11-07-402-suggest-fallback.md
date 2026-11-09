# Suggest gate on interrupt fallbacks (#402)

`interrupt_for`'s out-of-hours and budget-spent branches check
`suggest` before queueing their fallback observation; denied queues
nothing. Two tests (one per fallback) fail before and pass after;
`loop_policy` 5/5, `proactive` 9/9.
