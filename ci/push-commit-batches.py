"""Emit the earlier commits in a branch push for the GitHub Actions matrices."""
import json
import os
import subprocess

MATRIX_LIMIT = 256
EMPTY_SHA = "0" * 40


def git(*args, **kwargs):
    return subprocess.run(["git", *args], text=True, **kwargs)


def earlier_commits(event):
    before, after = event["before"], event["after"]
    if before == EMPTY_SHA:
        # GitHub provides the commits that came with the creation of a branch.
        commits = [commit["id"] for commit in event["commits"]]
    else:
        # A force push's previous tip may be absent from the full checkout.
        if git("cat-file", "-e", f"{before}^{{commit}}", stderr=subprocess.DEVNULL).returncode:
            git("fetch", "--no-tags", "origin", before, check=True, stdout=subprocess.DEVNULL)
        commits = git("rev-list", "--reverse", f"{before}..{after}", check=True, capture_output=True).stdout.splitlines()
    return [commit for commit in commits if commit != after]


def main():
    with open(os.environ["GITHUB_EVENT_PATH"], encoding="utf-8") as file:
        commits = earlier_commits(json.load(file))
    if len(commits) > MATRIX_LIMIT ** 2:
        raise SystemExit("Push exceeds the GitHub Actions matrix capacity. Split it into smaller pushes.")
    batches = [
        {"commits": commits[start:start + MATRIX_LIMIT]}
        for start in range(0, len(commits), MATRIX_LIMIT)
    ]
    print(f"batches={json.dumps(batches, separators=(',', ':'))}")


if __name__ == "__main__":
    main()
