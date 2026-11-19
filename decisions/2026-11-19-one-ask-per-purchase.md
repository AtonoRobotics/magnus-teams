# A Link purchase is asked about once, in Link

## Choice
Link's approval card is the payment ask for a Link purchase (docs/proposals/2026-11-19-approvals-review.md, item 7). Before `settle`, the gate gives a purchase tool to `Kernel::purchase_gate` (`payments/mod.rs`) when the kernel raised no ask of its own for it:

- **`purchase_request`** only asks Link to put the purchase in front of the person. Auto Review still reads it and its denial stands, but an allow, an ask or an unreachable review lets it go on to Link: the person's card is Link's, and no second card is raised for the request.
- **`purchase_fill` of a purchase the person approved in Link** (this person's, on the Link rail, `Ready`, with its spend request) is covered by that approval, recorded as an approval decision. No Auto Review and no kernel payment ask.
- **Anything else** goes through `settle` as before. A saved card's fill is that purchase's one ask (`kernel:saved_card`, Allow once). A fill of a purchase that is waiting, declined, already filled or someone else's is not covered.

A covered fill uses the one-time card only when it is the one approved: when the card is fetched, Link's request must be the purchase's own, `approved`, and for the amount, currency and seller on record wherever Link names them (`spend_matches`). The page must still be the seller's (`same_site`). A fill whose amount, seller or card differs is refused.

The person's own ask-first rules and the Rule of Two (case 3) are unchanged: when either asks, `settle` raises it as before.

## Reason
The owner's standard: an ask is worth the person's attention only when they know something the system does not. For a purchase, that is whether this money is wanted, and Link's card already asks exactly that, with the seller, amount and the agent's reason. Auto Review's ask on the request, and again on the fill of what the person had just approved, asked the same question two more times. Money is still approved by the person once per purchase: in Link, or at the saved card's fill. Auto Review's denial is kept on the request because it is protection that costs the person nothing.

## Not done: case 3 on a purchase
In a run that read untrusted content and private data, the Rule of Two still asks on `purchase_request` and on each `purchase_fill`, so a tainted run still sees up to three cards. `tests/invariants/tests/payments.rs` (`a_purchase_is_a_one_time_card_the_person_approved_and_the_agent_never_sees`, `a_purchase_declined_in_link_ends_there_and_no_saved_card_stands_in`, `a_saved_card_is_approved_at_every_fill_and_works_only_at_its_seller`) expects those case-3 cards, so dropping them needs the owner. Recommendation: the request sends only the seller, amount and reason to the person's own Link wallet, where they read it, and a covered fill sends the approved card to the approved seller's page; neither carries private data to a new destination, so neither should be a case-3 ask.
