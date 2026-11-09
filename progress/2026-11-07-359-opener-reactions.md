# #359 opener reactions (2026-11-07)

- Added `opener` to the `MAGNUS` kinds in `crates/magnus/src/reactions.rs`; resolution via the existing `m:{task}` path, no new lookup.
- Test `the_opener_takes_reactions` fails before ("only Magnus's messages take a reaction") and passes after; full `cold_start_proposals` file 5/5, lib reaction tests 4/4.
- Note: scratch tests spawn the prebuilt `magnus-appliance` binary, so `cargo build --workspace --bins` must run after the source edit before the test sees it.
