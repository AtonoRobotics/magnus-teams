# Openers take reactions (2026-11-07)

- The reaction endpoint accepts great/annoying on `opener` entries, shown on the thread and carried into the person's later runs (#359). Reason: the opener is Magnus's own opening, rendered as its message in the thread, so under #267's "any message" rule the person judges that choice like any other; the `m:{task}` resolution path already handled it, only the `MAGNUS` gate excluded it. Test: `cold_start_proposals.rs` `the_opener_takes_reactions` (fails without: "only Magnus's messages take a reaction").
