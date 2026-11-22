# Aurora on a phone: the computer stays on the screen, and its card is framed

## Choice

The owner: "Mobile website is cutting off computer and not updating bento
box frame." These are the fixes, all for a phone (760 px wide or less):

- **The conversation's head keeps its row on the screen.** The row in the
  head holds the view icons, the computer chip and the delete button. It
  could not wrap, so at 390 px it was 461 px wide in a 366 px head, and the
  computer chip was cut off at "Your c…". Now the row wraps instead of
  running off the screen.
- **The computer chip shows only its icon.** Its words, such as "Your
  computer" or "Magnus is using your computer", stay in its accessible name.
  At desktop width the words still show (`aurora-stage.css`; the chip's
  words are now their own element in `screen.js`).
- **The computer's card has room inside its frame.** Aurora gave
  `.computer-view` a frame with no padding, so the status, its words,
  "Take control" and the screen touched the frame. It now has 12 px of
  padding all round (`aurora.css`), on the computer's page and where it is
  pinned beside the conversation.

## Reason

Nothing should run off a phone's screen or touch its frame. The chip's
state is still shown by its icon and colour, and the full words are kept
for assistive technology.

"Not updating" may also have been a tab still showing a page from before an
update. `decisions/2026-11-21-web-tab-follows-updates.md`, released in
0.2.6-test.255, makes a tab load each new version by itself.

## Verification

`desktop/tests/aurora-phone.mjs` (`tests/scratch/tests/aurora_phone.rs`)
runs the real app with a real computer at 390 × 844. It checks that:
- nothing in the conversation's head runs off the screen, and the computer
  chip is wholly on it, named for its state;
- the computer's card keeps at least 8 px between what is in it and its
  frame;
- the computer's page does not scroll sideways;
- turned sideways, the head still fits;
- at desktop width the chip still shows its words.

Run on the build before this change, the head ran to 493 px, the chip was
off the screen, and the card's contents were 1 px from its frame.
