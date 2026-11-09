# Destination-preview reads are authorized like any read (2026-11-07)

- The gate's preview read of what a send acts on gets its own access
  check, pin validation and policy decision before the provider is
  touched, audited as a Decision of stage `destinations` naming the read
  tool (#184). Reason: the read goes to the provider even though nothing
  it returns reaches the model, so a task layer forbidding it must deny
  the send rather than be bypassed.
- Ask fails closed like Deny: the send cannot show where it goes without
  the read. Reason: suspending a send for approval of a read the person
  never asked for is the card pattern #175 removed; the denial names the
  read and the rule so the person can permit it in policy.
- The preview is judged by the run's taint through policy text only, not
  the Rule of Two. Reason: an explicit "forbid reads when private" is
  content policy and binds the preview; taint alone still asks nothing
  (#175), and ordinary reads are untouched.
- The sent-mail search (`mailed_before`) keeps its shape for now: it
  fails closed toward asking and is its own issue.
