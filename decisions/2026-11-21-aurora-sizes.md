# Aurora's sizes move

## Choice

In Aurora and Obsidian 2 Aurora, the bento under the conversation changes
size over time, not in one frame, and the conversation above it gives up or
takes back its room with it (`aurora-stage.js`, `grow()`):

- When a box arrives, the bento grows from the size it was drawn at to its
  new size over 460 ms. It keeps pace with the box rising in, on the same
  curve as the stage's other movement.
- When the last box goes, the bento folds on an ease-in-out curve: slow while
  the leaving box fades (320 ms), then quick. The room closes after the box
  has gone, not under it.
- A conversation read to its end stays at its end while it shrinks, frame by
  frame. One scrolled back is left where it is.
- A box that keeps its place rides with the bento, with no slide of its own.
  Slides are measured from the bento, not the window, so the bento's own
  movement is not mistaken for a box moving.
- Reduced motion changes sizes at once, as before. Other styles are
  unchanged.

This is step 3 of the Aurora motion plan: one helper that measures a region
before and after it is rebuilt, then animates between the two. Decisions and
pop-ups have since moved onto the stage
(`decisions/2026-11-21-aurora-alerts.md`), so the bento is now the one region
whose size changes. The canvas beside the conversation already slides.

The filmstrip kit (`desktop/tests/filmstrip-kit.mjs`) acknowledged each
screencast frame with a CDP method that does not exist
(`Page.screencastAck`), and its error was swallowed. Chrome stops after three
unacknowledged frames, so every filmstrip video until now was 0.6 s of
stills; the sheets of screenshots were right. It now uses
`Page.screencastFrameAck`, and the videos run their full length.

## Reason

The owner's concept: "What is important slides into view. When it doesn't
need user focus it disappears." Before this change, a box arriving made the
conversation jump shorter in one frame and lose its last message off the
bottom. The last box going made it jump back. The person saw the layout
change without seeing why.

## Verification

`desktop/tests/aurora-sizes.mjs` (7 checks), run by the `appearance`
scratch test. Four of them fail against the stage before this change
(3aba2c2). `desktop/tests/aurora-filmstrip-sizes.mjs OUT 3aba2c2` films the
stage before and after at the same moments.
