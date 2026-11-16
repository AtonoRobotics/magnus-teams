# Theme injection and auto-sizing for magnus-html frames

Agent-written `magnus-html` blocks now receive Magnus's active theme tokens as CSS variables (`--foreground`, `--muted-foreground`, `--card`, `--card-border`, `--primary`, `--primary-foreground`, `--success`, `--warning`, `--error`, `--chart-1..6`) on a transparent `<body>`, plus live `magnus:theme` updates and `magnus:resize` height reporting over `postMessage`.

Because `magnus-html` frames run with `sandbox="allow-scripts"` (opaque origin, no `allow-same-origin`), the parent cannot inspect `contentDocument` directly as `sizeComposedFrame` does for `magnus-ui`. A `postMessage` bridge validated by `e.source === frame.contentWindow` and clamped to `[48px, 640px]` lets inline widgets size cleanly to their content without inner scrollbars or loosening the sandbox.
