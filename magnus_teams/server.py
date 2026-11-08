"""Watcher + status server. Runs on the witness host (container).

GET /            plain status page (who is doing what)
GET /api/status  JSON projection
GET /api/workspace  JSON {rooms, items, timeline} (see record.py)
POST /events     append one coordination event as JSON
"""
import fcntl, json, os, time
from http.server import BaseHTTPRequestHandler, HTTPServer
from . import kernel
from .kernel import Store, project
from .record import workspace

# kernel.py is not edited for this phase, so the "decision" event
# type is admitted here, server-side: extending kernel.TYPES in
# memory makes Store validation accept decisions in this process,
# and append_event below additionally writes decisions through a
# local helper if that extension is ever not in effect.
EXTRA_TYPES = ("decision",)
if "decision" not in kernel.TYPES:
    kernel.TYPES = tuple(kernel.TYPES) + ("decision",)

STORE = Store(os.environ.get("MAGNUS_TEAMS_STORE", "state/events.jsonl"))


def _validate_event(ev):
    if not isinstance(ev, dict):
        raise ValueError("event must be a dict")
    if not ev.get("type"):
        raise ValueError("missing type")
    if not ev.get("actor"):
        raise ValueError("missing actor")
    if ev.get("issue") is None or ev.get("issue") == "":
        raise ValueError("missing issue")


def _append_unchecked(store, ev):
    """Append ``ev`` to ``store`` bypassing kernel type validation.

    Mirrors kernel.Store.append (locked, fsynced, sorted JSON, ``ts``
    default) for the extra types this server admits.
    """
    ev = dict(ev)
    ev.setdefault("ts", time.time())
    line = json.dumps(ev, sort_keys=True) + "\n"
    with open(store.path, "a") as f:
        fcntl.flock(f.fileno(), fcntl.LOCK_EX)
        try:
            f.write(line)
            f.flush()
            os.fsync(f.fileno())
        finally:
            fcntl.flock(f.fileno(), fcntl.LOCK_UN)
    return ev


def append_event(store, ev):
    """Validate and append one event; returns the recorded event.

    Kernel types go through ``store.append``. The server-admitted
    ``decision`` type (which may carry ``options`` and a ``room``)
    is appended via the local helper if the store's validation
    rejects it. Unknown types raise ``ValueError``.
    """
    _validate_event(ev)
    if ev["type"] in kernel.TYPES and ev["type"] not in EXTRA_TYPES:
        return store.append(ev)
    if ev["type"] in EXTRA_TYPES:
        try:
            return store.append(ev)
        except ValueError:
            return _append_unchecked(store, ev)
    # Fall back to the store so its error message wins for bad types.
    return store.append(ev)


# Aliases for unit-level callers / tests.
record_event = append_event
accept_event = append_event


def handle_post_event(store, payload):
    """Unit-level POST /events logic: payload is dict or JSON bytes/str.

    Returns ``(status_code, body_dict)`` without any socket involved.
    """
    try:
        if isinstance(payload, (bytes, bytearray)):
            payload = payload.decode() if payload.strip() else "{}"
        if isinstance(payload, str):
            ev = json.loads(payload) if payload.strip() else {}
        else:
            ev = payload
        assert isinstance(ev, dict)
        assert ev.get("type") and ev.get("actor") \
            and ev.get("issue") is not None and ev.get("issue") != ""
    except Exception:
        return 400, {"error": "bad event"}
    try:
        recorded = append_event(store, ev)
    except ValueError:
        return 400, {"error": "bad event"}
    return 201, {"recorded": recorded["type"]}


def render(items):
    rows = "".join(
        f"<tr><td>#{i['issue']}</td><td>{i['owner'] or '—'}</td>"
        f"<td>{i['state']}</td><td>{', '.join(i['files'])}</td></tr>"
        for i in sorted(items.values(), key=lambda x: x["issue"]))
    return ("<html><head><title>magnus-teams</title></head><body>"
            "<h1>magnus-teams — who is doing what</h1>"
            "<table border=1 cellpadding=6><tr><th>Issue</th><th>Owner</th>"
            f"<th>State</th><th>Files</th></tr>{rows}</table></body></html>")

class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def _send(self, code, body, ctype):
        b = body.encode()
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(b)))
        self.end_headers(); self.wfile.write(b)
    def do_GET(self):
        path = self.path.split("?", 1)[0]
        web = {"/": "index.html", "/index.html": "index.html",
               "/app.js": "app.js", "/style.css": "style.css"}
        if path in web:
            here = os.path.join(os.path.dirname(__file__), "web", web[path])
            if os.path.exists(here):
                ctype = ("text/html" if here.endswith(".html") else
                         "text/javascript" if here.endswith(".js") else "text/css")
                with open(here, encoding="utf-8") as f:
                    self._send(200, f.read(), ctype)
                return
        if path == "/api/workspace":
            self._send(200, json.dumps(workspace(STORE), indent=1,
                                       sort_keys=True),
                       "application/json")
            return
        items = project(STORE.events())
        if path == "/api/status":
            self._send(200, json.dumps(items, indent=1, sort_keys=True),
                       "application/json")
        elif path in ("/", "/health"):
            self._send(200, render(items) if path == "/" else "ok",
                       "text/html" if path == "/" else "text/plain")
        else:
            self._send(404, "not found", "text/plain")
    def do_POST(self):
        if self.path.split("?", 1)[0] != "/events":
            self._send(404, "not found", "text/plain"); return
        n = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(n) if n else b"{}"
        code, body = handle_post_event(STORE, raw)
        if code != 201:
            self._send(400, "bad event", "text/plain"); return
        self._send(201, json.dumps(body), "application/json")

if __name__ == "__main__":
    port = int(os.environ.get("PORT", "8080"))
    HTTPServer(("0.0.0.0", port), H).serve_forever()
