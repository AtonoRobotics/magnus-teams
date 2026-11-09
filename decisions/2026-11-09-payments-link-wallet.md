# Payments use Stripe's Link wallet for agents

The agent pays with Stripe's Link wallet for agents, using a one-time card issued for each approved purchase. If that is not available, the fallback is the vault card filled at checkout, with approval every time.

Reason: a card that exists for one approved purchase cannot be reused or spent on anything else, so the approval and the money are tied together. The vault card has no such limit, which is why the fallback never runs without its own approval.

Payments are built last. Nothing earlier waits on them.

## How it works

- Link's own approval is the approval for a Link purchase. The person approves the amount and seller in Link, and Magnus adds no second question.
- A purchase declined or left to expire in Link ends there. The saved card stands in only when Link cannot serve the person at all: not connected, or the service failing. A saved card replacing a purchase the person refused would overrule them.
- Every saved-card fill is a kernel ask (`kernel:saved_card`) that only a one-time approval can answer and policy cannot permit. The saved card has no limit of its own.
- A card works only on the seller's host named in the purchase, and only once. The fill is recorded before the page is touched, so a retry after a crash cannot pay twice.
- Card details go from the vault or Link to the page. They are not shown to the model, written to the log or stored outside the vault.
- Link needs Stripe's OAuth client, granted through Stripe's application form, and serves customers in the US and Canada only. It is tested against a fake of Link's API, not the live service.

## Where this departs from the spec

The spec says "Saved cards never autofill" and "Payments wait on choosing a card issuer". The owner chose Link with the vault card as the fallback on 2026-11-09, so the fallback does fill a saved card, always behind its own approval. The spec needs those two lines changed to match.
