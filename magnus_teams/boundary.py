"""Effect boundary: intent before effect, idempotency, denial.

Every externally-visible effect goes through ``execute``. The intent
is recorded in the append-only store *before* the effect runs, so a
crash between intent and result is visible and can be reconciled.
A result recorded under an idempotency key is replayed verbatim
without re-executing the effect and without a second intent.
A denied actor gets a recorded denial result and no effect runs.
"""
__all__ = ["execute"]


def execute(store, actor, issue, tool, effect, idem_key, allowed=None):
    """Run ``effect()`` once for ``idem_key``, recording intent/result.

    Returns the effect's value, the recorded value on a replay, or
    ``{"denied": True}`` when ``allowed`` is given and ``actor`` is
    not in it (the effect is never called in that case).
    """
    for ev in store.events():
        if ev.get("type") == "result" and ev.get("idem_key") == idem_key:
            if ev.get("denied"):
                return {"denied": True}
            return ev.get("value")
    if allowed is not None and actor not in allowed:
        store.append({"type": "result", "actor": actor, "issue": issue,
                      "tool": tool, "idem_key": idem_key, "denied": True})
        return {"denied": True}
    store.append({"type": "intent", "actor": actor, "issue": issue,
                  "tool": tool, "idem_key": idem_key})
    value = effect()
    store.append({"type": "result", "actor": actor, "issue": issue,
                  "tool": tool, "idem_key": idem_key, "value": value})
    return value
