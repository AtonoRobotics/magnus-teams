# The installed-app drivers install every missing package (2026-11-06)

The CI step before the installed-app tests installed nothing, because its package list had been lost when it was written. It now calls `release/ensure-packages.sh`, which installs WebKitWebDriver and xclip together when a runner lacks them and nothing when it has them. A test in `ci.rs` runs the step's own text with stubbed commands and fails on the previous workflow.
