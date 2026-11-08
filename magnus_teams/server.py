"""Watcher + status server. Runs on the witness host (container).

GET /            plain status page (who is doing what)
GET /api/status  JSON projection
POST /events     append one coordination event as JSON
"""
import json, os
from http.server import BaseHTTPRequestHandler, HTTPServer
from .kernel import Store, project

STORE = Store(os.environ.get("MAGNUS_TEAMS_STORE", "state/events.jsonl"))

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
        items = project(STORE.events())
        if self.path == "/api/status":
            self._send(200, json.dumps(items, indent=1, sort_keys=True),
                       "application/json")
        elif self.path in ("/", "/health"):
            self._send(200, render(items) if self.path == "/" else "ok",
                       "text/html" if self.path == "/" else "text/plain")
        else:
            self._send(404, "not found", "text/plain")
    def do_POST(self):
        if self.path != "/events":
            self._send(404, "not found", "text/plain"); return
        n = int(self.headers.get("Content-Length", 0))
        try:
            ev = json.loads(self.rfile.read(n) or b"{}")
            assert ev["type"] and ev["actor"] and ev["issue"]
        except Exception:
            self._send(400, "bad event", "text/plain"); return
        STORE.append(ev)
        self._send(201, json.dumps({"recorded": ev["type"]}), "application/json")

if __name__ == "__main__":
    port = int(os.environ.get("PORT", "8080"))
    HTTPServer(("0.0.0.0", port), H).serve_forever()
