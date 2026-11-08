import os, sys, tempfile, unittest
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from magnus_teams.kernel import Store, project
from magnus_teams.boundary import execute

class Boundary(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp()
        self.s = Store(os.path.join(self.d, "e.jsonl"))

    def test_intent_precedes_effect(self):
        seen = {}
        def effect():
            evs = self.s.events()
            seen["types"] = [e["type"] for e in evs]
            seen["intent"] = evs[-1] if evs else None
            return 42
        out = execute(self.s, "claude", 11, "send", effect, "k1")
        self.assertEqual(out, 42)
        # Effect observed the intent already recorded, with tool+key.
        self.assertEqual(seen["types"], ["intent"])
        self.assertEqual(seen["intent"]["tool"], "send")
        self.assertEqual(seen["intent"]["idem_key"], "k1")
        self.assertEqual(seen["intent"]["actor"], "claude")
        self.assertEqual(seen["intent"]["issue"], 11)
        # Record order after completion: intent, then result.
        evs = self.s.events()
        self.assertEqual([e["type"] for e in evs], ["intent", "result"])
        self.assertEqual(evs[1]["idem_key"], "k1")
        self.assertEqual(evs[1]["value"], 42)

    def test_dedupe_single_execution(self):
        calls = []
        def effect():
            calls.append(1)
            return "done"
        a = execute(self.s, "claude", 11, "send", effect, "k1")
        b = execute(self.s, "claude", 11, "send", effect, "k1")
        self.assertEqual(a, "done"); self.assertEqual(b, "done")
        self.assertEqual(len(calls), 1)
        # No second intent/result on replay.
        self.assertEqual([e["type"] for e in self.s.events()],
                         ["intent", "result"])
        # A different key executes again.
        self.assertEqual(execute(self.s, "claude", 11, "send", effect, "k2"), "done")
        self.assertEqual(len(calls), 2)

    def test_deny_no_effect(self):
        calls = []
        def effect():
            calls.append(1)
            return "done"
        out = execute(self.s, "mallory", 11, "send", effect, "k1",
                      allowed={"claude", "codex"})
        self.assertEqual(out, {"denied": True})
        self.assertEqual(calls, [])
        evs = self.s.events()
        self.assertEqual([e["type"] for e in evs], ["result"])
        self.assertTrue(evs[0]["denied"])
        self.assertEqual(evs[0]["actor"], "mallory")
        # Allowed actor is not denied.
        self.assertEqual(execute(self.s, "claude", 11, "send", effect, "k2",
                                 allowed={"claude"}), "done")
        self.assertEqual(len(calls), 1)

    def test_project_ignores_intent_and_result(self):
        execute(self.s, "claude", 11, "send", lambda: 1, "k1")
        items = project(self.s.events(), now=9999999999)
        self.assertIn(11, items)  # no crash; unknown fields ignored
        self.assertEqual(items[11]["state"], "open")

if __name__ == "__main__":
    unittest.main()
