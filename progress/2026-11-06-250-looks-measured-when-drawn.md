# The looks are measured once drawn (#250)

The looks check waits for every part it measures instead of pausing 800 ms, so a panel mid-redraw is no longer measured as missing. Test: `looks.rs` `each_look_is_measured_once_every_part_is_drawn` (fails on the old script), and `each_look_draws_the_workspace_as_the_references_do` passes locally.
