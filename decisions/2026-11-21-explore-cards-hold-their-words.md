# Explore's cards hold their words

## Choice

In Aurora and Obsidian 2 Aurora, each Explore card had a fixed shape
(4:3, the first 21:9) and clipped what did not fit. Its label (the kind of
playbook and its apps) was pinned to the top, and its words were stacked
from the bottom. The owner: "Explore cards are malformed on mobile web
app." On a phone, and on a phone turned sideways, a card's title and
description did not fit the shape. They ran up under the label, and the
top of the title was cut off. Opening a card's steps had nowhere to go.
Turned sideways, the first card was 747 px wide on an 844 px screen: its
21:9 shape with a 320 px least height made it wider than its column.

Now (`desktop/ui/aurora-stage.css`):
- The shape is the least a card takes. A card whose words need more room
  grows to hold them: the card no longer clips, which makes `aspect-ratio`
  a minimum. The image is still clipped by its own frame.
- The label takes its place at the top of the card, and the words still
  sit at the foot over the fade. When the words need the whole card, they
  start below the label.
- A card is never wider than its column (`min-width: 0`). The first card
  loses its fixed 320 px least height (0 on a phone), which both stopped it
  growing and pushed it wider than its column. Its words now set its least
  height.
- At desktop width, where the words fit, the cards look as before.

## Reason

What is in a card has to be readable on every screen the app runs on. A
fixed shape is the mockup's look, not something to keep at the cost of the
words.

## Verification

`desktop/tests/aurora-phone.mjs` (the `aurora_phone` scratch test), in the
real app at 390 × 844 and turned sideways at 844 × 390, checks that:
- every card's label, title, description, apps, steps and button are inside
  the card;
- no words sit on the label;
- no card runs off the screen, and the page does not scroll sideways;
- a card with its steps open grows to hold them.

On the code before this change it fails: on the phone every card's words
leave the card and sit on its label, and turned sideways the first card
runs off the screen (113..860 on an 844 px screen).
