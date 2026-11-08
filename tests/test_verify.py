import os, sys, unittest
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from magnus_teams.verify import Contract, Criterion, verify


def world_with(**kw):
    return dict(kw)


class Verify(unittest.TestCase):
    def test_complete_when_all_checks_pass(self):
        c = Contract("ship", [
            Criterion("tests", "tests pass",
                      check=lambda w: w["tests"], expected="green"),
            Criterion("count", "two items",
                      check=lambda w: len(w["items"]), expected=2),
        ])
        r = verify(c, world_with(tests="green", items=[1, 2]))
        self.assertEqual(r["status"], "complete")
        self.assertEqual([x["ok"] for x in r["results"]], [True, True])

    def test_fresh_read_world_changed_after_contract(self):
        # Contract is created while the world is failing; the world
        # changes afterwards. verify must follow the new world, not
        # a value cached at contract-creation time.
        world = {"tests": "red"}
        calls = []

        def check(w):
            calls.append(w["tests"])
            return w["tests"]

        c = Contract("ship", [
            Criterion("tests", "tests pass", check=check, expected="green"),
        ])
        self.assertEqual(verify(c, world)["status"], "not_complete")
        world["tests"] = "green"  # world changes after contract creation
        r = verify(c, world)
        self.assertEqual(r["status"], "complete")
        self.assertEqual(r["results"][0]["observed"], "green")
        # The callable really ran again inside the second verify.
        self.assertEqual(calls, ["red", "green"])

    def test_unchecked_goes_to_needs_review(self):
        c = Contract("ship", [
            Criterion("tests", "tests pass",
                      check=lambda w: w["tests"], expected="green"),
            Criterion("look", "looks right to a person"),  # no check
        ])
        r = verify(c, world_with(tests="green"))
        self.assertEqual(r["status"], "needs_review")
        unchecked = [x for x in r["results"] if x["id"] == "look"][0]
        self.assertIsNone(unchecked["ok"])
        self.assertIsNone(unchecked["observed"])

    def test_all_unchecked_is_needs_review(self):
        c = Contract("ship", [Criterion("look", "looks right")])
        self.assertEqual(verify(c, {})["status"], "needs_review")

    def test_failure_detail_present(self):
        c = Contract("ship", [
            Criterion("tests", "tests pass",
                      check=lambda w: w["tests"], expected="green"),
        ])
        r = verify(c, world_with(tests="3 failed"))
        self.assertEqual(r["status"], "not_complete")
        res = r["results"][0]
        self.assertEqual(res["id"], "tests")
        self.assertFalse(res["ok"])
        self.assertEqual(res["observed"], "3 failed")
        self.assertEqual(res["expected"], "green")

    def test_failure_beats_unchecked(self):
        c = Contract("ship", [
            Criterion("tests", "tests pass",
                      check=lambda w: w["tests"], expected="green"),
            Criterion("look", "looks right to a person"),
        ])
        r = verify(c, world_with(tests="red"))
        self.assertEqual(r["status"], "not_complete")

    def test_default_expected_is_true(self):
        c = Contract("ship", [
            Criterion("ready", "is ready", check=lambda w: w["ready"]),
        ])
        self.assertEqual(verify(c, {"ready": True})["status"], "complete")
        self.assertEqual(verify(c, {"ready": False})["status"], "not_complete")


if __name__ == "__main__":
    unittest.main()
