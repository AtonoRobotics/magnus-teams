# An answer shows as given only once the server took it

**Choice.** The desktop says a decision was answered ("✓ Allowed" on the
thread's line, on the card, on the approval box) only after the server
accepted the answer. `respond()` in app.js sends `magnus:answered` on
`document` (the item, the decision, the button) once `Server.inbox.answer`
succeeded, before the redraw. aurora-alerts.js and aurora-stage.js listen
for it instead of for the tap. A refused answer (someone else answered
first, the card expired, the network failed) sends nothing: its toast says
why, and the thread line goes as for any decision answered elsewhere.

**Why.** In a shared project Mo can answer "Don't allow" just before Sam taps
"Allow". The server refuses Sam's answer, yet the line read "✓ Allowed",
because it was recorded at the tap. Only the server knows what was decided.

**Choice.** An approval box is known by its decision alone: how many other
decisions wait (`count`, carried only by the first approval box) is no
longer part of its sign. Other boxes keep their count in it: one more
message is new, so a closed Messages box comes back for it.

**Why.** Closing the first approval box stored its sign with the count, so
when another decision arrived or was answered the closed box came back.

A sign stored by an earlier version with a count no longer matches, so such
a closed box shows once more after the update.

Checked by desktop/tests/answer-shown-check.mjs and
desktop/tests/aurora-boxes-check.mjs.
