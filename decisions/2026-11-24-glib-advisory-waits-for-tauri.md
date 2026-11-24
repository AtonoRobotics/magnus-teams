# The glib `VariantStrIter` advisory waits for tauri

**Choice.** Dependabot alert 1 (glib `>= 0.15, < 0.20`, unsound
`Iterator` impls on `glib::VariantStrIter`) stays open, and the desktop
app keeps glib 0.18.5 until tauri moves its Linux stack past gtk 0.18.
We do not patch or fork glib.

**Reason.** glib reaches `desktop/src-tauri` only through tauri 2.12.5's
gtk 0.18 (the GTK3 bindings). gtk-rs published no GTK3 crate on glib
0.20, so glib cannot be raised without leaving tauri. The unsound code is
the iterator over a GVariant string array, which the desktop app's own
code never calls (it has no `glib::` use of its own), and the server
crates do not link glib. Raise it with the first tauri release that does.
