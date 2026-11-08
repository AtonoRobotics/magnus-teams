import json, os, sys, tempfile, unittest
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from magnus_teams import record, server
from magnus_teams.github_sync import import_issues
from magnus_teams.kernel import Store


def gh_payload():
    return [
        {"number": 2, "title": "GitHub sync", "state": "OPEN",
         "labels": [{"name": "queue"}], "assignees": [{"login": "gh-user"}],
         "url": "https://example/2"},
        {"number": 3, "title": "Status page", "state": "OPEN",
         "labels": [], "assignees": [], "url": "https://example/3"},
        {"number": 4, "title": "Done thing", "state": "CLOSED",
         "labels": [], "assignees": [{"login": "claude"}],
         "url": "https://example/4"},
    ]


class WorkspaceMergedShape(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp()
        self.store = Store(os.path.join(self.d, "e.jsonl"))

    def test_merged_shape_kernel_wins_gh_extras_kept(self):
        self.store.append({"type": "claim", "actor": "codex", "issue": 2,
                           "files": ["a.py"], "ttl": 3600, "ts": 4102444800})
        ws = record.workspace(self.store, import_issues(gh_payload()))
        self.assertEqual(set(ws), {"rooms", "items", "timeline"})
        # GH-only issue present, closed GH issue absent.
        self.assertIn(3, ws["items"])
        self.assertNotIn(4, ws["items"])
        self.assertEqual(ws["items"][3]["title"], "Status page")
        self.assertEqual(ws["items"][3]["state"], "open")
        # Kernel claim state wins over GH import for issue 2.
        it = ws["items"][2]
        self.assertEqual(it["owner"], "codex")
        self.assertEqual(it["state"], "claimed")
        self.assertEqual(it["files"], ["a.py"])
        self.assertEqual(it["expires"], 4102448400)
        # ...while GH extras survive the merge.
        self.assertEqual(it["title"], "GitHub sync")
        self.assertEqual(it["url"], "https://example/2")
        self.assertEqual(it["labels"], ["queue"])

    def test_workspace_without_gh_items(self):
        self.store.append({"type": "claim", "actor": "codex", "issue": 7,
                           "ts": 1000})
        ws = record.workspace(self.store)
        self.assertEqual(ws["items"][7]["owner"], "codex")
        self.assertEqual(len(ws["timeline"]), 1)

    def test_workspace_accepts_raw_gh_payload(self):
        self.store.append({"type": "claim", "actor": "codex", "issue": 2,
                           "ts": 1000})
        ws = record.workspace(self.store, gh_payload())
        self.assertEqual(ws["items"][2]["owner"], "codex")
        self.assertEqual(ws["items"][2]["title"], "GitHub sync")
        self.assertIn(3, ws["items"])

    def test_rooms_default_build_and_named_room(self):
        self.store.append({"type": "claim", "actor": "codex", "issue": 1,
                           "ts": 1000})
        self.store.append({"type": "plan", "actor": "codex", "issue": 1,
                           "text": "go", "room": "design", "ts": 1001})
        # Decision written via the server allowance carries a room too.
        server.append_event(self.store,
                            {"type": "decision", "actor": "claude",
                             "issue": 1, "text": "ship it",
                             "options": ["ship", "wait"],
                             "room": "design", "ts": 1002})
        ws = record.workspace(self.store)
        self.assertEqual(set(ws["rooms"]), {"build", "design"})
        self.assertEqual(ws["rooms"]["build"]["count"], 1)
        self.assertEqual(ws["rooms"]["build"]["items"], [1])
        self.assertEqual(ws["rooms"]["design"]["name"], "design")
        self.assertEqual(ws["rooms"]["design"]["count"], 2)
        self.assertEqual(ws["rooms"]["design"]["items"], [1])
        # Room timeline is newest-first.
        self.assertEqual(
            [e["type"] for e in ws["rooms"]["design"]["timeline"]],
            ["decision", "plan"])


class Timeline(unittest.TestCase):
    def test_newest_first_order(self):
        events = [{"type": "claim", "actor": "a", "issue": 1, "ts": 100},
                  {"type": "touch", "actor": "a", "issue": 1, "ts": 300},
                  {"type": "plan", "actor": "a", "issue": 1, "ts": 200}]
        out = record.timeline(events)
        self.assertEqual([e["ts"] for e in out], [300, 200, 100])
        # Input order is not mutated.
        self.assertEqual([e["ts"] for e in events], [100, 300, 200])

    def test_cap_defaults_to_100_and_is_applied(self):
        events = [{"type": "touch", "actor": "a", "issue": 1, "ts": i}
                  for i in range(150)]
        out = record.timeline(events)
        self.assertEqual(len(out), 100)
        self.assertEqual(out[0]["ts"], 149)
        self.assertEqual(out[-1]["ts"], 50)
        self.assertEqual(len(record.timeline(events, cap=3)), 3)
        self.assertEqual([e["ts"] for e in record.timeline(events, cap=3)],
                         [149, 148, 147])
        self.assertEqual(len(record.timeline(events, cap=None)), 150)
        self.assertEqual(record.timeline(events, cap=0), [])

    def test_workspace_timeline_is_capped_and_newest_first(self):
        d = tempfile.mkdtemp()
        store = Store(os.path.join(d, "e.jsonl"))
        for i in range(105):
            store.append({"type": "touch", "actor": "a", "issue": 1,
                          "ts": 1000 + i})
        ws = record.workspace(store)
        self.assertEqual(len(ws["timeline"]), 100)
        self.assertEqual(ws["timeline"][0]["ts"], 1104)


class DecisionRoundTrip(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp()
        self.store = Store(os.path.join(self.d, "e.jsonl"))

    def test_decision_via_server_helper_lands_in_workspace(self):
        code, body = server.handle_post_event(self.store, json.dumps({
            "type": "decision", "actor": "claude", "issue": 5,
            "text": "use passkeys?", "options": ["passkeys", "named actor"],
            "room": "build", "ts": 2000}))
        self.assertEqual(code, 201)
        self.assertEqual(body, {"recorded": "decision"})
        events = self.store.events()
        self.assertEqual(len(events), 1)
        self.assertEqual(events[0]["type"], "decision")
        self.assertEqual(events[0]["options"], ["passkeys", "named actor"])
        ws = record.workspace(self.store)
        self.assertEqual(ws["timeline"][0]["type"], "decision")
        self.assertEqual(ws["timeline"][0]["options"],
                         ["passkeys", "named actor"])
        self.assertIn("build", ws["rooms"])

    def test_decision_append_event_direct_and_dict_payload(self):
        code, _ = server.handle_post_event(
            self.store, {"type": "decision", "actor": "codex", "issue": 6,
                         "options": ["a"], "ts": 100})
        self.assertEqual(code, 201)
        rec = server.append_event(self.store,
                                  {"type": "decision", "actor": "codex",
                                   "issue": 6, "options": ["b"], "ts": 101})
        self.assertEqual(rec["type"], "decision")
        self.assertEqual([e["type"] for e in self.store.events()],
                         ["decision", "decision"])

    def test_handler_logic_rejects_bad_and_unknown_events(self):
        for payload in ({"type": "explode", "actor": "a", "issue": 1},
                        {"actor": "a", "issue": 1},
                        {"type": "claim", "issue": 1},
                        {"type": "decision", "actor": "a"},
                        "not json"):
            code, _ = server.handle_post_event(self.store, payload)
            self.assertEqual(code, 400, payload)
        self.assertEqual(self.store.events(), [])

    def test_normal_kernel_event_still_round_trips(self):
        code, body = server.handle_post_event(
            self.store, {"type": "claim", "actor": "codex", "issue": 9,
                         "files": ["x.py"], "room": "ops",
                         "ts": 4102444800})
        self.assertEqual((code, body), (201, {"recorded": "claim"}))
        ws = record.workspace(self.store)
        self.assertEqual(ws["items"][9]["state"], "claimed")
        self.assertIn("ops", ws["rooms"])

    def test_get_workspace_handler_logic(self):
        # Exercise do_GET without a socket: swap the module STORE and
        # call the handler's workspace branch through handle helpers.
        self.store.append({"type": "claim", "actor": "codex", "issue": 1,
                           "ts": 1000})
        old = server.STORE
        server.STORE = self.store
        try:
            ws = record.workspace(server.STORE,
                                    import_issues(gh_payload()))
        finally:
            server.STORE = old
        self.assertEqual(set(ws), {"rooms", "items", "timeline"})
        self.assertEqual(ws["items"][1]["owner"], "codex")


if __name__ == "__main__":
    unittest.main()
