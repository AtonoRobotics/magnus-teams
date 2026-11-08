import os, sys, tempfile, unittest
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from magnus_teams.kernel import Store
from magnus_teams.worker import watch_once


def run(rid, conclusion, name="CI", url="https://example/1"):
    return {"id": rid, "conclusion": conclusion, "name": name, "url": url}


class WatchOnce(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp()
        self.store = Store(os.path.join(self.d, "e.jsonl"))

    def test_green_silent(self):
        seen = set()
        out = watch_once(self.store, [run(1, "success"), run(2, None),
                                      run(3, "cancelled")], seen)
        self.assertEqual(out, [])
        self.assertEqual(self.store.events(), [])
        self.assertEqual(seen, set())

    def test_red_raises_exactly_once_on_repeat(self):
        seen = set()
        runs = [run(10, "failure")]
        first = watch_once(self.store, runs, seen)
        self.assertEqual(len(first), 1)
        ev = first[0]
        self.assertEqual(ev["type"], "decision")
        self.assertEqual(ev["actor"], "magnus-worker")
        self.assertEqual(ev["issue"], 0)
        self.assertEqual(ev["room"], "build")
        self.assertEqual(ev["options"], ["Open fix issue", "Ignore once"])
        self.assertEqual(ev["run_id"], 10)
        self.assertIn("CI", ev["text"])
        # Repeat with same seen: no duplicate.
        second = watch_once(self.store, runs, seen)
        self.assertEqual(second, [])
        self.assertEqual(len(self.store.events()), 1)
        self.assertEqual(seen, {10})

    def test_multiple_reds_each_raise(self):
        seen = set()
        runs = [run(1, "failure", name="tests"),
                run(2, "success"),
                run(3, "failure", name="build")]
        out = watch_once(self.store, runs, seen)
        self.assertEqual([e["run_id"] for e in out], [1, 3])
        self.assertEqual(len(self.store.events()), 2)
        self.assertEqual(seen, {1, 3})
        # All already seen on repeat.
        self.assertEqual(watch_once(self.store, runs, seen), [])
        self.assertEqual(len(self.store.events()), 2)


if __name__ == "__main__":
    unittest.main()
