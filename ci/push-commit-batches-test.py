import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("push-commit-batches.py").resolve()


class PushCommitBatchesTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.repo = Path(directory.name)
        self.git("init", "-q")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.com")

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.repo, text=True).strip()

    def commit(self, name):
        self.git("commit", "-qm", name, "--allow-empty")
        return self.git("rev-parse", "HEAD")

    def batches(self, event):
        event_file = self.repo / "event.json"
        event_file.write_text(json.dumps(event), encoding="utf-8")
        output = subprocess.check_output(
            [sys.executable, "-I", SCRIPT],
            cwd=self.repo,
            env={**os.environ, "GITHUB_EVENT_PATH": str(event_file)},
            text=True,
        )
        return json.loads(output.removeprefix("batches="))

    def test_a_batch_push_checks_each_commit_other_than_the_tip(self):
        before = self.commit("before")
        first = self.commit("first")
        second = self.commit("second")
        tip = self.commit("tip")
        self.assertEqual(self.batches({"before": before, "after": tip}), [{"commits": [first, second]}])

    def test_a_single_commit_push_has_no_earlier_checks(self):
        before = self.commit("before")
        tip = self.commit("tip")
        self.assertEqual(self.batches({"before": before, "after": tip}), [])

    def test_a_new_branch_uses_the_push_payload(self):
        earlier = self.commit("earlier")
        tip = self.commit("tip")
        event = {"before": "0" * 40, "after": tip, "commits": [{"id": earlier}, {"id": tip}]}
        self.assertEqual(self.batches(event), [{"commits": [earlier]}])


if __name__ == "__main__":
    unittest.main()
