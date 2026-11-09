# #540 model picker follows the default (2026-11-08)

`modelPicker()`, `chosenModel()` and the composer's draft record carry the
default a pick was made under; a pick from under another default is ignored.

Test: `tests/scratch/tests/model_picker.rs` (`desktop/tests/model-picker.mjs`,
real appliance and browser). Without the fix it fails with `m-a` still selected
after the default moved to `m-b`. `workspace_continuity` still passes.
