import os, sys, tempfile, unittest
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from magnus_teams.kernel import Store, claim, project

class Kernel(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp()
        self.s = Store(os.path.join(self.d, "e.jsonl"))

    def test_first_claim_wins(self):
        self.assertTrue(claim(self.s, "codex", 42, ["a.py"], now=1000)["granted"])
        r = claim(self.s, "claude", 42, ["a.py"], now=1001)
        self.assertFalse(r["granted"]); self.assertEqual(r["owner"], "codex")

    def test_expired_claim_is_free(self):
        claim(self.s, "codex", 42, ttl=10, now=1000)
        self.assertTrue(claim(self.s, "claude", 42, now=2000)["granted"])

    def test_same_actor_reclaims(self):
        claim(self.s, "codex", 42, now=1000)
        self.assertTrue(claim(self.s, "codex", 42, now=1001)["granted"])

    def test_file_conflict_reported(self):
        claim(self.s, "codex", 1, ["x.rs"], now=1000)
        r = claim(self.s, "claude", 2, ["x.rs", "y.rs"], now=1001)
        self.assertTrue(r["granted"])
        self.assertEqual(r["conflicts"][0]["files"], ["x.rs"])

    def test_handoff_frees_and_records(self):
        claim(self.s, "codex", 7, now=1000)
        self.s.append({"type": "handoff", "actor": "codex", "issue": 7,
                       "text": "done, tests green", "ts": 1002})
        it = project(self.s.events(), now=1003)[7]
        self.assertEqual(it["state"], "handed-off")
        self.assertIsNone(it["owner"])
        self.assertEqual(it["handoffs"][0]["text"], "done, tests green")

if __name__ == "__main__":
    unittest.main()
