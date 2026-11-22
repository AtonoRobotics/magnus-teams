# A credential's review is kept by host and method, not path

## Choice
`Kernel::review_credential_use` (`auto_review.rs`) keeps Auto Review's decision on a credential leaving through the proxy by person, computer, host (lower case) and method (upper case), with the rules' version, for the review cache's 30 minutes. The path is no longer in the key. It stays in the document the reviewer reads, which now says what its decision covers ("every GET request with this credential to api.example.com from this computer for 30 minutes"). An ask, or a review that could not be reached, is still never kept, and denies.

## Reason
docs/proposals/2026-11-19-approvals-review.md, item 9. With the path in the key, every new path on a host the person issued a credential for was another model call, and an outage of Auto Review denied it, though nothing about the decision had changed: the person's credential going to the host it was issued for. Keying by host and method keeps the review where it judges something (a new host, a new method, another computer or person, changed rules) and drops the repeats. A denial holds for the host and method too, which is the safer side.
