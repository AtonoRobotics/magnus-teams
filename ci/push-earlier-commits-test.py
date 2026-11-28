import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("push-earlier-commits.py").resolve()


class PushEarlierCommitsTest(unittest.TestCase):
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

    def run_script(self, event):
        event_file = self.repo / "event.json"
        event_file.write_text(json.dumps(event), encoding="utf-8")
        return subprocess.run(
            [sys.executable, "-I", SCRIPT],
            cwd=self.repo,
            env={**os.environ, "GITHUB_EVENT_PATH": str(event_file)},
            text=True,
            capture_output=True,
        )

    def commits(self, event):
        result = self.run_script(event)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout.removeprefix("commits="))

    def test_a_push_lists_each_commit_but_its_tip_oldest_first(self):
        before = self.commit("before")
        first = self.commit("first")
        second = self.commit("second")
        tip = self.commit("tip")
        self.assertEqual(self.commits({"before": before, "after": tip}), [first, second])

    def test_a_push_of_one_commit_lists_none(self):
        before = self.commit("before")
        tip = self.commit("tip")
        self.assertEqual(self.commits({"before": before, "after": tip}), [])

    def test_a_new_branch_uses_the_commits_in_the_push_payload(self):
        earlier = self.commit("earlier")
        tip = self.commit("tip")
        event = {"before": "0" * 40, "after": tip, "commits": [{"id": earlier}, {"id": tip}]}
        self.assertEqual(self.commits(event), [earlier])

    def test_a_push_too_big_for_a_matrix_fails_loudly(self):
        before = self.commit("before")
        for n in range(257):
            self.commit(f"c{n}")
        tip = self.commit("tip")
        result = self.run_script({"before": before, "after": tip})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("smaller parts", result.stderr)


if __name__ == "__main__":
    unittest.main()
