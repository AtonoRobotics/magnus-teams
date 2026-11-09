# Native material ordering

Fixed the race where an older native material effect could complete after a newer theme choice despite the previous response-metadata revision guard. Theme.apply now serializes the existing bridge effects and coalesces pending choices by effective arguments, while tokens apply immediately. Unchanged layout-only preferences do not repeat native effects.

`node desktop/tests/theme-check.mjs` passes: existing contrast/migration suite plus controlled-host response tests of actual Theme.apply calls covering ordering, coalescing, stale completion, failure recovery, layout-only preference changes and a return to the effect already in flight. This unit fixture is not claimed as a real native integration check; CI and final installed-host smoke validation remain required.
