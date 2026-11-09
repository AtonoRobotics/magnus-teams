# Retry a throttled client registration (#315)

The sign-in's RFC 7591 registration POST had no retry: one 429 from a
provider under load failed the sign-in, and twice failed a whole CI run's
live catalog test. The retry lives in the product path, not the test
helper, so a person's sign-in waits out the same throttle.

Three attempts on 429 or 5xx only; any other refusal still fails at once,
and a transport failure still errors untouched — the issue is the
throttle, not the reach. The wait honors the server's
`Retry-After` in seconds, capped at 10 s, else 1 s, 2 s: the sign-in is
interactive, so the waits stay short. A date-form `Retry-After` falls
back to the backoff.
