# Payments use Stripe's Link wallet for agents

The agent pays with Stripe's Link wallet for agents, using a one-time card issued for each approved purchase. If that is not available, the fallback is the vault card filled at checkout, with approval every time.

Reason: a card that exists for one approved purchase cannot be reused or spent on anything else, so the approval and the money are tied together. The vault card has no such limit, which is why the fallback never runs without its own approval.

Payments are built last. Nothing earlier waits on them.
