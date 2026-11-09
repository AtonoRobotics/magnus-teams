"""Run the owner's request set (requests.json) on several models as chat
messages, as trials of one experiment, so the Experiments view compares
them: what each model chose (answer, tasks, a schedule, a question back),
policy stops, cost and time per request. The owner judges each choice there.

    python3 eval/requests/run.py [--model ADAPTER/MODEL ...] [--experiment ID]

Each model runs as its own eval person (eval-1, eval-2, ... in the appliance
config, with the developer role), so no model sees another's memory or
chats. Requests run in order, one at a time per model; the models run side
by side. A request is done when its reply and every task it started have
ended or stopped (for a person, or to wait), or after --minutes. Then what
is left is cancelled and any proposed schedule declined: nothing is
approved, nothing is scheduled, and no action that needs approval is taken.
A request with "after" continues that request's conversation.
"""
import argparse, concurrent.futures, datetime, json, os, sys, time, tomllib, urllib.error, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_MODELS = ["codex/gpt-6.1-sol", "codex/gpt-6-luna", "grok/grok-4.7", "grok/grok-4.7-build-fast"]


class Api:
    def __init__(self, url, token):
        self.url, self.token = url.rstrip("/"), token

    def call(self, method, path, body=None):
        req = urllib.request.Request(self.url + path, method=method, data=None if body is None else json.dumps(body).encode())
        req.add_header("content-type", "application/json")
        req.add_header("authorization", f"Bearer {self.token}")
        try:
            with urllib.request.urlopen(req, timeout=120) as r:
                return r.status, json.loads(r.read() or b"{}")
        except urllib.error.HTTPError as e:
            return e.code, json.loads(e.read() or b"{}")


def row_of(api, exp, task):
    st, x = api.call("GET", f"/v1/dev/experiments/{exp}")
    if st != 200:
        return None
    return next((r for r in x.get("requests", []) if r["task"] == task), None)


def tidy(api, row):
    """Stop what the request left behind; record nothing new."""
    for t in row["tasks"]:
        if t["status"] in ("running", "suspended"):
            api.call("POST", f"/v1/tasks/{t['task']}/cancel", {})
    _, turn = api.call("GET", f"/v1/tasks/{row['task']}")
    if turn.get("status") in ("running", "suspended"):
        api.call("POST", f"/v1/tasks/{row['task']}/cancel", {})
    _, th = api.call("GET", f"/v1/chat/{row['thread']}")
    for e in th.get("entries", []):
        if e["kind"] == "schedule_card" and e.get("state") == "open":
            api.call("POST", f"/v1/chat/cards/{e['card']}", {"decision": "decline"})


def one_model(url, token, exp, model, requests, minutes, log):
    api = Api(url, token)
    adapter, name = model.split("/", 1)
    threads, out = {}, []
    for q in requests:
        body = {"case": q["id"], "trial": 1, "text": q["text"], "via": "chat", "model": {"adapter": adapter, "model": name}}
        if q.get("after"):
            body["thread"] = threads[q["after"]]
        st, v = api.call("POST", f"/v1/dev/experiments/{exp}/trials", body)
        if st != 201:
            out.append({"case": q["id"], "model": model, "error": f"{st}: {v}"})
            log(f"{model} #{q['id']}: could not start ({st}: {v})")
            continue
        threads[q["id"]] = v["thread"]
        started, row, timed_out = time.time(), None, False
        while True:
            row = row_of(api, exp, v["task_id"]) or row
            if row and row["settled"]:
                break
            if time.time() - started > minutes * 60:
                timed_out = True
                break
            time.sleep(5)
        if row:
            tidy(api, row)
            out.append({**row, "model": model, "timed_out": timed_out})
            log(f"{model} #{q['id']}: {row['choice']}, {len(row['stops'])} stops, ${row['usd'] or 0:.3f}, {row['seconds']:.0f} s{' (timed out)' if timed_out else ''}")
        else:
            out.append({"case": q["id"], "model": model, "task": v["task_id"], "error": "never reported"})
            log(f"{model} #{q['id']}: never reported")
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="http://127.0.0.1:7701")
    ap.add_argument("--config", default=os.path.expanduser("~/.magnus-appliance/magnus.toml"))
    ap.add_argument("--model", action="append", help="adapter/model; repeat for each (default: the four chosen 2026-10-28)")
    ap.add_argument("--experiment", default="requests-" + datetime.datetime.now().strftime("%Y%m%d-%H%M"))
    ap.add_argument("--minutes", type=float, default=30, help="how long one request may run before it is stopped")
    a = ap.parse_args()
    models = a.model or DEFAULT_MODELS
    requests = json.load(open(os.path.join(HERE, "requests.json")))
    people = {p["id"]: p["token"] for p in tomllib.load(open(a.config, "rb")).get("people", []) if p.get("token")}
    tokens = [people.get(f"eval-{i + 1}") for i in range(len(models))]
    if not all(tokens):
        sys.exit(f"the appliance config needs people eval-1 to eval-{len(models)} with tokens and the developer role")
    _, listed = Api(a.url, tokens[0]).call("GET", "/v1/models")
    offered = {f"{x['adapter']}/{m}" for x in listed.get("adapters", []) for m in x["models"]}
    missing = [m for m in models if m not in offered]
    if missing:
        sys.exit(f"not offered by any connected adapter: {', '.join(missing)}")
    _, versions = Api(a.url, tokens[0]).call("GET", "/v1/versions")
    print(f"experiment {a.experiment}: {len(requests)} requests on {', '.join(models)}", flush=True)
    log = lambda s: print(f"{datetime.datetime.now():%H:%M:%S} {s}", flush=True)
    with concurrent.futures.ThreadPoolExecutor(len(models)) as pool:
        runs = list(pool.map(lambda mt: one_model(a.url, mt[1], a.experiment, mt[0], requests, a.minutes, log), zip(models, tokens)))
    path = os.path.join(HERE, "results", f"{a.experiment}.json")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    json.dump({"experiment": a.experiment, "versions": versions, "models": models, "minutes": a.minutes, "results": [r for rs in runs for r in rs]}, open(path, "w"), indent=2)
    print(f"\nresults: {path}\nJudge each choice in the developer view: Experiments, {a.experiment}.")


if __name__ == "__main__":
    main()
