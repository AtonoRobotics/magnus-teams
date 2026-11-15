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
- Second batch, each after its own pin extension: arrangements (schedules,
  grants, commitments, cards), the library (files, projects, moves,
  knowledge, memory, the playbook, the bundle), the inbox with the
  proactive loop's marks, then identity (sign-ins, subjects, rights from the
  provider, roles, data privacy, preferences, the home mark). A domain that
  reads another's facts gets them read-only from the router: arrangements
  and the inbox take identity's preferences, and an erasure's record takes
  the subject from identity. Identity holds folded facts only; what a
  person may do is still answered by `people::rights` alone. Trash and
  erasure, and the work itself (runs and tasks), stay in the state for now.
- Third batch: the trash and the erasures each fold in their own
  projection; taking an item out and putting it back stay orchestrations on
  the state, through each domain's take and put. The admin's workspace
  settings fold with the organisation's (`OrgSettings::admin`). The work
  (tasks, runs, the runs a person stopped, drafts' sources, attachments and
  the conversations Magnus opened) is relocated, not encapsulated: its types
  and folds live in `state::work`, and its fields stay on the state as
  `pub(crate)`. Reason: about 450 reads in about 90 files take them in
  place; renaming each read buys no safety the folds do not already give,
  and it would collide with every change landing beside this one. The
  folds take only the fields they write, so a run's fold cannot reach
  another domain. `openers` goes with the work because a task's creation
  writes it. The state's own tests moved to `state/tests.rs`, and
  `state.rs` leaves `ci/oversize.txt`.
