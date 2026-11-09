# #263: cancel reports cover the uncertain and the stopped

`controls.rs` classifies intents (done / possibly done / skipped) with a unit test; task cancels add `possibly_done`; schedule cancels return per-run reports with an honest count, and DELETE returns 200 with the report (`api.rs`).

GREEN locally: `cancel_report.rs` 2/2 (in-flight send held at FakeMcp; scheduled send then cancel), `controls.rs` classifier unit test, `magnus` lib suite. T1's RED is structural (no `possibly_done` key unfixed); T2's RED likewise (204 with no body unfixed).
