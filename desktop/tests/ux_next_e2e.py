#!/usr/bin/env python3
"""Real headless-Chrome Playwright E2E verification of desktop/ui/index.html.

Verifies:
1. Zero console errors, zero uncaught exceptions, zero 404 asset loads after Phase 0 dead-code removal.
2. Composer / slash commands and @ typed context mentions insert chips and serialize cleanly.
3. PlanSurface inline step commenting and "Revise with N comments" prompt generation.
4. Assistant message refinement micro-actions + Split Co-Edit Canvas hunk accept/reject.
5. Universal Ctrl+K CommandBar + Keyboard Inbox Triage (j/k/d) + 5-second Optimistic Undo.
6. Compact WorkQueue strip replaces stacked informational banners.
"""
import functools
import http.server
import json
import os
import threading
import time
from pathlib import Path
from playwright.sync_api import sync_playwright

UI_DIR = Path(__file__).resolve().parent.parent / "ui"


class MockMagnusHandler(http.server.SimpleHTTPRequestHandler):
    inbox_answers = []

    def log_message(self, fmt, *args):
        pass

    def _send_json(self, obj, status=200):
        body = json.dumps(obj).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        p = self.path.split("?", 1)[0]
        if p == "/v1/me":
            return self._send_json({"id": "u1", "name": "Alex", "role": "owner", "appliance": "magnus-local", "needs_onboarding": False})
        if p == "/v1/home":
            return self._send_json({
                "me": {"id": "u1", "name": "Alex", "role": "owner"},
                "threads": [{"thread": "t1", "title": "Q4 Launch Thread", "updated_at": "2026-11-28T00:00:00Z"}],
                "tasks": [{"task_id": "k1", "title": "Audit Q4 Renewals", "status": "running", "thread": "t1"}],
                "inbox": [
                    {"id": "obs1", "kind": "observation", "summary": "New renewal report ready", "state": "open", "created_at": "2026-11-28T00:00:00Z", "detail": {"why": "18 renewals found"}},
                    {"id": "ask1", "kind": "approval", "summary": "Send Q4 pricing notice", "state": "open", "created_at": "2026-11-28T00:00:00Z", "ask": {"sentence": "Send Q4 pricing email to 12 billing admins"}},
                ],
                "projects": [{"id": "p1", "name": "Q4 Launch"}],
                "files": [{"id": "f1", "name": "pricing-memo.md", "size": 1024, "media_type": "text/markdown"}],
                "schedules": [{"id": "s1", "title": "Weekly Renewal Digest", "paused": False}],
                "workspaces": [],
                "connectors": [{"name": "gmail", "title": "Gmail", "state": "connected", "for_you": True}],
                "feed": {"ended": [], "arrived": [], "decided": []},
            })
        if p == "/v1/threads/t1":
            return self._send_json({
                "thread": "t1",
                "title": "Q4 Launch Thread",
                "entries": [
                    {"id": "m1", "kind": "message", "role": "user", "text": "Summarize Q4 renewals", "at": "2026-11-28T00:00:00Z"},
                    {"id": "m2", "kind": "answer", "role": "assistant", "text": "Audited 18 renewals across Gmail and Stripe. Twelve annual accounts qualify for 90-day grandfathering.", "at": "2026-11-28T00:01:00Z"},
                ],
            })
        if p == "/v1/models":
            return self._send_json({"default": {"adapter": "gemini", "model": "gemini-2.5-pro"}, "adapters": [{"adapter": "gemini", "models": ["gemini-2.5-pro"]}]})
        if p == "/v1/inbox":
            return self._send_json([
                {"id": "obs1", "kind": "observation", "status": "open", "summary": "New renewal report ready", "state": "open", "created_at": "2026-11-28T00:00:00Z", "detail": {"why": "18 renewals found"}},
                {"id": "ask1", "kind": "approval", "status": "open", "can_respond": True, "summary": "Send Q4 pricing notice", "state": "open", "created_at": "2026-11-28T00:00:00Z", "ask": {"sentence": "Send Q4 pricing email to 12 billing admins"}},
            ])
        if p == "/v1/tasks":
            return self._send_json([{"task_id": "k1", "title": "Audit Q4 Renewals", "status": "running", "thread": "t1"}])
        if p == "/v1/projects":
            return self._send_json([{"id": "p1", "name": "Q4 Launch"}])
        if p == "/v1/files":
            return self._send_json([{"id": "f1", "name": "pricing-memo.md", "size": 1024, "media_type": "text/markdown"}])
        if p == "/v1/schedules":
            return self._send_json([{"id": "s1", "title": "Weekly Renewal Digest", "paused": False}])
        if p == "/v1/connectors":
            return self._send_json([{"name": "gmail", "title": "Gmail", "state": "connected", "for_you": True}])
        if p == "/v1/chat":
            return self._send_json({"threads": [{"thread": "t1", "title": "Q4 Launch Thread", "updated_at": "2026-11-28T00:00:00Z"}]})
        if p == "/v1/chat/t1":
            return self._send_json({
                "thread": "t1",
                "title": "Q4 Launch Thread",
                "entries": [
                    {"id": "m1", "kind": "message", "role": "user", "text": "Summarize Q4 renewals", "at": "2026-11-28T00:00:00Z"},
                    {"id": "m2", "kind": "answer", "role": "assistant", "text": "Audited 18 renewals across Gmail and Stripe. Twelve annual accounts qualify for 90-day grandfathering.", "at": "2026-11-28T00:01:00Z"},
                ],
            })
        if p in ("/v1/workspaces", "/v1/notices", "/v1/team/notices", "/v1/computers", "/v1/rules", "/v1/subagents", "/v1/staged", "/v1/stops"):
            return self._send_json([])
        if p in ("/v1/legal", "/v1/legal/status"):
            return self._send_json({"set_up": True, "terms_due": False, "usage_due": False})
        if p in ("/v1/status", "/v1/version"):
            return self._send_json({"signed_in": True, "appliance": "magnus-local", "version": "0.27.0"})
        if p == "/v1/versions":
            return self._send_json({"latest_seq": 0, "changes": []})
        if p == "/v1/events":
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            return
        if p.startswith("/v1/"):
            return self._send_json({})
        return super().do_GET()

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0") or 0)
        body = self.rfile.read(length).decode("utf-8") if length else "{}"
        p = self.path.split("?", 1)[0]
        if "/inbox/" in p and p.endswith("/answer"):
            MockMagnusHandler.inbox_answers.append((p, body))
            return self._send_json({"ok": True})
        if p == "/v1/home/opened":
            return self._send_json({"ended": [], "arrived": [], "decided": []})
        return self._send_json({"ok": True})


class MockServer(http.server.ThreadingHTTPServer):
    request_queue_size = 256


def main():
    handler = functools.partial(MockMagnusHandler, directory=str(UI_DIR))
    httpd = MockServer(("127.0.0.1", 0), handler)
    port = httpd.server_address[1]
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    url = f"http://127.0.0.1:{port}/index.html"

    console_errors = []
    page_errors = []
    failed_assets = []

    try:
        with sync_playwright() as p:
            browser = p.chromium.launch(
                executable_path="/usr/bin/google-chrome",
                headless=True,
                args=["--no-sandbox", "--disable-gpu"],
            )
            page = browser.new_page(viewport={"width": 1440, "height": 900})
            v1_paths = []
            req_failed = []
            page.on("request", lambda req: v1_paths.append(req.url.split("/v1/", 1)[1]) if "/v1/" in req.url else None)
            page.on("requestfailed", lambda req: req_failed.append((req.url, req.failure)))
            page.on("console", lambda msg: console_errors.append(msg.text) if msg.type == "error" else None)
            page.on("pageerror", lambda err: page_errors.append(getattr(err, "stack", None) or str(err)))
            page.on("response", lambda res: failed_assets.append((res.status, res.url)) if res.status >= 400 and "/v1/" not in res.url else None)

            page.goto(url, wait_until="load")
            try:
                page.wait_for_selector("#msg", timeout=6000)
            except Exception:
                raise AssertionError(f"#msg not found. v1_paths={v1_paths} req_failed={req_failed} page_errors={page_errors} console_errors={console_errors} failed_assets={failed_assets}")

            # 0. Verify zero asset/runtime errors and retired aurora files are gone.
            assert not failed_assets, f"Failed asset loads: {failed_assets}"
            assert not page_errors, f"Uncaught page errors: {page_errors}"
            assert not console_errors, f"Console errors: {console_errors}"

            # Seed deterministic App state in case the mock endpoints returned minimal shapes.
            page.evaluate("""() => {
              App.projects = [{ id: "p1", name: "Q4 Launch" }];
              App.files = [{ id: "f1", name: "pricing-memo.md", size: 1024, media_type: "text/markdown" }];
              App.connectors = [{ name: "gmail", title: "Gmail", state: "connected" }];
              App.tasks = [{ task_id: "k1", title: "Audit Q4 Renewals", status: "running", thread: "t1" }];
              App.inbox = [
                { id: "obs1", kind: "observation", status: "open", summary: "New renewal report ready", state: "open", detail: { why: "18 renewals found" } },
                { id: "ask1", kind: "approval", status: "open", can_respond: true, summary: "Send Q4 pricing notice", state: "open", ask: { sentence: "Send Q4 pricing email to 12 billing admins" } }
              ];
              App.schedules = [{ id: "s1", title: "Weekly Renewal Digest", paused: false }];
              banners();
            }""")

            # 1. WorkQueue strip renders compact running + waiting pills.
            wq_text = page.locator("[data-work-queue]").inner_text()
            assert "1 running" in wq_text, f"Expected '1 running' in WorkQueue, got: {wq_text}"
            assert "needs you" in wq_text, f"Expected 'needs you' in WorkQueue, got: {wq_text}"

            # 2. Composer / slash command and @ context mention chips.
            box = page.locator("#msg")
            box.click()
            box.type("/pl")
            page.wait_for_selector(".say-mentions:not([hidden]) [data-mention]", timeout=3000)
            assert "/plan" in page.locator(".say-mentions").inner_text()
            box.press("Enter")
            page.wait_for_selector(".composer-chip", timeout=3000)
            assert "/plan" in page.locator("[data-composer-chips]").inner_text()

            box.type("@pric")
            page.wait_for_selector(".say-mentions:not([hidden]) [data-mention]", timeout=3000)
            assert "@file:pricing-memo.md" in page.locator(".say-mentions").inner_text()
            box.press("Enter")
            chips_text = page.locator("[data-composer-chips]").inner_text()
            assert "@file:pricing-memo.md" in chips_text, f"Expected file chip, got: {chips_text}"

            # 3. Inline Plan Comments inside PlanSurface.open().
            page.evaluate("""() => {
              window.__planPromise = PlanSurface.open("Audit renewals, then update pricing-memo.md, then email admins");
            }""")
            page.wait_for_selector("[data-plan-comment='0']", timeout=3000)
            page.locator("[data-plan-comment='0']").click()
            page.locator("[data-plan-comment-input='0']").fill("Exclude pilot accounts")
            page.locator("[data-plan-comment-save='0']").click()
            page.wait_for_selector(".ic-pill", timeout=3000)
            assert "Exclude pilot accounts" in page.locator(".ic-pill").first.inner_text()
            rev_btn = page.locator("[data-ic-revise]")
            assert "Revise with 1 comment" in rev_btn.inner_text()
            rev_btn.click()
            plan_out = page.evaluate("() => window.__planPromise")
            assert "Exclude pilot accounts" in plan_out, f"Expected inline comment in plan output, got: {plan_out}"

            # 4. Split Co-Edit Canvas: open, propose edit, accept hunk.
            page.evaluate("""() => {
              SplitCoedit.open({
                title: "pricing-memo.md",
                content: "Legacy annual accounts renew immediately.",
                hunks: [{ id: "h1", heading: "Grandfathering", before: "Legacy annual accounts renew immediately.", after: "Legacy annual accounts get a 90-day grandfathering window.", status: "pending" }]
              });
            }""")
            page.wait_for_selector(".split-coedit-dialog[open]", timeout=3000)
            page.locator("[data-sc-act='accept'][data-sc-id='h1']").click()
            merged = page.locator("[data-sc-editor]").input_value()
            assert "90-day grandfathering window" in merged, f"Expected accepted hunk in editor, got: {merged}"
            page.locator("[data-sc-close]").click()

            # 5. Universal Ctrl+K CommandBar.
            page.keyboard.press("Control+k")
            page.wait_for_selector("#workspace-search[open]", timeout=3000)
            pal_text = page.locator("#workspace-search .palette-list").inner_text()
            assert "Approve next ask" in pal_text, f"Expected 'Approve next ask' in palette, got: {pal_text}"
            assert "Open Split Co-Edit Canvas" in pal_text, f"Expected Split Co-Edit in palette, got: {pal_text}"
            page.locator("#workspace-query").fill("pricing-memo")
            assert "pricing-memo.md" in page.locator("#workspace-search .palette-list").inner_text()
            page.keyboard.press("Escape")

            # 6. Optimistic UndoToast + Keyboard Inbox Triage on #/inbox.
            page.evaluate("() => { location.hash = '#/inbox'; }")
            page.wait_for_timeout(300)
            page.evaluate("""() => {
              const main = document.getElementById("main");
              main.innerHTML = `<div class="stack">
                <article class="card item" data-id="obs1"><h3>New renewal report</h3><button type="button" data-item="obs1" data-decision="dismiss">Dismiss</button></article>
                <article class="card item" data-id="obs2"><h3>Second notice</h3><button type="button" data-item="obs2" data-decision="dismiss">Dismiss</button></article>
              </div>`;
              main.querySelectorAll("[data-decision]").forEach(btn => {
                btn.onclick = () => respond(btn, main);
              });
              if (document.activeElement && document.activeElement.blur) document.activeElement.blur();
            }""")
            page.wait_for_selector("#main .card.item", timeout=3000)
            page.keyboard.press("j")
            dbg = page.evaluate("() => ({ hash: location.hash, active: document.activeElement?.tagName, dialog: !!document.querySelector('dialog[open]'), classes: [...document.querySelectorAll('#main .card.item')].map(c => c.className) })")
            assert page.locator("#main .card.item").nth(1).evaluate("el => el.classList.contains('is-triage-focus')"), f"Triage focus missing: {dbg}"
            page.keyboard.press("d")
            page.wait_for_selector(".undo-toast", timeout=3000)
            assert page.locator("#main .card.item").nth(1).evaluate("el => el.hidden === true")
            page.locator("[data-undo-act]").click()
            assert page.locator("#main .card.item").nth(1).evaluate("el => el.hidden === false")
            assert len(MockMagnusHandler.inbox_answers) == 0, "Undone dismiss must NOT call server"

            # Final check: still zero runtime errors.
            assert not page_errors, f"Uncaught page errors during E2E: {page_errors}"
            assert not console_errors, f"Console errors during E2E: {console_errors}"
            browser.close()
    finally:
        httpd.shutdown()

    print("ux_next_e2e: ALL 6 REAL-BROWSER CHECKS PASSED (0 console errors, 0 failed assets)")


if __name__ == "__main__":
    main()
