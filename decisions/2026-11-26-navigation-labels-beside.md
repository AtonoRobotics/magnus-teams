# Navigation labels show beside their item; titles and release notes read whole

## Decision
- **Rail labels.** The label that shows when the pointer rests on an item in
  the navigation is drawn to its right, centred on it. Elsewhere labels stay
  below their control, as before.
- **Titles.** A conversation's title drops a Markdown mark left without its
  pair (a title is cut mid-sentence) and a list number that sat inside bold:
  "**1. Product Review: … Moat)** Magnus is **" reads "Product Review: …
  Moat) Magnus is".
- **Release notes.** What's new and Settings › Updates show a release's notes
  as sentences and a list (`desktop/ui/release-notes.js`). Lines wrapped for
  a terminal are joined, "- " lines are items, and trailers such as
  Co-Authored-By are left out.

## Reason
Checked in the installed app at 0.2.6-test.357 on 2026-11-26:
- The label sat 8px below its item and stays open while the pointer is on
  it, so it lay over the next item down the rail. A person who rested on
  Projects and then clicked Briefing, Explore or Apps clicked the label, and
  nothing happened. `desktop/tests/nova-screens.mjs` now clicks down the rail
  as a person does.
- The app's own conversation list showed "1. Product Review … Magnus is **".
- What's new broke every sentence where the commit message wrapped and
  ended with the Co-Authored-By line.
