"""GitHub sync: import issues/PRs, mirror claims, surface PR conflicts.

Network-free: every function takes already-parsed JSON (the output of
``gh issue list --json ...`` / ``gh pr list --json ...`` parsed with
``json.loads``) and returns plain data. No subprocess, no network.
The CLI / server remain responsible for actually calling ``gh``.

Status shape compatibility: :func:`import_issues` returns a dict keyed
by issue number whose values have exactly the keys ``kernel.project``
produces (``issue``, ``owner``, ``files``, ``expires``,
``last_heartbeat``, ``state``, ``plan``, ``handoffs``), plus GitHub
extras (``title``, ``url``, ``labels``). It can therefore be merged
into / rendered like the kernel projection.
"""
import itertools
import json

__all__ = [
    "parse_issues", "parse_issue_list",
    "parse_prs", "parse_pull_requests", "parse_pr_list",
    "import_issues", "import_open_issues", "issues_to_status",
    "status_from_issues", "to_status",
    "mirror_body", "mirror_comment", "comment_body",
    "build_comment", "format_comment", "mirror_message",
    "pr_conflicts", "detect_pr_conflicts", "find_conflicts",
    "file_conflicts", "detect_conflicts", "pr_file_conflicts",
    "sync",
]

_TYPES = ("claim", "plan", "touch", "heartbeat", "handoff", "release")


# ---------------------------------------------------------------- helpers
def _as_list(payload):
    """Accept list / single dict / wrapper dict / JSON string -> list."""
    if payload is None:
        return []
    if isinstance(payload, str):
        payload = json.loads(payload) if payload.strip() else []
    if isinstance(payload, dict):
        for key in ("issues", "pullRequests", "pull_requests", "prs",
                    "items", "data"):
            if key in payload and isinstance(payload[key], list):
                return payload[key]
        return [payload]
    return list(payload)


def _is_open(entry, default=True):
    state = entry.get("state") or entry.get("status") or ""
    if not state:
        # gh pr list also has "isDraft"; absence of state means open
        # when the caller already filtered with `gh ... list`.
        if entry.get("closed") is True or entry.get("merged") is True:
            return False
        return default
    return str(state).upper() in ("OPEN", "OPENED")


def _label_names(entry):
    out = []
    for lab in entry.get("labels") or []:
        if isinstance(lab, dict):
            if lab.get("name"):
                out.append(lab["name"])
        elif lab:
            out.append(str(lab))
    return out


def _assignee_logins(entry):
    out = []
    for a in entry.get("assignees") or []:
        if isinstance(a, dict):
            if a.get("login") or a.get("name"):
                out.append(a.get("login") or a.get("name"))
        elif a:
            out.append(str(a))
    author = entry.get("author")
    # author is not an assignee; handled by callers that want it
    return out


def _file_paths(entry):
    """PR `files`: list of {path:...} / strings / single string -> [str]."""
    raw = entry.get("files")
    if raw is None:
        raw = entry.get("changedFiles") or entry.get("changed_files") or []
    if isinstance(raw, str):
        return [raw] if raw else []
    out = []
    for f in raw or []:
        if isinstance(f, dict):
            p = f.get("path") or f.get("filename") or f.get("name")
            if p:
                out.append(p)
        elif f:
            out.append(str(f))
    return out


# ---------------------------------------------------------------- parsing
def parse_issues(payload):
    """Normalize `gh issue list --json` payload -> list of plain dicts.

    Each item: {number, title, state, labels, assignees, url}.
    Closed issues are kept here (state preserved); filtering to open
    happens in :func:`import_issues`.
    """
    out = []
    for e in _as_list(payload):
        if not isinstance(e, dict):
            continue
        number = e.get("number", e.get("issue"))
        if number is None:
            continue
        try:
            number = int(number)
        except (TypeError, ValueError):
            continue
        state = str(e.get("state") or ("OPEN" if _is_open(e) else "CLOSED"))
        out.append({
            "number": number,
            "title": e.get("title", "") or "",
            "state": state.upper(),
            "labels": _label_names(e),
            "assignees": _assignee_logins(e),
            "url": e.get("url", "") or "",
        })
    return out


parse_issue_list = parse_issues


def parse_prs(payload):
    """Normalize `gh pr list --json` payload -> list of plain dicts.

    Handles gh's file shape ``{"path": ..., "additions": ...}`` as
    well as plain string paths. Each item:
    {number, title, state, branch, files, author, url, is_draft}.
    Also sets ``headRefName`` alias via ``branch`` only.
    """
    out = []
    for e in _as_list(payload):
        if not isinstance(e, dict):
            continue
        number = e.get("number", e.get("pr", e.get("pull_request")))
        if number is None:
            continue
        try:
            number = int(number)
        except (TypeError, ValueError):
            continue
        state = str(e.get("state") or ("OPEN" if _is_open(e) else "CLOSED"))
        author = e.get("author")
        if isinstance(author, dict):
            author = author.get("login") or author.get("name") or ""
        out.append({
            "number": number,
            "title": e.get("title", "") or "",
            "state": state.upper(),
            "branch": e.get("headRefName") or e.get("branch") or e.get("head_ref") or "",
            "files": _file_paths(e),
            "author": author or "",
            "url": e.get("url", "") or "",
            "is_draft": bool(e.get("isDraft", e.get("is_draft", False))),
        })
    return out


parse_pull_requests = parse_prs
parse_pr_list = parse_prs


# ---------------------------------------------------------------- import
def import_issues(payload):
    """Import open issues into a kernel.project-shaped status dict.

    Returns ``{issue_number: item}`` where item has the kernel keys
    ``issue, owner, files, expires, last_heartbeat, state, plan,
    handoffs`` plus ``title, url, labels``. Closed issues are skipped.
    Owner is the first assignee login, else None. State is "open".
    """
    # Accept already-normalized entries as well as raw gh payloads:
    # detect normalized by presence of int 'number' + 'labels' list of str
    # parse_issues is idempotent for both, so just run it.
    items = {}
    for iss in parse_issues(payload):
        if iss["state"] != "OPEN":
            continue
        n = iss["number"]
        items[n] = {
            "issue": n,
            "owner": iss["assignees"][0] if iss["assignees"] else None,
            "files": [],
            "expires": 0,
            "last_heartbeat": 0,
            "state": "open",
            "plan": None,
            "handoffs": [],
            "title": iss["title"],
            "url": iss["url"],
            "labels": list(iss["labels"]),
        }
    return items


import_open_issues = import_issues
issues_to_status = import_issues
status_from_issues = import_issues
to_status = import_issues


# ---------------------------------------------------------------- mirror
def mirror_body(*args, **kwargs):
    """Build the exact comment body the CLI posts to GitHub.

    The CLI (cli.py) posts::

        f"[magnus-teams] {actor} {msg}: {text} {files}"

    where ``files`` is a Python list (rendered as ``['a.py']``).
    This pure function reproduces that string exactly.

    Accepted call forms:
      mirror_body(actor, msg, text="", files=None)
      mirror_body(msg, actor, text="", files=None)   # swapped, detected
      mirror_body(event_dict)  # {"actor":..,"type"/"msg":..,"text":..,"files":..}
      keyword: actor=, msg=/type_/message_type=, text=, files=
    """
    actor = kwargs.pop("actor", None)
    msg = (kwargs.pop("msg", None) or kwargs.pop("type", None)
           or kwargs.pop("type_", None) or kwargs.pop("message_type", None)
           or kwargs.pop("kind", None))
    text = kwargs.pop("text", None)
    files = kwargs.pop("files", None)
    if kwargs:
        # tolerate stray kwargs like issue= by ignoring them
        pass

    positional = list(args)
    # event dict form
    if positional and isinstance(positional[0], dict):
        ev = positional[0]
        actor = actor if actor is not None else ev.get("actor", "unknown")
        msg = msg or ev.get("type") or ev.get("msg") or ""
        if text is None:
            text = ev.get("text", "")
        if files is None:
            files = ev.get("files", [])
        positional = positional[1:]

    # positional fill: could be (actor, msg, ...) or (msg, actor, ...)
    if positional:
        first = positional.pop(0)
        if actor is None and msg is None:
            # need two values; decide order by membership in _TYPES
            second = positional.pop(0) if positional else ""
            if str(first) in _TYPES and str(second) not in _TYPES:
                msg, actor = first, second
            else:
                actor, msg = first, second
        elif actor is None:
            actor = first
        elif msg is None:
            msg = first
    if positional and text is None:
        text = positional.pop(0)
    if positional and files is None:
        files = positional.pop(0)

    if actor is None:
        actor = "unknown"
    if msg is None:
        msg = ""
    if text is None:
        text = ""
    if files is None:
        files_list = []
    elif isinstance(files, str):
        files_list = [f for f in files.split(",") if f]
    else:
        files_list = list(files)
    return f"[magnus-teams] {actor} {msg}: {text} {files_list}"


mirror_comment = mirror_body
comment_body = mirror_body
build_comment = mirror_body
format_comment = mirror_body
mirror_message = mirror_body


# ---------------------------------------------------------------- conflicts
def pr_conflicts(prs):
    """Return file conflicts between open PRs.

    Accepts raw `gh pr list --json` payload (files as {path} dicts or
    strings) or already-normalized PRs. Only open PRs are considered.
    For each file touched by two different open PRs, one entry per
    unordered PR pair:

        {"file": path, "files": [path],
         "prs": [n1, n2], "pull_requests": [n1, n2],
         "numbers": [n1, n2]}

    Sorted by file, then PR numbers, for determinism. A PR never
    conflicts with itself; duplicate paths within one PR are ignored.
    """
    normalized = parse_prs(prs)
    open_prs = [p for p in normalized if p["state"] == "OPEN"]
    # file -> sorted unique PR numbers touching it
    by_file = {}
    for pr in open_prs:
        for path in dict.fromkeys(pr["files"]):  # dedupe, keep order
            by_file.setdefault(path, [])
            if pr["number"] not in by_file[path]:
                by_file[path].append(pr["number"])
    conflicts = []
    for path in sorted(by_file):
        numbers = sorted(by_file[path])
        if len(numbers) < 2:
            continue
        for a, b in itertools.combinations(numbers, 2):
            pair = [a, b]
            conflicts.append({
                "file": path,
                "files": [path],
                "prs": pair,
                "pull_requests": list(pair),
                "numbers": list(pair),
            })
    conflicts.sort(key=lambda c: (c["file"], c["prs"]))
    return conflicts


detect_pr_conflicts = pr_conflicts
pr_file_conflicts = pr_conflicts
file_conflicts = pr_conflicts
detect_conflicts = pr_conflicts
find_conflicts = pr_conflicts


# ---------------------------------------------------------------- combined
def sync(issues_payload=None, prs_payload=None):
    """Combined import: {"items": status, "pull_requests": [...],
    "conflicts": [...]}. Network-free."""
    return {
        "items": import_issues(issues_payload) if issues_payload is not None else {},
        "pull_requests": parse_prs(prs_payload) if prs_payload is not None else [],
        "conflicts": pr_conflicts(prs_payload) if prs_payload is not None else [],
    }
