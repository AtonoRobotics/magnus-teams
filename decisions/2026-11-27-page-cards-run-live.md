# A delivered page's card runs it, and never asks to be opened

The owner, 2026-11-27, on the dashboard's card at the bottom of the
conversation ("Interactive page · open to use it"): "You have to open the
file. It should have just opened." and "The DSL composer is used in every
other case."

So an HTML file is the one file kind that is not drawn through Compose, and
wherever its card appears it runs live, in the same sandbox as every page
Magnus writes (`drawLiveHtml`: scripts on, opaque origin, `HTML_CSP`):

- When the deliverable's view already runs the page (a `file` component that
  names it), the card under it keeps only the name and Download. The page is
  not drawn twice, and nothing asks for a click.
- When the page came only as a file, the card draws it live itself, with the
  name and Download under it.

Every other file keeps its glimpse, and every other deliverable stays drawn
by the composer. Checked by `desktop/tests/html-deliverable.mjs`, which a
task delivering a page outside its view now exercises.
