# Kernel modules, Link sign-in state on the kernel, and `Body` extractors (2026-11-15)

**Choice.**

1. **Link sign-ins and token refresh live on the kernel.** `payments/routes.rs`'s waiting OAuth sign-ins (`payments::SignIns`) and `payments/link.rs`'s one-at-a-time refresh lock move from process-wide `OnceLock` statics onto `connectors::Connecting` (`k.connecting.link_sign_ins` and `k.connecting.link_refresh`), finishing the follow-up left in `decisions/2026-11-14-connectors-in-modules.md`.
2. **JSON body extraction through `extract::Body`.** The routes in `api`, `control`, `dev` and `payments/routes` that keep a dynamic `serde_json::Value` body now extract `Body<Value>` instead of `axum::Json<Value>`, so a malformed or missing JSON payload is answered with `{ "error": ... }` rather than a plain-text rejection.
3. **Oversize modules split by responsibility.** Seven production files leave `ci/oversize.txt`:
   - `accounts.rs` → `accounts/tokens.rs` (invitations, password resets, machine codes).
   - `api.rs` → `api/sign_in.rs`, `api/work.rs`, `api/owner.rs`.
   - `boundary.rs` → `boundary/call.rs` (invocation, grant/person checks, memory hits, computer command framing).
   - `config.rs` → `config/connector.rs` (`Connector`, `Watch`, `ToolClass`) and `config/load.rs` (loading, validation, person lookup and tests).
   - `control.rs` → `control/ops.rs` (policy sets/drafts and module/provider/update operations).
   - `dev.rs` → `dev/experiments.rs` (experiment trials, verdicts and comparisons).
   - `run/mod.rs` → `run/records.rs` (result/item recording, chat turn completion and plan validation).
   `events.rs` moves its supporting structs (`ChatOrigin`, `Opener`, `ProjectContext`, `RunConfig`, `ExperimentTag`, `ContextRef`) and `Ev` methods into `events/meta.rs`, lowering its baseline in `ci/oversize.txt` from 1404 to 1236.
4. **Unwraps removed.** Twenty-two non-test `.unwrap()` calls across `accounts.rs`, `api/work.rs`, `boundary.rs` and `dev.rs` are replaced with option handling or poison-recovering lock guards, and `ci/clippy-baseline` is ratcheted down to match.

**Reason.** Two servers in one process (as the test suites run) no longer share Link sign-in state or wait on each other's Link token refresh; every production module outside the single `Ev` enum definition is now under the 800-line ceiling; and routes that read dynamic JSON bodies still answer malformed requests in the API's JSON error format.
