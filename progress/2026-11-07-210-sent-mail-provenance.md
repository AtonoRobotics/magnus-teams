# #210: recipient provenance from sent mail

## Changed

- `crates/magnus/src/config.rs`: `SentSearch` (`read`/`input`/`query`/
  `found`) with `fault()` validation, on connectors beside
  `destinations`.
- `crates/magnus/src/catalog.rs`: manifest `[sent]`, copied to the
  runtime connector, checked by the manifest test.
- `catalog/google-gmail.toml`: sent search via `search_threads`.
- `crates/magnus/src/boundary.rs`: `connector_call` extracted from
  `destinations()` unchanged; `mailed_before` runs the sent search;
  the gate waives recipients mailed before.
- `crates/magnus/src/connectors.rs`: new-connector literal.
- `tests/invariants/src/appliance.rs`: `Spec.sent()` for tests.
- Test `tests/scratch/tests/sent_provenance.rs`: an address found only
  in sent mail passes under both taints with no card, naming nothing in
  the person's words; an unmailed one asks with its address on the card.

## Verification

- New test RED on pristine code (92s timeout: the mailed address
  asked), GREEN with the fix.
- #176 invariant tests (`an_allowed_recipient…`, `a_new_address…`)
  still pass: the refactor changed no behaviour.
- `magnus` lib 99/99 (manifest test covers the new `[sent]`);
  clippy clean on touched regions.
