"""Ask the appliance a mixed set of chat messages and report how often the
model's choice (answer, task, schedule, or a question back) matched what
each message meant.

    python3 eval/chat/run.py [--url URL] [--parallel N] [--out FILE]

It signs in with the appliance's device-code flow: open the printed link,
approve, and the run starts. The session token stays in this process and is
signed out at the end. Each message gets a thread of its own. A task the
model starts is cancelled at once and a schedule card is declined: only the
choice is measured, so no research runs and nothing is scheduled.
"""
import argparse, concurrent.futures, datetime, json, os, sys, time, urllib.error, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
TERMINAL = {"completed", "failed", "cancelled"}


class Api:
    def __init__(self, url):
        self.url, self.token = url.rstrip("/"), None

    def call(self, method, path, body=None):
        req = urllib.request.Request(self.url + path, method=method, data=None if body is None else json.dumps(body).encode())
        req.add_header("content-type", "application/json")
        # Signs in as an app, so it names an app version the appliance accepts.
        req.add_header("x-magnus-app", "0.5.0")
        if self.token:
            req.add_header("authorization", f"Bearer {self.token}")
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return r.status, json.loads(r.read() or b"{}")
        except urllib.error.HTTPError as e:
            return e.code, json.loads(e.read() or b"{}")

    def sign_in(self):
        st, d = self.call("POST", "/v1/sign-in/device", {"client": "app"})
        if st != 200:
            sys.exit(f"device sign-in could not start ({st}): {d}")
        print(f"Sign in to run the chat eval: open {d.get('verification_uri_complete') or d['verification_uri']} (code {d['user_code']})", flush=True)
        deadline = time.time() + d["expires_in"]
        while time.time() < deadline:
            time.sleep(d.get("interval", 5))
            st, r = self.call("POST", f"/v1/sign-in/device/{d['flow']}")
            if st == 200:
                self.token = r["token"]
                print(f"Signed in as {r['person']['id']}.", flush=True)
                return
            if st == 502:
                print(f"the identity provider did not answer ({r.get('error', '')[:120]}); still waiting", flush=True)
            elif st != 202:
                sys.exit(f"sign-in failed ({st}): {r}")
        sys.exit("the sign-in code expired")

    def sign_out(self):
        self.call("POST", "/v1/sign-out")


def choice(entries):
    """What the model did, read from the thread."""
    kinds = [e["kind"] for e in entries]
    if "schedule_card" in kinds:
        return "schedule"
    if "task" in kinds:
        return "task"
    answer = next((e for e in entries if e["kind"] == "answer"), None)
    if not answer:
        turn = next((e for e in entries if e["kind"] == "turn"), {})
        return f"no answer ({turn.get('status')}: {turn.get('waiting_on') or turn.get('text')})"
    text = (answer.get("text") or "").strip()
    return "asked" if text.endswith("?") else "answer"


def one(api, m):
    st, r = api.call("POST", "/v1/chat", {"text": m["text"]})
    if st != 201:
        return {**m, "got": f"error {st}: {r}"}
    thread, turn = r["thread"], r["task_id"]
    started = time.time()
    while time.time() - started < 600:
        _, t = api.call("GET", f"/v1/tasks/{turn}")
        if t.get("status") in TERMINAL or t.get("status") == "suspended":
            break
        time.sleep(2)
    _, th = api.call("GET", f"/v1/chat/{thread}")
    entries = th.get("entries", [])
    # Measure the choice only: stop any task, decline any card.
    for e in entries:
        if e["kind"] == "task":
            api.call("POST", f"/v1/tasks/{e['task_id']}/cancel")
        if e["kind"] == "schedule_card" and e.get("state") == "open":
            api.call("POST", f"/v1/chat/cards/{e['card']}", {"decision": "decline"})
    detail = next((e for e in entries if e["kind"] in ("schedule_card", "task", "answer")), {})
    _, t = api.call("GET", f"/v1/tasks/{turn}")
    _, tl = api.call("GET", f"/v1/tasks/{turn}/timeline")
    return {
        **m,
        "got": choice(entries),
        "thread": thread,
        "seconds": round(time.time() - started),
        "model_calls": t.get("usage", {}).get("model_calls"),
        "tools": [c["tool"] for s in tl.get("steps", []) for c in s.get("calls", [])],
        "detail": {k: detail.get(k) for k in ("kind", "text", "every_minutes", "daily_at", "scope", "budget") if detail.get(k) is not None},
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="http://127.0.0.1:7701")
    ap.add_argument("--parallel", type=int, default=4)
    ap.add_argument("--out", default=os.path.join(HERE, "results", datetime.datetime.now().strftime("%Y%m%d-%H%M%S") + ".json"))
    a = ap.parse_args()
    messages = json.load(open(os.path.join(HERE, "messages.json")))
    api = Api(a.url)
    api.sign_in()
    try:
        _, versions = api.call("GET", "/v1/versions")
        with concurrent.futures.ThreadPoolExecutor(a.parallel) as pool:
            results = list(pool.map(lambda m: one(api, m), messages))
    finally:
        api.sign_out()
    os.makedirs(os.path.dirname(a.out), exist_ok=True)
    json.dump({"versions": versions, "results": results}, open(a.out, "w"), indent=2)
    matched = [r for r in results if r["got"] == r["meant"]]
    print(f"\nmodel {versions.get('model')}, {versions.get('chat_instructions')}")
    print(f"matched {len(matched)} of {len(results)} ({100 * len(matched) // len(results)}%)")
    for meant in ("answer", "task", "schedule"):
        rs = [r for r in results if r["meant"] == meant]
        got = {}
        for r in rs:
            got[r["got"]] = got.get(r["got"], 0) + 1
        print(f"  meant {meant:8} {sum(r['got'] == meant for r in rs)}/{len(rs)}  got {got}")
    for r in results:
        if r["got"] != r["meant"]:
            print(f"  mismatch {r['id']}: meant {r['meant']}, got {r['got']}: {r['text']}  {json.dumps(r['detail'])[:200]}")
    print(f"results: {a.out}")


if __name__ == "__main__":
    main()
