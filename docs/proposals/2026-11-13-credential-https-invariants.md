# Move credential acceptance fixtures to verified HTTPS

The production credential fix refuses plaintext HTTP. Two existing tests in
`tests/invariants/tests/inv2_credentials.rs` still expect an HTTP request to carry
a real credential and return successfully. Their positive cases therefore need
TLS fixtures before the full invariant suite can validate this fix.

The accompanying `2026-11-13-credential-https-invariants.patch` is prepared for
owner approval under `AGENTS.md`. It is not applied. It preserves the existing
credential, approval, host-binding, echo, filesystem and log assertions, and adds
an explicit assertion that plaintext credential traffic never reaches the site.
Both destination fixtures use certificates the proxy trusts, so a wrong-host
failure still tests host binding rather than failing incidentally at TLS.

The fixture uses Python's standard-library HTTPS server and OpenSSL, with the
existing typed `upstream_ca` setting. It does not disable certificate validation
or change the tests' model adapter behavior. No new dependencies are required.

`git apply --cached --check` validates the patch against the repository index.
It has not been compiled or executed. After approval, apply it on Strix Beta and
run the two named credential acceptance cases with the installed computer service.
The production regression tests already cover secure origins, malformed framing,
chunked credential echoes, short echoes and compression refusal.
