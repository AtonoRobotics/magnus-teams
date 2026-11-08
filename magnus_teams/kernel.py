"""Deterministic coordination kernel.

Same inputs, same result. Claims, expiry, conflicts and handoffs are
computed here and nowhere else. Orchestrators (if any) only propose.
"""
import json, os, time

TYPES = ("claim", "plan", "touch", "heartbeat", "handoff", "release")

class Store:
    def __init__(self, path):
        self.path = path
        os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)

    def append(self, ev):
        ev = dict(ev)
        ev.setdefault("ts", time.time())
        with open(self.path, "a") as f:
            f.write(json.dumps(ev, sort_keys=True) + "\n")
        return ev

    def events(self):
        if not os.path.exists(self.path):
            return []
        with open(self.path) as f:
            return [json.loads(l) for l in f if l.strip()]

def project(events, now=None):
    now = time.time() if now is None else now
    items = {}
    for ev in events:
        it = items.setdefault(ev["issue"], {"issue": ev["issue"], "owner": None,
            "files": [], "expires": 0, "last_heartbeat": 0, "state": "open",
            "plan": None, "handoffs": []})
        t = ev["type"]
        if t == "claim":
            it.update(owner=ev["actor"], files=ev.get("files", []),
                      expires=ev["ts"] + ev.get("ttl", 3600),
                      last_heartbeat=ev["ts"], state="claimed")
        elif t == "plan":
            it["plan"] = ev.get("text")
            if ev.get("files"): it["files"] = ev["files"]
        elif t == "touch":
            it["state"] = "in-progress"; it["last_heartbeat"] = ev["ts"]
        elif t == "heartbeat":
            it["last_heartbeat"] = ev["ts"]
            it["expires"] = ev["ts"] + ev.get("ttl", 3600)
        elif t == "release":
            it.update(owner=None, state="open", expires=0)
        elif t == "handoff":
            it["handoffs"].append({"actor": ev["actor"], "text": ev.get("text", ""),
                                    "ts": ev["ts"]})
            it.update(owner=None, state="handed-off", expires=0)
    for it in items.values():
        if it["owner"] and it["expires"] and now > it["expires"]:
            it["state"] = "expired"  # claim is free; owner kept for blame
    return items

def _live(it, now):
    return bool(it and it["owner"] and it["expires"] > now)

def claim(store, actor, issue, files=None, ttl=3600, now=None):
    now = time.time() if now is None else now
    items = project(store.events(), now)
    cur = items.get(issue)
    if _live(cur, now) and cur["owner"] != actor:
        return {"granted": False, "reason": "claimed", "owner": cur["owner"],
                "expires": cur["expires"]}
    files = files or []
    conflicts = [{"issue": k, "owner": v["owner"],
                  "files": sorted(set(files) & set(v["files"]))}
                 for k, v in items.items()
                 if k != issue and _live(v, now) and set(files) & set(v["files"])]
    store.append({"type": "claim", "actor": actor, "issue": issue,
                  "files": files, "ttl": ttl, "ts": now})
    return {"granted": True, "conflicts": conflicts}
