"""Completion verification.

Criteria are written before acting. Checks re-read the world at
completion time: verify() calls each criterion's check callable
fresh, inside verify, and compares the observed value it returns
against the criterion's expected value. Nothing is cached at
contract-creation time, so a world that changed after the contract
was written is what the verdict follows.

Criteria without a check cannot be decided by code. If every
checked criterion passes but some are unchecked, the verdict is
``needs_review`` so a person (or a different agent) reviews them.
A failed checked criterion makes the verdict ``not_complete``,
with the observed value recorded beside the expected one.
"""
from dataclasses import dataclass, field
from typing import Any, Callable, Optional


@dataclass
class Criterion:
    id: str
    description: str
    check: Optional[Callable[[Any], Any]] = None
    expected: Any = True


@dataclass
class Contract:
    goal: str
    criteria: list = field(default_factory=list)


def verify(contract, world):
    """Check `contract` against the current `world`.

    Returns {"status": "complete"|"not_complete"|"needs_review",
             "results": [{"id", "ok", "observed", "expected"}, ...]}
    where ok is True/False for checked criteria and None for
    unchecked ones.
    """
    results = []
    any_failed = False
    any_unchecked = False
    for crit in contract.criteria:
        if crit.check is None:
            any_unchecked = True
            results.append({"id": crit.id, "ok": None,
                            "observed": None, "expected": crit.expected})
            continue
        observed = crit.check(world)  # fresh read, at verify time
        ok = bool(observed == crit.expected)
        if not ok:
            any_failed = True
        results.append({"id": crit.id, "ok": ok,
                        "observed": observed, "expected": crit.expected})
    if any_failed:
        status = "not_complete"
    elif any_unchecked:
        status = "needs_review"
    else:
        status = "complete"
    return {"status": status, "results": results}
