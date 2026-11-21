# Fast-gate reliability: unique executables and verified build-host identity

## Choice

The workspace has one executable producer for each public binary name. Invariant tests use the production `magnus-adapter-anthropic` executable built by `cargo build --workspace --bins`; they do not build a same-named wrapper.

The Strix Beta runner resolves its SSH host-key alias from the destination host by default, with an explicit `STRIX_BETA_HOST_KEY_ALIAS` override for nonstandard known-host entries. It never disables host-key checking.

## Reason

Cargo writes binaries to a shared target directory. Two targets named `magnus-adapter-anthropic` race to replace the same output, warning today and becoming a future hard error. The old test wrapper only called the production adapter library, so it added no behavior or isolation.

A fixed stale SSH alias makes the test helper fail before any build runs. Binding verification to the actual destination preserves strict host authentication while allowing the documented LAN default to use its verified known-host entry.

## Verification

The fast gate builds all workspace binaries and runs all library/binary tests on Strix Beta. The helper’s default LAN route must establish a verified SSH session before copying the isolated checkout.
