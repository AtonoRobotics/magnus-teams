# A write whose answer breaks its schema is an unknown outcome (2026-11-06)

When a brokered tool's call has been made but its answer does not match its output schema, the kit now says so in the result's outcome (`bad_answer`), and the appliance's own check of the answer gives the same signal. For a write, that outcome is unknown and goes through reconciliation (the person is asked before it is sent again); a read's bad answer stays an ordinary failure. The kit changed, so the Gmail, Calendar, Chat and Slack images were rebuilt and their manifests name the new digests.
