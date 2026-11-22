# A browser tab follows the server's updates

## Choice

The server serves the web app, stamped with its version (`?v=` on every
script and sheet in `index.html`). A tab opened before an update kept the
old page until someone reloaded it. After release 0.2.6-test.254 the owner
asked, "The web app is not 254?": the server served 254, but the open tab
was still the old page.

Now a browser tab follows the server's updates (`desktop/ui/web-update.js`):
- **It checks the server's version.** It asks `/v1/version` every minute,
  and whenever the person comes back to the tab.
- **When the version has changed, it loads the new page** at the address it
  is at, as soon as loading would interrupt nothing: at once if the tab is
  hidden, or nothing has been typed or clicked in it for 20 seconds, and no
  field has the focus and no dialog is open. Otherwise it loads the new page
  the moment the person leaves the tab or stops. Drafts are kept as before.
- **It loads once per version,** so a page still stamped with the old
  version after loading, for example from a cache in between, does not load
  over and over.
- **The desktop app is unchanged:** it updates itself.

## Reason

"Make the UI automated so the user doesn't need to learn it." A person
should not have to know to reload a page to get an update. Waiting until
nothing is being done follows the same rule, since reloading in the middle
of typing or a decision would interrupt them.

## Verification

`desktop/tests/web-update-check.mjs` (fast tier) runs `web-update.js` in a
stand-in page and checks that:
- a tab left alone loads the new version;
- a tab does not load it while the person is typing, has a dialog open or
  has just clicked, and loads it when they leave the tab;
- it loads once per version;
- it does nothing when the version is the same or the server cannot be
  reached;
- the desktop app does not check at all.

On the code before this change no tab ever loaded a new version.
