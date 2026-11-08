import concurrent.futures, json, os, sys, tempfile, unittest
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from magnus_teams.kernel import Store, project


class StoreValidation(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp()
        self.path = os.path.join(self.d, "e.jsonl")
        self.s = Store(self.path)

    def test_missing_fields_rejected(self):
        for ev in (
            {"actor": "codex", "issue": 1},
            {"type": "claim", "issue": 1},
            {"type": "claim", "actor": "codex"},
            {"type": "", "actor": "codex", "issue": 1},
            {"type": "claim", "actor": "", "issue": 1},
            {"type": "claim", "actor": "codex", "issue": None},
            {},
        ):
            with self.subTest(ev=ev):
                with self.assertRaises(ValueError):
                    self.s.append(ev)

    def test_unknown_type_rejected(self):
        with self.assertRaises(ValueError):
            self.s.append({"type": "explode", "actor": "codex", "issue": 1})

    def test_rejected_event_writes_nothing(self):
        for bad in ({"actor": "codex", "issue": 1},
                    {"type": "nope", "actor": "codex", "issue": 1}):
            with self.assertRaises(ValueError):
                self.s.append(bad)
        self.assertEqual(self.s.events(), [])
        self.assertFalse(os.path.exists(self.path))

    def test_rejection_leaves_existing_record_unchanged(self):
        self.s.append({"type": "claim", "actor": "codex", "issue": 1, "ts": 1000})
        with self.assertRaises(ValueError):
            self.s.append({"type": "claim", "actor": "codex"})
        self.assertEqual(self.s.events(),
                         [{"actor": "codex", "issue": 1, "ts": 1000, "type": "claim"}])

    def test_all_kernel_types_accepted(self):
        for t in ("claim", "plan", "touch", "heartbeat", "handoff", "release"):
            self.s.append({"type": t, "actor": "codex", "issue": 1, "ts": 1000})
        self.assertEqual([e["type"] for e in self.s.events()],
                         ["claim", "plan", "touch", "heartbeat", "handoff", "release"])


class StoreConcurrency(unittest.TestCase):
    def test_concurrent_appends_never_interleave(self):
        d = tempfile.mkdtemp()
        path = os.path.join(d, "e.jsonl")
        store = Store(path)

        def worker(w):
            for i in range(25):
                store.append({"type": "touch", "actor": f"w{w}", "issue": w,
                              "seq": i, "text": "x" * 500, "ts": 1000 + i})

        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as ex:
            list(ex.map(worker, range(8)))

        with open(path) as f:
            lines = f.readlines()
        self.assertEqual(len(lines), 200)
        seen = set()
        for line in lines:
            ev = json.loads(line)  # an interleaved line is not valid JSON
            seen.add((ev["actor"], ev["seq"]))
        self.assertEqual(len(seen), 200)
        self.assertEqual(len(store.events()), 200)


class StoreReplay(unittest.TestCase):
    def test_replay_rebuilds_same_projection(self):
        d = tempfile.mkdtemp()
        path = os.path.join(d, "e.jsonl")
        store = Store(path)
        store.append({"type": "claim", "actor": "codex", "issue": 7,
                      "files": ["a.py"], "ttl": 3600, "ts": 1000})
        store.append({"type": "plan", "actor": "codex", "issue": 7,
                      "text": "do it", "ts": 1001})
        store.append({"type": "touch", "actor": "codex", "issue": 7, "ts": 1002})
        store.append({"type": "heartbeat", "actor": "codex", "issue": 7,
                      "ttl": 3600, "ts": 1003})
        store.append({"type": "claim", "actor": "claude", "issue": 9,
                      "files": ["b.py"], "ttl": 10, "ts": 1000})

        first = project(store.events(), now=1004)
        # Deterministic: same events, same projection, every time.
        self.assertEqual(project(store.events(), now=1004), first)
        # Replay: a fresh store over the same record rebuilds it exactly.
        replayed = project(Store(path).events(), now=1004)
        self.assertEqual(replayed, first)
        self.assertEqual(replayed, {
            7: {"issue": 7, "owner": "codex", "files": ["a.py"],
                "expires": 4603, "last_heartbeat": 1003, "state": "in-progress",
                "plan": "do it", "handoffs": []},
            9: {"issue": 9, "owner": "claude", "files": ["b.py"],
                "expires": 1010, "last_heartbeat": 1000, "state": "claimed",
                "plan": None, "handoffs": []},
        })


if __name__ == "__main__":
    unittest.main()
