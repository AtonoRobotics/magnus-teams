"""Magnus worker: red-build watcher (network-free core).

``watch_once`` takes an already-fetched list of CI runs (from an
injected provider) and raises a decision event for each new failure.
It never auto-fixes: a person picks an option on the decision card.
"""
__all__ = ["watch_once"]


def watch_once(store, runs, seen=set()):
    """Raise one decision event per new failed run.

    Args:
        store: a ``kernel.Store`` (anything with ``append``).
        runs: iterable of dicts ``{id, conclusion, name, url}``.
        seen: set of run ids already reported; mutated in place.

    Returns:
        List of new decision events appended to ``store``.
    """
    if seen is None:
        seen = set()
    new_events = []
    for run in runs or []:
        if not isinstance(run, dict):
            continue
        if run.get("conclusion") != "failure":
            continue
        run_id = run.get("id")
        if run_id is None or run_id in seen:
            continue
        name = run.get("name") or "CI run"
        url = run.get("url") or ""
        text = f"Red build: {name} failed (run {run_id})"
        if url:
            text = f"{text} {url}"
        ev = store.append({
            "type": "decision",
            "actor": "magnus-worker",
            "issue": 0,
            "room": "build",
            "text": text,
            "options": ["Open fix issue", "Ignore once"],
            "run_id": run_id,
        })
        seen.add(run_id)
        new_events.append(ev)
    return new_events

def _runs_from_github(repo):
    import urllib.request
    url = f"https://api.github.com/repos/{repo}/actions/runs?per_page=20"
    req = urllib.request.Request(url, headers={"User-Agent": "magnus-teams-worker"})
    with urllib.request.urlopen(req, timeout=30) as r:
        data = json.load(r)
    return [{"id": x["id"], "conclusion": x.get("conclusion"),
             "name": x.get("name"), "url": x.get("html_url")}
            for x in data.get("workflow_runs", [])]

if __name__ == "__main__":
    import json, os, time
    from .kernel import Store
    repo = os.environ.get("MAGNUS_TEAMS_REPO", "AtonoRobotics/magnus-teams")
    store = Store(os.environ.get("MAGNUS_TEAMS_STORE", "state/events.jsonl"))
    seen = {e.get("run_id") for e in store.events() if e.get("run_id")}
    while True:
        try:
            new = watch_once(store, _runs_from_github(repo), seen)
            if new: print(f"worker: raised {len(new)} decision card(s)", flush=True)
        except Exception as e:
            print(f"worker: {e}", flush=True)
        time.sleep(int(os.environ.get("WORKER_INTERVAL", "300")))
