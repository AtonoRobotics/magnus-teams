"""List the commits a push brought in besides its tip, for fast-earlier.yml.

GitHub starts one workflow run per push, for the push's tip. The commits
before the tip in the same push get no run of their own; fast-earlier.yml
checks each of them, and this lists them (oldest first) as a JSON array in
`commits=[...]`, for the job's output. A push of one commit lists none.
"""
import json
import os
import subprocess

# One matrix holds at most 256 jobs. A bigger push fails the planning job
# instead of leaving commits unchecked without saying so.
MATRIX_LIMIT = 256
EMPTY_SHA = "0" * 40


def git(*args, **kwargs):
    return subprocess.run(["git", *args], text=True, **kwargs)


def earlier_commits(event):
    before, after = event["before"], event["after"]
    if before == EMPTY_SHA:
        # A new branch: GitHub names the commits that came with it.
        commits = [commit["id"] for commit in event.get("commits", [])]
    else:
        # A force push's previous tip may be absent from the checkout.
        if git("cat-file", "-e", f"{before}^{{commit}}", stderr=subprocess.DEVNULL).returncode:
            git("fetch", "--no-tags", "origin", before, check=True, stdout=subprocess.DEVNULL)
        commits = git("rev-list", "--reverse", f"{before}..{after}", check=True, capture_output=True).stdout.splitlines()
    return [commit for commit in commits if commit != after]


def main():
    with open(os.environ["GITHUB_EVENT_PATH"], encoding="utf-8") as file:
        commits = earlier_commits(json.load(file))
    if len(commits) > MATRIX_LIMIT:
        raise SystemExit(f"{len(commits)} earlier commits in one push is more than a matrix holds ({MATRIX_LIMIT}); push them in smaller parts.")
    print(f"commits={json.dumps(commits, separators=(',', ':'))}")


if __name__ == "__main__":
    main()
