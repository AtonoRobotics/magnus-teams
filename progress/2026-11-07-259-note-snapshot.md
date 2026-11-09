# Run-start note snapshot (2026-11-07)

Fixes #259: a run's system context now carries its person's relationship notes as they stood when the run started. New test corrects the note while the run's second model call waits on a gate and asserts the third call still carries the original wording.
