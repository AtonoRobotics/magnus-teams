# Evidence stays deletable, read-backs fail closed, and web provenance is exact

The owner requested fixes for all five findings in the repository review.

- Evidence responses, source claims and notes use the deletable content store.
  The existing inline-content migration also moves old evidence there on the
  next normal migration. Deleted source lists default empty so replay works.
  No production migration is run as part of this change.
- Missing or null expected read-back fields fail the comparison. Gmail checks
  To, Cc, Bcc, subject, body, attachment metadata and the digest of every MIME
  part; recipient comparisons use complete address sets and content comparisons
  use equality.
- Searches no longer bypass case 3 merely because the endpoint is a search
  provider, including browser engine pages: their query can contain private
  data. They retain ordinary policy and approval handling. This corrects the
  safety assumption in the November 12 and 13 search decisions under the
  owner's explicit instruction to fix the review findings.
- Web provenance compares whole address tokens, allowing only the documented
  trailing slash normalization, rather than matching arbitrary substrings.

Regression tests cover deletable evidence and replay, missing read-back fields,
Gmail content and recipient changes, private searches and partial URL matches.
They live outside the protected invariant suite.

Gmail's `get_message` now returns that same parts digest, and the read-back
binds it to the gate's digest. Its catalog version and pinned server image
are updated together. A mismatch is reported rather than silently treating
unverified message parts as checked.
