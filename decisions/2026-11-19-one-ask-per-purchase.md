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

## Case 3 on a purchase (owner, 2026-11-19)
The Rule of Two asked on `purchase_request` and on each `purchase_fill` in a
run that had read untrusted content and private data, so a tainted run still
saw up to three cards for one purchase. Neither tool carries the work's
private data to a new destination: a request goes to the person's own Link
wallet with the seller, amount and reason, and a fill puts the approved card
on the approved seller's page. So the Rule of Two leaves both alone
(`payments::sends_only_the_purchase`, read where the boundary decides what
transmits). Each is still asked about as a payment: in Link, or at a saved
card's every fill, which still asks each time. Invariant tests in
`tests/invariants/tests/payments.rs` expected those cards; the owner's
patch is `docs/proposals/2026-11-19-payments-invariants.patch`.
