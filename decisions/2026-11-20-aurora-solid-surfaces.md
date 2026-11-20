# Aurora surfaces stay unblurred

## Choice

Aurora may use layered gradients, borders and shadows, but it must not use `backdrop-filter` or `-webkit-backdrop-filter` on any desktop UI surface.

## Reason

The desktop interaction contract requires solid, unblurred surfaces. Backdrop blur obscures content behind navigation, panels and dialogs, adds unnecessary compositor cost, and regresses the existing `companion-panel-check.mjs` guard. Aurora retains visual depth through its scoped background gradients, opaque surface tokens, rims and shadows.

## Verification

`desktop/tests/companion-panel-check.mjs` scans every desktop stylesheet and fails if either blur declaration returns.
