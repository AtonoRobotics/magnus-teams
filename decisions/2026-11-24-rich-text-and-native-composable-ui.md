# Rich text paste, Markdown user messages, and native composable magnus-ui

**Date:** 2026-11-24

## Decision
Upgrade both chat input and output to rich text and low-token native composable UI:

1. **Rich Paste & User Messages**
   - Add `desktop/ui/rich-text.js` (registered in `index.html` and `crates/magnus/src/ui/assets.rs`) with `RichText.htmlToMarkdown()` to preserve headings, bold/italic/strikethrough, links, nested lists, checklists, blockquotes, code blocks, and HTML tables when pasting into `desktop/ui/chat-composer.js`.
   - Add lightweight composer shortcuts (`⌘/Ctrl+B`, `I`, `E`, and `K` when text is selected).
   - Render user messages through `Compose.markdown()` in `desktop/ui/chat-view.js`, with typed-secret masking (`Compose.maskedText`) applied inside `markdownInline` so the secret-masking invariant holds.

2. **Richer Markdown Prose (Zero Extra Tokens)**
   - Extend `Compose.markdown()` (`desktop/ui/compose.js`) with nested lists, task checkboxes, strikethrough, GitHub-style callouts (`> [!NOTE|TIP|IMPORTANT|WARNING|CAUTION]`), inline citation pills (`[1]`), zero-dependency syntax highlighting (`RichText.highlightCode`), sortable tables with numeric right-alignment, and one-click `Copy CSV`.

3. **Low-Token, Low-Latency Composable Output (`magnus-ui`)**
   - Render validated `magnus-ui` blocks natively in the DOM via `Compose.renderNative()` (`desktop/ui/composed-frames.js`) instead of wrapping every chart/KPI in an `<iframe>` (keeping `magnus-html` and `deck` in their existing frames/sandboxes).
   - Auto-group consecutive `stat` elements into a responsive horizontal `.stat-grid` KPI strip with optional `delta` badges, add `callout` and `sources` elements to `crates/magnus/src/ui/catalog.rs` (synced with `desktop/tests/fixtures/ui-catalog.json`), and add client-side hover tooltips to `bar_chart` and `line_chart`.
   - Replace *"Most answers are plain text."* in `crates/magnus/src/tools_ui.rs` with a compact ~110-token inline cheat sheet of the core `magnus-ui` elements so Magnus emits rich composable blocks immediately without an extra `ui_catalog` tool round-trip.

## Reason
Previously, pasting stripped all formatting to plain text, user messages rendered as raw text, Markdown lacked callouts/highlighting/nested lists/sortable tables, and the existing `magnus-ui` DSL was discouraged by the prompt, gated behind an extra tool call, and wrapped in iframes. This delivers a standout rich-text and composable research/charting experience with minimal token cost and zero extra latency.
