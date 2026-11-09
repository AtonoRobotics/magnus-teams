"""Tests for check_tools.py: which check commands are appliance-verified,
which self-checked, and which refused. Run with
`python3 -I sandbox/test_check_tools.py`. Program and module lookups use the
machine running the tests, so the cases use only what the image and a
development machine both have.
"""
import importlib.util
import os
import unittest

spec = importlib.util.spec_from_file_location("check_tools", os.path.join(os.path.dirname(os.path.abspath(__file__)), "check_tools.py"))
ct = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ct)


def label(cmd):
    f = ct.check(cmd)
    if f.problems:
        return "refused"
    return "self-checked" if f.runs else "appliance-verified"


APPLIANCE_VERIFIED = [
    "grep -q ok /workspace/out.txt",
    # Expansion syntax the shell never expands: single-quoted, escaped, or in a comment.
    "printf '%s\\n' '${PATH:=/workspace/bin}'",
    "echo \\${PATH:=/workspace/bin}",
    "grep -q ok /workspace/out.txt # not ${PATH:=/workspace/bin}",
    "echo '$((PATH=1))'",
    "test -f /workspace/a && echo yes",
    "wc -l < /workspace/f.txt",
    "head -1 /workspace/f | grep -q x",
    "find /workspace -name '*.py'",
    "cat $(ls /workspace)",
    "awk '{print $1}' /workspace/f.txt",
    "awk -F, '{ s += $2 } END { print s }' /workspace/f.csv",
    "sed -n '1p' /workspace/f",
    "sed -n '/done/p' /workspace/f",
    "sed 's/a/b/g' /workspace/f",
    "sed -e 's/sh run.sh/x/e' --sandbox /workspace/f",
    "python3 -m json.tool /workspace/r.json",
    "python3 -c \"import json; print(json.load(open('/workspace/r.json'))['ok'])\"",
    "python3 -c 'import os; print(os.path.exists(\"/workspace/x\"), len(os.listdir(\"/workspace\")))'",
    "python3 -c 'import os.path; print(os.path.getsize(\"/workspace/x\"))'",
    "python3 -c 'from xml.etree import ElementTree as E; print(E.parse(\"/workspace/a.xml\").getroot().tag)'",
    "python3 -c 'import sys, csv; print(sum(1 for _ in csv.reader(open(sys.argv[1]))))' /workspace/f.csv",
    # Shell syntax and options read as the shell and the tool read them.
    "cat /workspace/a#b",
    "grep -q ok /workspace/out.txt # look for ok",
    "echo \\' && grep -q ok /workspace/out.txt",
    "timeout 10 grep -q ok /workspace/out.txt",
    "timeout -s KILL 10 grep -q ok /workspace/out.txt",
    "env LC_ALL=C sort /workspace/f",
    "nice -n 5 wc -l /workspace/f",
    "command -v grep",
    "python3 -X utf8 -m json.tool /workspace/r.json",
    "python3 -Wignore -m json.tool /workspace/r.json",
    "sed -n -e '1p' /workspace/f",
    "sed -ne '/done/p' /workspace/f",
    "awk -F: '{print $1}' /workspace/f",
    "jq -r .ok /workspace/r.json",
]

SELF_CHECKED = [
    # The inner shell drops the backslash of "\$": the substitution runs.
    "bash -c \"cat \\$(sh run.sh)\"",
    # Arithmetic reads a quoted value as arithmetic, too.
    "x='$(sh run.sh)'; : $((x)); cat /workspace/f",
    # Workspace code by name.
    "sh run.sh",
    "bash -e run.sh",
    "python3 tests.py",
    "python3 -m unittest",
    "python3 -m pytest -q",
    # Interpreters reading their code from stdin.
    "sh < run.sh",
    "sh -s < run.sh",
    "cat run.sh | bash",
    "python3 < tests.py",
    "python3 - < tests.py",
    "python3 <<< \"$(cat tests.py)\"",
    # python3 -c code that can run other code.
    "python3 -c \"from os import system; system('sh run.sh')\"",
    "python3 -c \"import os; getattr(os, 'system')('sh run.sh')\"",
    "python3 -c \"import os; os.posix_spawn('/bin/sh', ['sh', 'run.sh'], {})\"",
    "python3 -c \"import os as o; o.system('sh run.sh')\"",
    "python3 -c \"import os; f = os; f.system('sh run.sh')\"",
    "python3 -c \"import sys; sys.modules['os'].system('sh run.sh')\"",
    "python3 -c \"import unittest; unittest.main(module=None, argv=['x', 'discover'])\"",
    "python3 -c \"x = __import__('os')\"",
    "python3 -c \"print(().__class__.__bases__)\"",
    "python3 -c \"import pathlib; print('{0.__globals__}'.format(pathlib.Path.cwd))\"",
    "python3 -c \"import typing\"",
    "python3 -c \"import subprocess\"",
    "python3 -c \"import checkrunner\"",
    "X='import os'; python3 -c \"$X\"",
    "python3 -c \"$(cat code.py)\"",
    "python3 -W ignore::checkrunner.Warn -c 'print(1)'",
    # Shells and text programs given code built when the check runs.
    "sh -c \"$(cat cmd.txt)\"",
    "bash -ec 'sh run.sh'",
    "awk 'BEGIN { system(\"sh run.sh\") }'",
    "awk 'BEGIN { \"sh run.sh\" | getline x; print x }'",
    "awk -f prog.awk /workspace/f",
    "X='system(\"sh run.sh\")'; awk \"BEGIN { $X }\"",
    "sed -n '1e sh run.sh' /workspace/f",
    "sed 's/x/sh run.sh/e' /workspace/f",
    "sed -f prog.sed /workspace/f",
    "jq -f prog.jq /workspace/r.json",
    # Programs that run others, or are not known only to read.
    "find /workspace -name '*.sh' -exec sh {} \\;",
    "sort --compress-program=sh /workspace/f",
    "xargs sh < /workspace/list",
    "git log",
    "make check",
    # Review 2026-11-04: each of these was labelled appliance-verified.
    # `#` inside a word does not start a comment.
    "cat x#; python3 /workspace/t.py",
    # Process substitution runs its command.
    "bash -c 'cat <(python3 /workspace/t.py)'",
    "diff <(python3 /workspace/t.py) /workspace/expected.txt",
    "tee >(sh run.sh) < /workspace/f",
    # An escaped quote does not open a quoted string.
    "echo \\' $(python3 /workspace/t.py) \\'",
    # A wrapper's option arguments are not the program it runs.
    "env -u cat python3 /workspace/t.py",
    "env '-Spython3 /workspace/t.py'",
    "env --split-string='python3 /workspace/t.py'",
    "xvfb-run -e cat python3 /workspace/t.py",
    # A wrapper named by its path is still a wrapper.
    "/usr/bin/env python3 /workspace/t.py",
    "/usr/bin/env -S 'python3 /workspace/t.py'",
    # Attached option values and abbreviated long options.
    "python3 -Wignore::checkrunner.Warn -c 'print(1)'",
    "sort -S 1K --com=sh /workspace/f",
    "sed --fi=prog.sed /workspace/f",
    "sed -n --expr='1e sh run.sh' p",
    "sed -e'1e sh run.sh' /workspace/f",
    "awk -bf prog.awk /workspace/f",
    "jq -nf prog.jq",
    "jq . -f prog.jq /workspace/r.json",
]

REFUSED = [
    "$(cat cmd.txt)",
    "X=sh; $X run.sh",
    "jest --ci",
    "./checkrunner.py",
    "PYTHONPATH=/workspace python3 -m pytest -q",
    "source env.sh",
    "python3 -E tests.py",
    # Review 2026-11-04: each of these was labelled appliance-verified.
    # Switching off the check environment, by any route.
    "unset PYTHONSAFEPATH MAGNUS_CHECK; python3 -m json.tool /workspace/r.json",
    "env -i python3 -c 'import json'",
    "env --ignore-env python3 -c 'import json'",
    "env -u PYTHONSAFEPATH python3 -m json.tool /workspace/r.json",
    "exec -c python3 -m json.tool /workspace/r.json",
    "for PATH in /workspace/bin; do cat /workspace/f; done",
    "read PATH < /workspace/p; cat /workspace/f",
    "printf -v PATH /workspace/bin; cat /workspace/f",
    ": $((PATH=0)); cat /workspace/f",
    "set -a; : ${PYTHONPATH:=/workspace}; python3 -m json.tool /workspace/r.json",
    # bash evaluates a variable's value as arithmetic, assignments included.
    "bash -c \"x='PATH=5'; : \\$((x)); cat f\"",
    "x='PATH=5'; : $((x)); cat /workspace/f",
    # Double quotes expand, and a single quote inside them is literal.
    "echo \"${PATH:=/workspace/bin}\"; cat /workspace/f",
    "echo \"'${PATH:=/workspace/bin}'\"; cat /workspace/f",
    "PS4='$(sh run.sh)' bash -xc true",
    # Moving where Python finds its modules' bytecode.
    "python3 -X pycache_prefix=/workspace/pfx -m json.tool /workspace/r.json",
    "python3 -Xpycache_prefix=/workspace/pfx -m json.tool /workspace/r.json",
    # A relative program path is resolved from a directory the check chooses.
    "cd /workspace/a && ../usr/bin/cat t.py",
    # A `#` inside `${...}` does not start a comment (review of #144).
    ": ${x- # $((PATH=0))}; cat /workspace/f",
    # ANSI-C quoting, which the check cannot follow.
    "echo $'\\'' $(python3 /workspace/t.py) ''",
]


class Labels(unittest.TestCase):
    def test_appliance_verified(self):
        for cmd in APPLIANCE_VERIFIED:
            with self.subTest(cmd=cmd):
                f = ct.check(cmd)
                self.assertEqual(label(cmd), "appliance-verified", f"problems={f.problems} runs={f.runs}")

    def test_self_checked(self):
        for cmd in SELF_CHECKED:
            with self.subTest(cmd=cmd):
                f = ct.check(cmd)
                self.assertEqual(label(cmd), "self-checked", f"problems={f.problems} runs={f.runs}")

    def test_refused(self):
        for cmd in REFUSED:
            with self.subTest(cmd=cmd):
                self.assertEqual(label(cmd), "refused", ct.check(cmd).runs)

    def test_reasons_show_the_shell_syntax(self):
        f = ct.check("python3 -c \"$(cat code.py)\"")
        self.assertTrue(f.runs and all(ct.EXPANDED not in ct.shown(r) for r in f.runs))
        self.assertIn("$(...)", ct.shown(ct.check("$(cat cmd.txt)").problems[0]))


if __name__ == "__main__":
    unittest.main()
