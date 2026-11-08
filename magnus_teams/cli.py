"""Five coordination messages, as a CLI harness agents can call.

  claim / plan / touch / heartbeat / handoff  (+ release)
Each prints one JSON line. The GitHub mirror is optional: with
--github REPO it also posts the message as an issue comment.
"""
import argparse, json, subprocess, sys, time
from .kernel import Store, claim, project

def gh_comment(repo, issue, body):
    subprocess.run(["gh", "issue", "comment", str(issue), "--repo", repo,
                    "--body", body], check=False, capture_output=True)

def main(argv=None):
    p = argparse.ArgumentParser(prog="magnus-teams")
    p.add_argument("msg", choices=["claim", "plan", "touch", "heartbeat",
                                   "handoff", "release", "status"])
    p.add_argument("--issue", type=int)
    p.add_argument("--actor", default="unknown")
    p.add_argument("--text", default="")
    p.add_argument("--files", default="", help="comma-separated")
    p.add_argument("--ttl", type=int, default=3600)
    p.add_argument("--store", default="state/events.jsonl")
    p.add_argument("--github", default="", help="owner/repo to mirror to")
    a = p.parse_args(argv)
    store = Store(a.store)
    files = [f for f in a.files.split(",") if f]
    if a.msg == "status":
        print(json.dumps(project(store.events()), indent=1, sort_keys=True))
        return 0
    if a.issue is None:
        print(json.dumps({"error": "--issue required"})); return 2
    if a.msg == "claim":
        out = claim(store, a.actor, a.issue, files, a.ttl)
    else:
        ev = {"type": a.msg, "actor": a.actor, "issue": a.issue,
              "text": a.text, "files": files, "ttl": a.ttl, "ts": time.time()}
        store.append(ev); out = {"recorded": a.msg, "issue": a.issue}
    if a.github:
        gh_comment(a.github, a.issue,
                   f"[magnus-teams] {a.actor} {a.msg}: {a.text} {files}")
    print(json.dumps(out, sort_keys=True))
    return 0 if out.get("granted", True) else 1

if __name__ == "__main__":
    sys.exit(main())
