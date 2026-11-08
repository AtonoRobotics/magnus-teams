import json, os, sys, unittest
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from magnus_teams.github_sync import (
    parse_issues, parse_prs, import_issues, mirror_body, mirror_comment,
    pr_conflicts, sync,
)
from magnus_teams.kernel import project

# Canned `gh issue list --json number,title,state,labels,assignees,url`
ISSUES_JSON = json.loads("""[
  {"number": 2, "title": "GitHub sync", "state": "OPEN",
   "labels": [{"name": "queue"}], "assignees": [{"login": "codex"}],
   "url": "https://example/2"},
  {"number": 3, "title": "Status page", "state": "OPEN",
   "labels": [], "assignees": [], "url": "https://example/3"},
  {"number": 4, "title": "Done thing", "state": "CLOSED",
   "labels": [], "assignees": [{"login": "claude"}], "url": "https://example/4"}
]""")

# Canned `gh pr list --json number,title,state,headRefName,files,author,url,isDraft`
PRS_JSON = json.loads("""[
  {"number": 10, "title": "A", "state": "OPEN", "headRefName": "feat/a",
   "files": [{"path": "magnus_teams/kernel.py", "additions": 3},
             {"path": "README.md", "additions": 1}],
   "author": {"login": "codex"}, "url": "https://example/pr/10", "isDraft": false},
  {"number": 11, "title": "B", "state": "OPEN", "headRefName": "feat/b",
   "files": [{"path": "magnus_teams/kernel.py", "additions": 5},
             {"path": "docs/product.md", "additions": 2}],
   "author": {"login": "claude"}, "url": "https://example/pr/11", "isDraft": true},
  {"number": 12, "title": "C", "state": "CLOSED", "headRefName": "feat/c",
   "files": [{"path": "README.md", "additions": 1}],
   "author": {"login": "codex"}, "url": "https://example/pr/12", "isDraft": false}
]""")


class ParseIssues(unittest.TestCase):
    def test_parse_normalizes(self):
        out = parse_issues(ISSUES_JSON)
        self.assertEqual([i["number"] for i in out], [2, 3, 4])
        self.assertEqual(out[0]["labels"], ["queue"])
        self.assertEqual(out[0]["assignees"], ["codex"])
        self.assertEqual(out[0]["state"], "OPEN")

    def test_parse_accepts_json_string_and_empty(self):
        self.assertEqual(parse_issues("[]"), [])
        self.assertEqual(parse_issues(None), [])
        self.assertEqual(len(parse_issues(json.dumps(ISSUES_JSON))), 3)

    def test_import_open_only_status_shaped(self):
        items = import_issues(ISSUES_JSON)
        self.assertEqual(sorted(items), [2, 3])  # closed #4 skipped
        it = items[2]
        # exact kernel.project keys present
        for k in ("issue", "owner", "files", "expires",
                  "last_heartbeat", "state", "plan", "handoffs"):
            self.assertIn(k, it)
        self.assertEqual(it["issue"], 2)
        self.assertEqual(it["owner"], "codex")
        self.assertEqual(it["state"], "open")
        self.assertEqual(it["files"], [])
        self.assertEqual(it["expires"], 0)
        self.assertIsNone(it["plan"])
        self.assertEqual(it["handoffs"], [])
        self.assertEqual(it["title"], "GitHub sync")
        self.assertIsNone(items[3]["owner"])

    def test_import_compatible_with_kernel_projection_shape(self):
        # kernel project() over no events vs imported: same value keys/types
        items = import_issues(ISSUES_JSON)
        for it in items.values():
            self.assertIsInstance(it["issue"], int)
            self.assertIsInstance(it["files"], list)
            self.assertIsInstance(it["handoffs"], list)
        self.assertEqual(project([], now=0), {})


class ParsePrs(unittest.TestCase):
    def test_parse_extracts_file_paths(self):
        out = parse_prs(PRS_JSON)
        self.assertEqual(out[0]["files"],
                         ["magnus_teams/kernel.py", "README.md"])
        self.assertEqual(out[0]["branch"], "feat/a")
        self.assertEqual(out[0]["author"], "codex")
        self.assertTrue(out[1]["is_draft"])

    def test_parse_plain_string_files(self):
        out = parse_prs([{"number": 1, "state": "OPEN",
                          "files": ["a.py", "b.py"]}])
        self.assertEqual(out[0]["files"], ["a.py", "b.py"])


class Mirror(unittest.TestCase):
    def test_exact_cli_body_claim(self):
        # cli.py: f"[magnus-teams] {actor} {msg}: {text} {files}"
        self.assertEqual(
            mirror_body("codex", "claim", "", ["a.py"]),
            "[magnus-teams] codex claim:  ['a.py']")
        self.assertEqual(
            mirror_comment("codex", "claim", "", ["a.py"]),
            "[magnus-teams] codex claim:  ['a.py']")

    def test_all_message_types(self):
        for msg in ("claim", "plan", "heartbeat", "handoff"):
            self.assertEqual(
                mirror_body("claude", msg, "hello", []),
                f"[magnus-teams] claude {msg}: hello []")

    def test_defaults_and_swapped_and_event_forms(self):
        self.assertEqual(mirror_body("codex", "heartbeat"),
                         "[magnus-teams] codex heartbeat:  []")
        self.assertEqual(mirror_body("claim", "codex", "", ["x.py"]),
                         "[magnus-teams] codex claim:  ['x.py']")
        self.assertEqual(
            mirror_body({"type": "handoff", "actor": "codex",
                         "text": "done", "files": []}),
            "[magnus-teams] codex handoff: done []")


class PrConflicts(unittest.TestCase):
    def test_open_overlap_only(self):
        confs = pr_conflicts(PRS_JSON)
        # kernel.py shared by open PRs 10 & 11; README only conflicts
        # with closed PR 12, so it must not appear
        self.assertEqual(len(confs), 1)
        self.assertEqual(confs[0]["file"], "magnus_teams/kernel.py")
        self.assertEqual(confs[0]["prs"], [10, 11])
        self.assertEqual(confs[0]["files"], ["magnus_teams/kernel.py"])

    def test_no_overlap_and_self_not_conflict(self):
        prs = [{"number": 1, "state": "OPEN", "files": ["a.py", "a.py"]},
               {"number": 2, "state": "OPEN", "files": ["b.py"]}]
        self.assertEqual(pr_conflicts(prs), [])
        self.assertEqual(pr_conflicts([]), [])

    def test_three_way_is_pairwise(self):
        prs = [{"number": n, "state": "OPEN", "files": ["x.py"]}
               for n in (1, 2, 3)]
        self.assertEqual([c["prs"] for c in pr_conflicts(prs)],
                         [[1, 2], [1, 3], [2, 3]])

    def test_sync_combines(self):
        out = sync(ISSUES_JSON, PRS_JSON)
        self.assertEqual(sorted(out["items"]), [2, 3])
        self.assertEqual(len(out["conflicts"]), 1)
        self.assertEqual(len(out["pull_requests"]), 3)


if __name__ == "__main__":
    unittest.main()
