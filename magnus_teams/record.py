"""Workspace record: rooms, timeline and merged status projection.

The append-only event record (see kernel.Store) is the source of
truth. This module derives read-only projections from it:

- ``timeline``: events newest-first, capped.
- ``workspace``: ``{rooms, items, timeline}`` where ``items`` is the
  kernel projection merged over imported GitHub issues, and ``rooms``
  groups events by their ``room`` field (default ``"build"``).
"""
from .kernel import project
from .github_sync import import_issues

DEFAULT_ROOM = "build"

__all__ = ["DEFAULT_ROOM", "timeline", "rooms", "workspace"]


def _ts(ev):
    try:
        return float(ev.get("ts") or 0)
    except (TypeError, ValueError):
        return 0


def timeline(events, cap=100):
    """Return events newest-first, capped at ``cap`` entries.

    Ordering is by ``ts`` descending; events with equal (or missing)
    timestamps keep reverse input order, so the most recently appended
    event wins. ``cap=None`` returns every event. The input is not
    mutated.
    """
    indexed = list(enumerate(events or []))
    indexed.sort(key=lambda pair: (_ts(pair[1]), pair[0]), reverse=True)
    ordered = [ev for _, ev in indexed]
    if cap is None:
        return ordered
    try:
        cap = int(cap)
    except (TypeError, ValueError):
        cap = 100
    if cap < 0:
        cap = 0
    return ordered[:cap]


def _room_name(ev):
    return ev.get("room") or DEFAULT_ROOM


def _sorted_ids(values):
    vals = list(values)
    try:
        return sorted(vals)
    except TypeError:
        return sorted(vals, key=repr)


def rooms(events):
    """Group events by room.

    Returns ``{room_name: {"name", "events", "timeline", "items",
    "count"}}`` where ``events``/``timeline`` are that room's events
    newest-first, ``items`` is the sorted list of issue ids seen in
    the room, and ``count`` is the number of events. Events without a
    ``room`` field belong to :data:`DEFAULT_ROOM`.
    """
    grouped = {}
    for ev in events or []:
        grouped.setdefault(_room_name(ev), []).append(ev)
    out = {}
    for name in sorted(grouped, key=str):
        room_events = timeline(grouped[name], cap=None)
        issue_ids = {ev.get("issue") for ev in grouped[name]
                     if ev.get("issue") is not None}
        out[name] = {
            "name": name,
            "events": room_events,
            "timeline": list(room_events),
            "items": _sorted_ids(issue_ids),
            "count": len(room_events),
        }
    return out


def _events_of(store):
    if store is None:
        return []
    events_fn = getattr(store, "events", None)
    if callable(events_fn):
        return list(events_fn())
    return list(store)


def _gh_dict(gh_items):
    """Normalise ``gh_items`` to an import_issues-shaped dict."""
    if gh_items is None:
        return {}
    if isinstance(gh_items, dict):
        if not gh_items:
            return {}
        # Already-imported shape: {issue: {issue:..., state:...,...}}
        if all(isinstance(v, dict) and "issue" in v
               for v in gh_items.values()):
            return {k: dict(v) for k, v in gh_items.items()}
        # Raw single issue / wrapper dict -> run through import_issues.
        return import_issues(gh_items)
    # Raw payload (list / JSON string) -> import.
    return import_issues(gh_items)


def workspace(store, gh_items=None):
    """Return ``{rooms, items, timeline}`` for a store.

    ``items`` merges ``kernel.project`` over the GitHub import dict
    (``gh_items``, as returned by ``github_sync.import_issues``, or a
    raw payload accepted by it). On conflict the kernel claim state
    wins; GitHub-only extras such as ``title``/``url``/``labels`` are
    preserved. ``timeline`` is newest-first capped at 100, ``rooms``
    is derived from event ``room`` fields (default ``"build"``).
    """
    events = _events_of(store)
    kernel_items = project(events)

    items = {}
    for key, value in _gh_dict(gh_items).items():
        item = dict(value)
        # Copy mutable fields so callers cannot alias our output.
        for field in ("files", "handoffs", "labels"):
            if isinstance(item.get(field), list):
                item[field] = list(item[field])
        items[key] = item

    for key, value in kernel_items.items():
        kernel_item = dict(value)
        for field in ("files", "handoffs"):
            if isinstance(kernel_item.get(field), list):
                kernel_item[field] = list(kernel_item[field])
        if key in items:
            merged = dict(items[key])
            merged.update(kernel_item)  # kernel claim state wins
            items[key] = merged
        else:
            # Defensive: also match a gh key of a different type
            # (e.g. int 5 vs "5") so an issue is never duplicated.
            alt = next((k for k in items if str(k) == str(key)), None)
            if alt is not None:
                merged = dict(items.pop(alt))
                merged.update(kernel_item)
                items[key] = merged
            else:
                items[key] = kernel_item

    return {
        "rooms": rooms(events),
        "items": items,
        "timeline": timeline(events),
    }
