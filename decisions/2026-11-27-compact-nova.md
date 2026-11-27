# Nova is compact, as the mockup draws it

The owner, 2026-11-27, on the installed app: "The fonts are still larger than
mockup. The tasks, apps, and cards are all larger than they should be. Extra
space and more information than is required. The side bar is large and wide."

What made it larger, and what changed:

- **Touch sizes on a touchscreen laptop.** Every 44px touch rule was under
  `@media (any-pointer: coarse)`, which a laptop with a touchscreen and a
  trackpad matches, so the owner's laptop drew every button, field and
  sidebar row at 44px where the mockup has 32-36px. The rules are now under
  `(pointer: coarse)`: a phone or tablet, whose main pointer is a finger,
  keeps 44px; a laptop keeps the mockup's sizes, all 28px or more, above the
  24px WCAG 2.2 AA minimum. This narrows a floor that docs/ui-styles.md and
  the spec's accessibility line state for `any-pointer`; the owner's request
  and the mockup settle it, and the spec line is worth the owner's update.
- **Type.** The mockup names Inter but loads no font, so on Windows it draws
  in Segoe UI; the app ships Inter, which runs about a pixel larger at the
  same size. Nova's answer and bubble text is 14px (from 15 and 14.5), the
  home title 30px, sidebar rows 13px in medium weight, launchpads 13px. No
  text goes under the 13px floor that is not already a 12px meta line.
- **Space.** Sidebar 216px (from 228), rows 34px; thread gap 18px and 20px
  ends; composer padding 10/14/8 with a 32px send and a 28px autonomy pick;
  briefing cards 12px by 14px; stage tiles 8px by 12px; Explore cards 192px
  tall with 16px padding and a 15px title, as the mockup's.
- **What the mockup does not show.** Explore drops its blurb, the
  connected-apps strip and each card's guardrail, tools and steps; the
  briefing drops the Active Tasks card, the summary's card (it is a line
  under the title) and the New conversation button; a stage tile drops its
  steps and proof; an app row drops the Reviewed badge.
- **Tables in a chat answer** (the screenshot's empty band): cells start at
  the top, a link or code in a cell breaks anywhere so a long address cannot
  widen the table past its card, a link shows its words, and the table is
  13px. The empty band was a column pushed off to the right by raw link
  addresses setting the row's height, drawn since native magnus-ui blocks
  (3147877). desktop/tests/chat-table-check.mjs (fast) and the scratch test
  chat_table (browser) hold it.
