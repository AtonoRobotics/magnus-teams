# Built-in tools come in families, listed once

The built-in tools were seven `BUILTINS` tables (core, local, desktop, team,
images, built connectors, payments) that every reader chained by hand:
`tools::builtin()` and `tools::builtin_apps()` each kept their own chain, and
the second had already left one table out (the `images` app name was not
reserved, fixed the change before). Specs were assembled in two more places,
and the boundary carried a built-in out with a 185-line match in
`boundary::call` plus prefix routing (`local::is_far_tool`). A built-in with
no executor failed only when called ("no executor for ...").

Now `families.rs` lists the seven families once, as an enum, `FAMILIES`. Each
family gives:
- `builtins()`: its table, which stays in its own module;
- `specs()`: every spec it can offer (a test checks each built-in has one);
- `Family::of(tool)`: which family carries a tool out (its table, or the
  desktop for a program's `desktop_<program>__<tool>`).

`tools::builtin()` and `builtin_apps()` read `families::builtins()`, so a new
table cannot be listed in one and missed in the other. `Kernel::family_specs`
replaces `far_specs`: the per-person specs, in the same order as before (local,
desktop, images, built connectors, payments when set up); the core family's
are still assembled per run in `run/mod.rs` with chat's and a project's rules,
and the team family's by the team run. `Kernel::carry_out` is the one dispatch:
a `match` over the family, to each family's own entry (`local_tool`,
`desktop_tool`, `team_tool`, `image_tool`, `connector_tool`, `purchase_tool`).
The core family's match moved to `tools_core.rs`: a name maps to an `Op`
(`tools_core::op`), and the match over `Op` is exhaustive, so a test can check
every core built-in has an executor without running one. `boundary::call` is
the connector path and one call to `carry_out` (1311 to 1113 lines).

An enum with one `match` per question, not a struct of function pointers:
the executors are async methods on `Arc<Kernel>`, and fn pointers to them
need a boxed future each. Behaviour is unchanged: the same names, specs,
risks, labels and audiences, and the name-keyed taint rules stay in
`boundary::execute` where they were (`memory_search` is still answered there
with its taint; its `Op` gives the same text if it is ever reached through
`carry_out`).

Tests (`families::tests`): every built-in has a spec in its family, is carried
out by its own family (and a core one has an `Op`), names are unique across
families, and every built-in's app is reserved.
