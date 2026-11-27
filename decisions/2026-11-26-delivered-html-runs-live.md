# A delivered HTML file runs as a page

## Decision
When a deliverable includes an HTML file, the file is drawn live wherever
the deliverable is drawn: the conversation, the outcome and the task page.
The file's own page draws it live too.

It runs in the same frame as a `magnus-html` block:
- `sandbox="allow-scripts"`, so it has an opaque origin and cannot reach the app;
- the `HTML_CSP` policy, so nothing loads from the network;
- Magnus's theme variables are injected.

The rest of the deliverable is drawn by Compose beside it, as before.

A page may grow to `MAX_PAGE_HEIGHT_PX` (2400px) before it scrolls inside.
Inline blocks keep their 640px cap. Fullscreen is one click away.

A file card no longer shows a page's source as its preview. It says
"Interactive page · open to use it".

## Reason
On 2026-11-26 the owner asked for an interactive dashboard from the Live
Workspace tile. Magnus delivered `project-dashboard.html`, and the
conversation showed it as "code in a scrolling frame". Compose drew any
text file as its text, inside a frame with scripts off.

An HTML page is meant to be used, not read as source. The sandbox and
policy are the ones agent-written HTML already runs under, so this makes
no new promise about what such a page can reach.

`tests/scratch/tests/html_deliverable.rs` with
`desktop/tests/html-deliverable.mjs` checks this. A task started from chat
delivers a page whose script changes a heading. The test finds:
- the page live in the thread, its script run;
- no source shown;
- the stat beside it still drawn.
