# Bounded, stable native-safe previews (#491, #494)

- Carry the full controls implementation onto current master to replace failed #490; keep its actual message operations, capabilities, speech ownership and sandbox security.
- Match deliverable layout by either the original iframe or its trusted parent wrapper. Preserve the conversation column's full width and cached height until a replacement srcdoc is initialized; initial blank-frame load events must not overwrite a real document's height.
- Pass the preview's row bound into shared Markdown table rendering. Parse and render only the retained rows, count the omitted rows and explain where the remainder lives. Ordinary answers keep their normal Markdown behavior; file/download bytes are unchanged.
- Keep native frame actions in the trusted parent. Copy dismisses the shared tooltip once and copies the captured original code. Native test automation scrolls the actual target into its independently scrolling region before measuring and making the real WebDriver click; it does not force a script click or substitute clipboard content.
- Parse absolute URLs without a base before resolving relative URLs against an actual host origin. Opaque native origins no longer suppress absolute HTTP(S) links. Unsafe schemes remain rejected and the existing external-link bridge remains the only handoff.
