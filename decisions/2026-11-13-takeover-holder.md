# Only the person holding a computer has its screen and hands it back

Two review findings (P1) on a workspace's computer, where every member may
take control.

Taking control from someone else now cuts every screen open on the computer,
right after the new holder is recorded. Before, the last holder's display
stream stayed open: the holder is checked only when a stream opens, and the
stream then carries input with no recheck, so they could keep typing while
someone else was shown as the sole controller, against "a new holder simply
replaces the last" (2026-11-08-computer-takeover.md). Cutting after the
record means the last holder cannot reopen (the open checks the holder), and
the new holder's ticket, issued after the cut, opens a screen the cut has
already passed. Taking control again as the same holder cuts nothing. The
stream does not recheck the holder on each input: a stream is only ever the
holder's, and every change of holder (a new holder, a handback) cuts.

"I'm done" (handback) is refused unless the caller holds the computer, with
the preview's wording, "you do not hold this computer", before anything is
stopped or cut. Before, any member could end another member's session: cut
their screen, close the programs they had opened and resume the agent
without them. Handback is the only path that ends a takeover; no admin or
kernel path ends one on someone's behalf, so none needs its own function.

Tests: `taking_control_cuts_the_last_holders_screen` and
`only_the_holder_hands_back` in computer/one_tests.rs; each fails without its
fix.
