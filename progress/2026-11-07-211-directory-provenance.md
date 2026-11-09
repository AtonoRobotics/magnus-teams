# #211: recipient provenance from the organisation's directory

## Changed

- `crates/magnus/src/identity.rs`: `in_directory(address)` on the
  trait (sole implementor: the OIDC provider).
- `crates/magnus/src/oidc.rs`: realm users by exact email via the
  status client; failures read as not there.
- `crates/magnus/src/boundary.rs`: the gate tries the directory after
  own words and sent mail; no provider, no waiver.
- Tests: `the_directory_holds_only_realm_addresses` (stub IdP unit
  test, case-blind, failure-closed); `directory_provenance.rs` e2e
  (colleague passes naming nothing, outsider asks on the card).

## Verification

- E2E RED without the gate wiring (90s timeout: the colleague asked),
  GREEN with it; provider unit RED-by-construction (new API).
- #176 invariants and the #210 sent test still pass.
- `magnus` lib 101/101; clippy clean on touched regions.
