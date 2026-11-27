# The app's scripts are checked as the one bundle they run as

Since Wave 4 (199231a) the server joins every `<script src>` of index.html into
`app.bundle.js`. One script is not many: a top-level `typeof NAME` before a
later file's `const NAME` throws (temporal dead zone) where it used to give
"undefined", and the throw stops every script after it. test.401 shipped that:
chat-composer.js asked `typeof UI` before app-shell.js declared it, and the
page never reached sign-in in Chromium.

The two dead lines are gone (app-shell.js's `UI` already has `mic` and
`stop`), and `desktop/tests/bundle-order-check.mjs`, in the fast tier, builds
the bundle as assets.rs does, parses it as one script, and refuses a
top-level `typeof` of a name the bundle declares later.
