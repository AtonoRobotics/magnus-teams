# A saved card's ask says on its card that it is the saved card, and the charge

**Choice.** An ask to fill the saved card carries `charge` in its details
(`saved_card: true`, the amount, the seller), and its card's one sentence is
"Magnus wants to charge your saved card USD 35.05 at Paper Shop." Every other
ask keeps its sentence. The rule (`kernel:saved_card`) and the reason stay in
the developer view only.

**Reason.** Since the asks were rebuilt for rev 140 the inbox strips an ask's
reasoning (`rule`, `why`, `reason`, `taint`), as rev 140's "What users and
developers see" says. The saved card's amount was only in `why`, so the card
said "Magnus wants to make a payment (purchase fill) with your payment
method": the person approved a charge without being told what it costs or that
it was the saved card. Rev 140 has the card say in one sentence what Magnus
wants to do and where; for a payment, that is the charge.

The saved card is still asked at every fill and Allow covers that one fill
(`Reason::Payment`, scope once); that did not change.

The invariant test `payments::a_saved_card_is_approved_at_every_fill_and_works_only_at_its_seller`
reads `details.rule` and `details.why` from the inbox, which rev 140 moved to
the developer view. A patch for it, reading `details.charge` and the card's
sentence instead, went to the owner; with it the test passes on Strix Beta.
`asks_tests::a_saved_cards_card_says_the_charge` fails without this change.
