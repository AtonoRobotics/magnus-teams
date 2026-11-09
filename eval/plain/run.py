"""Run the plain-language suite against a Magnus appliance and grade it.

    python3 eval/plain/run.py [--url URL] --token-file ~/.magnus-appliance/magnus.toml [--only id,id] [--trials N] [--parallel N] [--experiment ID]

With a token that holds the developer role, each trial is recorded as an
experiment trial and its grade posted back, so the developer view compares
configurations case by case (V2). Without it, trials are plain tasks.

The runner plays a passive person: it answers questions with "use your best
judgment", accepts review cards, and declines approvals (no task here needs
an external action). Each of those counts as an intervention (V3). Grading
runs in the magnus-grader image and checks delivered files and independent
facts, never the worker's report.
"""
import argparse, base64, concurrent.futures, datetime, json, mimetypes, os, re, subprocess, sys, time, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
TERMINAL = {"completed", "failed", "cancelled"}


def url_from(path):
    """The appliance's plain-HTTP address from its config, reached on loopback."""
    import tomllib
    listen = tomllib.load(open(os.path.expanduser(path), "rb"))["appliance"]["listen"]
    host, port = listen.rsplit(":", 1)
    return f"http://{'127.0.0.1' if host in ('0.0.0.0', '::', '[::]') else host}:{port}"


def token_from(path):
    text = open(os.path.expanduser(path)).read()
    m = re.search(r'roles = \["owner"\]', text)
    toks = re.findall(r'token = "([^"]+)"', text)
    if not toks:
        sys.exit("no token in " + path)
    return toks[0]


class Api:
    def __init__(self, url, token):
        self.url, self.token = url.rstrip("/"), token

    def call(self, method, path, body=None, raw=False):
        req = urllib.request.Request(self.url + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                     headers={"authorization": "Bearer " + self.token, "content-type": "application/json"})
        with urllib.request.urlopen(req, timeout=60) as r:
            data = r.read()
            return data if raw else json.loads(data or b"null")


def run_task(api, t, rdir, timeout_s, exp=None, trial=0):
    os.makedirs(os.path.join(rdir, "outputs"), exist_ok=True)
    atts = []
    for name in t.get("attachments", []):
        p = os.path.join(HERE, "fixtures", name)
        atts.append({"name": name, "media_type": mimetypes.guess_type(name)[0] or "application/octet-stream",
                     "data_b64": base64.b64encode(open(p, "rb").read()).decode()})
    started = time.time()
    if exp:
        tid = api.call("POST", f"/v1/dev/experiments/{exp}/trials", {"case": t["id"], "trial": trial, "text": t["text"], "attachments": atts})["task_id"]
    else:
        tid = api.call("POST", "/v1/tasks", {"text": t["text"], "attachments": atts})["task_id"]
    interventions = []
    handled = set()
    while True:
        task = api.call("GET", f"/v1/tasks/{tid}")
        items = [i for i in api.call("GET", "/v1/inbox")["items"] if i.get("task_id") == tid]
        for i in items:
            if i["status"] != "open" or i["id"] in handled or not i.get("can_respond"):
                continue
            handled.add(i["id"])
            if i["kind"] == "question":
                api.call("POST", f"/v1/inbox/{i['id']}", {"decision": "answer", "text": "I don't have more details. Use your best judgment."})
                interventions.append("answered a question")
            elif i["kind"] == "review":
                api.call("POST", f"/v1/inbox/{i['id']}", {"decision": "accept", "text": "Accepted by the evaluation runner; graded separately."})
                interventions.append("accepted a review")
            elif i["kind"] == "approval":
                api.call("POST", f"/v1/inbox/{i['id']}", {"decision": "reject", "text": "No external actions in this evaluation."})
                interventions.append("declined an approval")
        if task["status"] in TERMINAL:
            break
        if task["status"] == "suspended" and task.get("waiting_on") not in ("approval", "review", "question"):
            interventions.append(f"stopped: {task.get('waiting_on')}")
            break
        if time.time() - started > timeout_s:
            interventions.append("timed out")
            api.call("POST", f"/v1/tasks/{tid}/cancel", {})
            task = api.call("GET", f"/v1/tasks/{tid}")
            break
        time.sleep(3)
    for o in task.get("outputs", []):
        data = api.call("GET", f"/v1/tasks/{tid}/outputs/{o['id']}", raw=True)
        open(os.path.join(rdir, "outputs", os.path.basename(o["name"])), "wb").write(data)
    items = [i for i in api.call("GET", "/v1/inbox")["items"] if i.get("task_id") == tid]
    json.dump(task, open(os.path.join(rdir, "task.json"), "w"), indent=2)
    json.dump(items, open(os.path.join(rdir, "inbox.json"), "w"), indent=2)
    json.dump([s for s in api.call("GET", "/v1/schedules")["schedules"] if s["text"] and "inbox" in s["text"].lower()],
              open(os.path.join(rdir, "schedules.json"), "w"), indent=2)
    json.dump(api.call("GET", f"/v1/tasks/{tid}/timeline"), open(os.path.join(rdir, "timeline.json"), "w"), indent=2)
    return tid, task, interventions, time.time() - started


def grade(t, rdir):
    out = subprocess.run(
        ["docker", "run", "--rm", "-v", f"{rdir}:/r", "-v", f"{HERE}/fixtures:/fixtures:ro", "-v", f"{HERE}:/e:ro",
         "magnus-grader", "python3", "/e/grade.py", t["id"], "/r"],
        capture_output=True, text=True, timeout=600)
    try:
        return json.loads(out.stdout.strip().splitlines()[-1])
    except Exception:
        return {"score": 0.0, "passed": False, "checks": [["grader ran", False, (out.stderr or out.stdout)[-300:]]]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="", help="default: the config's appliance.listen")
    ap.add_argument("--token-file", default="~/.magnus-appliance/magnus.toml")
    ap.add_argument("--only", default="")
    ap.add_argument("--trials", type=int, default=1)
    ap.add_argument("--parallel", type=int, default=4)
    ap.add_argument("--timeout", type=int, default=1500)
    ap.add_argument("--experiment", default="", help="experiment id (default: plain-<time>)")
    a = ap.parse_args()
    api = Api(a.url or url_from(a.token_file), token_from(a.token_file))
    model = api.call("GET", "/v1/model")
    roles = api.call("GET", "/v1/me").get("roles", [])
    tasks = json.load(open(os.path.join(HERE, "tasks.json")))
    if a.only:
        keep = set(a.only.split(","))
        tasks = [t for t in tasks if t["id"] in keep]
    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    root = os.path.join(HERE, "results", stamp)
    jobs = [(t, k) for t in tasks for k in range(a.trials)]
    exp = (a.experiment or f"plain-{stamp}") if "developer" in roles else None
    if not exp:
        print("This token has no developer role: trials run as plain tasks and do not appear as an experiment.", file=sys.stderr)

    def one(job):
        t, k = job
        rdir = os.path.join(root, f"{t['id']}-{k}")
        try:
            tid, task, inter, secs = run_task(api, t, rdir, a.timeout, exp, k)
            g = grade(t, rdir)
            if exp and task.get("run_id"):
                api.call("POST", f"/v1/dev/runs/{task['run_id']}/grades", {"grader": "plain/grade.py", "passed": bool(g["passed"]), "score": g["score"], "details": {"checks": g["checks"]}})
        except Exception as e:
            tid, task, inter, secs, g = None, {}, [f"runner error: {e}"], 0, {"score": 0.0, "passed": False, "checks": [["runner", False, str(e)]]}
        usage = task.get("usage", {})
        r = {"task": t["id"], "trial": k, "category": t["category"], "task_id": tid, "status": task.get("status"),
             "score": g["score"], "passed": g["passed"], "checks": g["checks"], "interventions": inter,
             "model_calls": usage.get("model_calls"), "cost_usd": usage.get("cost_usd"), "seconds": round(secs)}
        print(f"{t['id']}#{k}: {r['status']} score={r['score']} passed={r['passed']} interventions={len(inter)}", flush=True)
        return r

    with concurrent.futures.ThreadPoolExecutor(a.parallel) as ex:
        results = list(ex.map(one, jobs))
    n = len(results)
    summary = {
        "model": model, "when": stamp, "tasks": len(tasks), "trials": a.trials, "experiment": exp,
        "mean_score": round(sum(r["score"] for r in results) / n, 3) if n else 0,
        "pass_rate": round(sum(r["passed"] for r in results) / n, 3) if n else 0,
        "interventions_per_task": round(sum(len(r["interventions"]) for r in results) / n, 2) if n else 0,
        "cost_usd": round(sum(r["cost_usd"] or 0 for r in results), 2),
    }
    os.makedirs(root, exist_ok=True)
    json.dump({"summary": summary, "results": results}, open(os.path.join(root, "report.json"), "w"), indent=2)
    lines = [f"# Plain-language suite, {stamp}", "", f"Model: {model['adapter']} / {model['model']}", "",
             f"Mean score {summary['mean_score']}, pass rate {summary['pass_rate']}, interventions per task {summary['interventions_per_task']}, cost ${summary['cost_usd']}.", "",
             "| Task | Trial | Status | Score | Passed | Interventions | Model calls | Cost |", "|---|---|---|---|---|---|---|---|"]
    for r in sorted(results, key=lambda r: (r["task"], r["trial"])):
        lines.append(f"| {r['task']} | {r['trial']} | {r['status']} | {r['score']} | {r['passed']} | {'; '.join(r['interventions'])} | {r['model_calls']} | {r['cost_usd']} |")
    lines += ["", "Failed checks:", ""]
    for r in results:
        for c in r["checks"]:
            if not c[1]:
                lines.append(f"- {r['task']}#{r['trial']}: {c[0]} ({c[2]})")
    open(os.path.join(root, "report.md"), "w").write("\n".join(lines) + "\n")
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
