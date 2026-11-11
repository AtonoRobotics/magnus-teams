# On a phone, the composer stays at the bottom while writing

The owner reported that on a phone the text bar jumped to the top of the screen
while writing, which made the conversation hard to read. Opening the keyboard
shrinks the visible height below 520px, which switched the app to a
"short viewport" layout. That layout turned the conversation into a page that
scrolls, capped the thread at half the screen, and let the browser scroll the
focused composer to the top. On iOS a second effect added to it: the shell was
sized to the visible area but pinned to the top of the page, while Safari pans
the page to show the keyboard.

Choice: a short viewport keeps the conversation's column. The thread takes
whatever height is left and scrolls on its own, and the composer stays at the
bottom, just above the keyboard. While someone writes, the conversation header
hides and the banners shrink to one line that scrolls, so the newest messages
stay in view. The shell follows the visual viewport's offset
(`--workspace-top`, updated on visualViewport scroll), so on iOS the app fills
exactly the visible part of the page. This is how messaging apps lay out a
composer.

Also: the admin's kill-switch buttons in the top bar get the 44px touch target
that every other button has on coarse pointers. The layout test's touch-target
check caught them at 32px.

Test: `desktop/tests/workspace-layout.mjs` (scratch `workspace_layout`) checks
that at 390×300 with the composer focused, the composer's bottom sits at the
viewport's bottom, the thread is above it, and nothing scrolls. Before this
change it failed with the composer pushed below the visible area.
