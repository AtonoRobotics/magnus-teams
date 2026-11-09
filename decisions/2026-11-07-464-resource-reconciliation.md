# Scoped refresh feedback and truthful effects

Track each regularly refreshed resource separately, with its newest request token and an identity epoch. Failure preserves the last successful objects and names the affected resource and its actual last successful refresh time. Healthy resources continue independently. A status transition is announced once rather than on every poll. Session expiry invokes the existing sign-in flow and invalidates outstanding reads. Failed work refresh retains the same object's actual composed result, never another object's result.

The existing DELETE schedule endpoint stops nonterminal runs for an active schedule. Both conversation and work confirmations must describe that effect; an ended schedule's deletion leaves its runs alone. No endpoint, permission or approval behavior is changed. Verification details are expandable, with failures open and named in the summary; usage metadata appears only when supplied by actual task data.
