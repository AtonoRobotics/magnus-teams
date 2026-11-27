# A broken page installs a downloaded update unasked

0.2.6-test.380 to test.384 opened to an empty window: a script used a name
before the script defining it had loaded, the page stopped, and the
"Restart to update" button it draws never appeared. The app downloaded the
release that mended it and could not install it. The owner had to
reinstall by hand.

Choice: the page tells the app it works (`page_works`) once its first
screen is drawn (the shell, sign-in, or update-required) with no error
while its scripts loaded. If the page hasn't said so within
`PAGE_WORKS_WAIT` (90 s) of the app starting, the app looks for a release
every `BROKEN_PAGE_LOOK_EVERY` (5 min) and installs one as soon as it is
downloaded, without the click. A page that works still waits for the
person's "Restart to update".

Reason: the person cannot click a button that was never drawn, and an
app stuck on a broken page is no use to them. Installing a release the
app already checked against the release key promises nothing new about
their data. The updater still only moves forward, so a broken release
that has nothing newer is not reinstalled in a loop.

Also: `desktop/tests/ui-load-order-check.mjs` runs index.html's scripts in
order in the fast tier and fails on a ReferenceError while they load, the
class of break that started this.

Only apps from this release on have the watch. Older installed apps still
need the release installed by hand once.
