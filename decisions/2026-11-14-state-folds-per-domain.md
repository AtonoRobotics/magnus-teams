# The state projection folds per domain (2026-11-14)

`State` (`crates/magnus/src/state.rs`) already held sixteen domains as
projections of their own (`push::Ledger` is the plainest): private fields,
a fold, named queries and an `impl Forget`, fed by nested `Change` events.
About seventy-five fields were still flat and public on `State`, folded in
one match of more than a thousand lines. Those move into domains one at a
time, a commit each.

- `State::apply` stays one exhaustive match, a router, not a fan-out to
  every domain. A single-domain arm is one line handing the event to that
  domain's `fold`. An arm that touches several domains is a short
  orchestration, in a fixed order, that hands each the sibling data it
  needs read-only. Reason: the compiler still names every event kind the
  match misses, and the order of effects stays written in one place.
- The `last_seq` preamble runs first and `outcomes::observe` last, as before.
- Each domain is `#[derive(Default, Debug)]` (`forget_tests.rs` searches the
  state's `{:?}` for what an erasure left), has a `pub(crate) fn fold`, and
  an `impl Forget` listed in `forget.rs`'s destructure of `State`, so a
  field nobody forgets or keeps still fails to compile. A domain that holds
  nothing personal says so in its `forget`.
- A field read in few places outside its domain becomes private behind a
  named query. A field read widely (a connector's config, a person's
  reactions) stays a public field of the domain for now, so the move does
  not also rewrite a dozen callers; the domain is still the one place that
  folds it.
- New events for these domains are nested `Change`-style kinds, as `push`
  has. The flat kinds already in the log are never edited or removed: they
  keep their routes in `apply`, each to the domain that folds it now.
- There is no snapshot of the state: it is rebuilt by replay at every start,
  so there is no stored format to migrate. The risk is in memory: the same
  events folding to something else. So before anything moved, a test was
  committed (`state_canonical_tests.rs`) that replays a broad synthetic
  sequence, made up in the test and never taken from the production log,
  renders every field being moved under its old name with hash collections
  sorted, and compares the result with a pinned rendering
  (`state_canonical_pinned.txt`). Each move must leave it unchanged. Reason:
  a rendering pinned before the move cannot be bent to fit it.
- Domains in order: watch, server operations (`server_ops`: updates,
  backups and scrubs; the product says server, not appliance), vault, org
  settings, thread extras, connectors. Identity waits: authorization work is
  under way beside it.
