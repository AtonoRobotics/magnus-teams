# Every screen scrolls, and every page has a way out

## Choice

- **Every screen scrolls.** Aurora clipped the main area (`overflow:
  hidden` on `.shell > main` in `aurora.css`), so a page taller than the
  window could not be scrolled: Explore hid 2,053 px and Apps 1,219 px at
  1280 × 720, and an app far down Apps could not even be reached to click.
  The main area now scrolls up and down (never sideways); a conversation
  still scrolls its own thread inside it.
- **Every page that takes the conversation's place has a close** at its top
  right, which stays there as the page scrolls and goes back to the
  conversation the person came from, or to a new one (`aurora-pages.js`):
  Explore, Teams, Trash, a project, and the like. A page with its own close
  (Apps) or its own Back keeps it and gets no second. A conversation has
  none. Other styles are unchanged.
- **An app's page has Back.** Its small "Apps" link above the app is now
  "← Apps", named "Back to Apps", in every style.
- What opens on the canvas already has Close, and Settings and the Admin
  pages (a dialog) have Done.

## Reason

The owner: "explore, apps, and all screens need scroll. Every app, card, etc
needs a way to go back or close." A close at the top right is where people
look for one, and going back to the conversation keeps the composition's
rule that the conversation is home: a page is something visited from it.

## Verification

`desktop/tests/aurora-pages.mjs` (the `workspace_layout` scratch test), in
the real app: Explore, Apps, Teams and Trash with nothing cut off, Explore
and Apps scrolling to their end with the wheel, the close on Explore, Teams
and Trash staying at the top, none added to Apps or a conversation, an app
far down Apps opened and Back taking the person to Apps, Explore's close
going back to the conversation, and another style with no added close. Run
on the code before this change, the same pages were cut off (Explore's main
area hid 2,053 px) and Explore had no close.
