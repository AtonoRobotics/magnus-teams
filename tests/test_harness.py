import os, subprocess, unittest
ROOT = os.path.join(os.path.dirname(__file__), "..")
class Harness(unittest.TestCase):
    def test_script_exists_and_executable(self):
        p = os.path.join(ROOT, "scripts", "harness.sh")
        self.assertTrue(os.path.exists(p)); self.assertTrue(os.access(p, os.X_OK))
    def test_docs_describe_usage(self):
        s = open(os.path.join(ROOT, "docs", "harness.md")).read()
        self.assertIn("harness.sh", s); self.assertIn("claim", s.lower())
if __name__ == "__main__": unittest.main()
