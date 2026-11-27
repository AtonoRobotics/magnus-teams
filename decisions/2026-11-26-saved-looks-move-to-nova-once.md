# Saved looks move to Nova once

## Decision
A look saved on a device before this change moves to Nova the next time
its person signs in there: the style becomes Nova, and everything else
saved stays (light or dark, accent, motion, background, companion, the
panel's width, the pinned sidebar). The record then carries `look: 2`, so
this happens once. A style chosen after that stays, through any later
change such as a splitter drag.

## Reason
Nova became the default on 2026-11-26, but most people never saw it.
`Theme.set` writes the whole record, style included, on any change: a
splitter drag, a rail pin, a mode or accent. So anyone who had touched one
of those had the default of then (Obsidian) saved as if they had chosen it,
and `Theme.scope` never moved them. Every Nova feature is off under
Obsidian. A saved record cannot tell a chosen style from one written by a
splitter drag. The owner, the same day, asked for every UI problem the
review found to be fixed, after it recommended moving everyone once.
Anyone who wants their old style back picks it in Settings > Appearance,
and it then stays.
