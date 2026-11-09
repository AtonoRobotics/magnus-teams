# A bad browser allow entry fails at load naming itself (2026-11-06)

`[browser] allow` entries are validated when the config loads, against the same grammar the sidecar uses. A bad entry now fails with its own text and what was expected, instead of a sidecar that never starts.
