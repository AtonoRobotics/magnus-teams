"""Resolve the tools a contract check's command runs, against this image,
and say whether its verdict comes from running workspace code.

The kernel runs this in a fresh check VM with an empty workspace when a
contract is recorded, so whatever is found here is in the image. Every
program a check runs must come from the image, and nothing may change where
programs and modules are looked up. A check that also
executes a workspace file (a script, tests, a demo, code it imports) is
self-checked: its verdict rests on code the agent wrote. One that only reads
the work with the image's tools is appliance-verified. When in doubt, it is
self-checked: a check is appliance-verified only when every program it runs
is one known to read what it is given (READERS), and every program text it
hands an interpreter (python3 -c, sh -c, awk, sed, jq) is fixed when the
contract is recorded and uses only what is known not to run other code.

usage: python3 -I check_tools.py BASE64_JSON_LIST_OF_COMMANDS
Prints a JSON list holding, for each command,
{"problems": [...], "self_checked": [reasons it runs workspace code]}.
"""
import ast
import base64
import importlib.util
import json
import os
import re
import shlex
import shutil
import sys

PATH = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
# Variables that change where programs or modules come from, or switch off
# the check environment.
SEARCH = {
    "PATH", "HOME", "IFS", "CDPATH", "ENV", "BASH_ENV", "SHELLOPTS", "BASHOPTS", "MAGNUS_CHECK",
    "LD_PRELOAD", "LD_LIBRARY_PATH", "LD_AUDIT", "GCONV_PATH", "NODE_PATH", "NODE_OPTIONS",
    "PERL5LIB", "PERL5OPT", "RUBYLIB", "RUBYOPT",
    # Expanded by the shell, command substitutions included, as it traces
    # (-x) or before a prompt.
    "PS4", "PROMPT_COMMAND",
}
BUILTINS = {"cd", "test", "[", "echo", "printf", "true", "false", "exit", ":", "set", "unset", "shift", "wait", "read", "pwd", "umask", "export", "return"}
REFUSED = {
    ".": "sources a file into the checking shell",
    "source": "sources a file into the checking shell",
    "eval": "builds a command the check cannot be resolved from",
    "alias": "changes what a name runs",
    "hash": "changes what a name runs",
    "case": "is not supported in a check; use test or a python3 -c snippet",
}
KEYWORDS = {"if", "then", "else", "elif", "fi", "do", "done", "while", "until", "!", "{", "}"}
# Wrappers that run the program named after their own options, with those
# options: short ones that take no value, short ones that take one (attached
# or the next word), and the long ones of each kind. Any other option leaves
# what the wrapper runs unknown, so the check is self-checked.
WRAPPERS = {
    "env": ("0v", "uCS", {"--null", "--debug", "--ignore-environment", "--ignore-signal", "--default-signal", "--block-signal", "--list-signal-handling"}, {"--unset", "--chdir", "--split-string"}),
    "timeout": ("v", "sk", {"--preserve-status", "--foreground", "--verbose"}, {"--signal", "--kill-after"}),
    "nice": ("", "n", set(), {"--adjustment"}),
    "nohup": ("", "", set(), set()),
    "stdbuf": ("", "ioe", set(), {"--input", "--output", "--error"}),
    "ionice": ("t", "cnp", {"--ignore"}, {"--class", "--classdata", "--pid"}),
    "time": ("pav", "of", {"--portability", "--append", "--verbose", "--quiet"}, {"--output", "--format"}),
    "command": ("pvV", "", set(), set()),
    "exec": ("l", "a", set(), set()),
    "xvfb-run": ("al", "efnpsw", {"--auto-servernum", "--listen-tcp"}, {"--error-file", "--auth-file", "--server-num", "--xauth-protocol", "--server-args", "--wait"}),
}
# The short option with which a wrapper starts its program with an empty
# environment, which drops the check environment (PYTHONSAFEPATH,
# MAGNUS_CHECK, PATH).
CLEARS = {"env": "i", "exec": "c"}
SEPARATORS = {";", "&&", "||", "|", "&", "(", ")", "\n", ";;", "|&"}
REDIRECTS = {"<", ">", ">>", "<<", "<<<", ">&", "<&", ">|", "&>", "&>>", "<>"}
PYTHON = re.compile(r"^python(\d+(\.\d+)?)?$")
SHELLS = {"sh", "bash", "dash"}
# Image programs that only read what they are given and run nothing else.
# Any other program a check runs makes it self-checked. Interpreters and the
# programs with a way to run commands are looked at below.
READERS = {
    "cat", "head", "tail", "wc", "grep", "egrep", "fgrep", "diff", "cmp", "comm", "ls", "stat", "file",
    "sha1sum", "sha256sum", "sha512sum", "md5sum", "cksum", "uniq", "cut", "tr", "paste", "join", "fold",
    "nl", "od", "rev", "tac", "strings", "iconv", "basename", "dirname", "realpath", "readlink", "du", "df",
    "date", "seq", "expr", "tee", "true", "false", "echo", "printf", "test", "[", "pwd", "id", "uname",
    "pdftotext", "pdfinfo", "pdffonts", "pdftoppm", "pdfseparate", "qpdf", "unzip", "zipinfo", "zcat",
    "gzip", "gunzip", "xz", "unxz", "xzcat", "bzip2", "bunzip2", "bzcat", "tesseract", "sleep",
}
# Modules that only read data, never run code from a file (python3 -m).
DATA_MODULES = {"json.tool", "base64", "zipfile", "tarfile", "gzip", "py_compile", "tokenize", "ast", "filecmp", "sqlite3"}
# Modules python3 -c code may import and stay appliance-verified: none of
# them runs code from a file, imports a module named in data, or evaluates
# a string. os and sys only through the attributes below.
SAFE_MODULES = {
    "json", "re", "math", "statistics", "string", "textwrap", "difflib", "csv", "hashlib", "base64", "binascii",
    "zipfile", "gzip", "bz2", "lzma", "zlib", "tarfile", "datetime", "time", "collections", "itertools",
    "functools", "decimal", "fractions", "unicodedata", "struct", "glob", "fnmatch", "filecmp", "html",
    "html.parser", "xml", "xml.etree", "xml.etree.ElementTree", "xml.dom", "xml.dom.minidom", "configparser",
    "tomllib", "io", "codecs", "pathlib", "os", "os.path", "sys", "email", "mimetypes", "pprint", "shlex",
    "random", "uuid", "ast", "tokenize", "enum", "openpyxl", "docx", "pptx", "pypdf", "pdfplumber", "fitz",
    "bs4", "lxml", "lxml.etree", "lxml.html", "tabulate",
}
OS_SAFE = {
    "path", "listdir", "scandir", "walk", "stat", "lstat", "getcwd", "sep", "linesep", "environ", "getenv",
    "access", "R_OK", "W_OK", "X_OK", "F_OK", "fspath", "cpu_count",
}
SYS_SAFE = {"argv", "exit", "stdin", "stdout", "stderr", "version", "version_info", "platform", "maxsize", "byteorder", "float_info", "implementation"}
RESTRICTED = {"os": OS_SAFE, "sys": SYS_SAFE}
# Builtins that run, import or reach code by name.
UNSAFE_NAMES = {
    "exec", "eval", "compile", "__import__", "getattr", "setattr", "delattr", "globals", "locals", "vars",
    "breakpoint", "help", "__builtins__", "__loader__", "__spec__",
}
# Programs whose program text decides what they do, and the options that
# load that text, or code, from a file.
AWKS = {"awk", "gawk", "mawk"}
# Marks a shell expansion outside single quotes ($NAME, ${...}, $(...),
# `...`): what it stands for is known only when the check runs.
EXPANDED = "\x01"
SUBSTITUTED = EXPANDED + "SUB"


def shown(s):
    return s.replace(SUBSTITUTED, "$(...)").replace(EXPANDED, "$")


def comment_at(cmd, i):
    """Whether the `#` at i starts a comment: the shell starts one only at
    the beginning of a word (`x#y` is one word). After an operator (`;#`)
    it does too, but reading that `#` as part of the command is the safe
    side: the text after it is still checked."""
    return cmd[i] == "#" and (i == 0 or cmd[i - 1] in " \t\r\n")


def closing(cmd, i):
    """The index just past the `)` that closes the substitution whose body
    starts at i, read as the shell reads it: parentheses in quotes, after a
    backslash or in a comment do not count."""
    depth, quote = 1, None
    while i < len(cmd):
        c = cmd[i]
        if c == "\\" and quote != "'":
            i += 2
            continue
        if quote:
            if c == quote:
                quote = None
        elif c in "'\"":
            quote = c
        elif comment_at(cmd, i):
            nl = cmd.find("\n", i)
            i = len(cmd) if nl < 0 else nl
            continue
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    raise ValueError("a substitution is not closed")


def substitutions(cmd):
    """Pull out $(...), `...` and bash's <(...) and >(...), outside single
    quotes, and drop comments; return the command with each substitution
    replaced by a marked word and every other $ outside single quotes
    marked, and the inner commands. A backslash keeps the next character
    from opening or closing anything, as in the shell."""
    out, inner, i, quote = [], [], 0, None
    while i < len(cmd):
        c = cmd[i]
        if quote == "'":
            if c == "'":
                quote = None
            out.append(c)
            i += 1
            continue
        if c == "\\":
            out.append(cmd[i : i + 2])
            i += 2
            continue
        if quote is None and comment_at(cmd, i):
            nl = cmd.find("\n", i)
            i = len(cmd) if nl < 0 else nl
            continue
        if quote is None and cmd.startswith("$'", i):
            # Its escapes (\' among them) are bash's alone: what the shell
            # reads as quoted cannot be told from what it runs.
            raise ValueError("ANSI-C quoting ($'...') is not supported in a check")
        if c == "'" and quote is None:
            quote = "'"
        elif c == '"':
            quote = None if quote == '"' else '"'
        if (cmd.startswith("$(", i) and not cmd.startswith("$((", i)) or (quote is None and cmd.startswith(("<(", ">("), i)):
            j = closing(cmd, i + 2)
            inner.append(cmd[i + 2 : j - 1])
            out.append(SUBSTITUTED)
            i = j
            continue
        if c == "`":
            j = i + 1
            while j < len(cmd) and cmd[j] != "`":
                j += 2 if cmd[j] == "\\" else 1
            if j >= len(cmd):
                raise ValueError("a `...` substitution is not closed")
            inner.append(cmd[i + 1 : j])
            out.append(SUBSTITUTED)
            i = j + 1
            continue
        out.append(EXPANDED if c == "$" else c)
        i += 1
    return "".join(out), inner


# Assignments inside an arithmetic expansion ($((...)), $[...]) or a
# parameter expansion (${NAME:=...}, and the arithmetic in ${a[i]} and
# ${x:offset}): the shell sets the variable as it expands the word.
ARITH_ASSIGN = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)\s*(?:[-+*/%&|^]|<<|>>)?=(?!=)|(?:\+\+|--)\s*([A-Za-z_][A-Za-z0-9_]*)|([A-Za-z_][A-Za-z0-9_]*)\s*(?:\+\+|--)")
DEFAULT_ASSIGN = re.compile(r"^([A-Za-z_][A-Za-z0-9_]*):?=")
# Where bash reads text as arithmetic or as a variable's name, a command
# substitution in that text runs, even one written in single quotes
# (x='a[$(cmd)]'; : $((x))).
EVALUATES = re.compile(r"\$\(\(|\$\[|\$\{!|\$\{[A-Za-z_][A-Za-z0-9_]*[\[:]|\b(?:test|\[|printf|wait)\s+-[vp]\b|\b(?:read|unset)\b")
QUOTED_SUBSTITUTION = re.compile(r"'[^']*(?:\$\(|`)[^']*'")


def expansion_bodies(cmd):
    """The text inside each $((...)), $[...] and ${...}."""
    for m in re.finditer(r"\$(\(\(|\[|\{)", cmd):
        opens, closes = {"((": ("(", ")"), "[": ("[", "]"), "{": ("{", "}")}[m.group(1)]
        depth, j = 1, m.end()
        while j < len(cmd) and depth:
            depth += {opens: 1, closes: -1}.get(cmd[j], 0)
            j += 1
        yield m.group(1), cmd[m.end() : j]


def evaluated(cmd):
    """The command with what the shell never expands blanked out: text in
    single quotes (outside double quotes), a character after a backslash,
    and comments before any `${`. The length and the rest stay as they were."""
    out, i, quote = list(cmd), 0, None
    while i < len(cmd):
        c = cmd[i]
        if quote == "'":
            if c == "'":
                quote = None
            else:
                out[i] = " "
        elif c == "\\":
            out[i : i + 2] = " " * len(cmd[i : i + 2])
            i += 2
            continue
        elif c == "'" and quote is None:
            quote = "'"
        elif c == '"':
            quote = None if quote == '"' else '"'
        # A `#` inside `${...}` is text, not a comment, and bash evaluates
        # what follows it (`${x- # $((PATH=0))}`). Rather than follow where
        # each `${` closes, no comment after one is blanked.
        elif quote is None and comment_at(cmd, i) and "${" not in cmd[:i]:
            nl = cmd.find("\n", i)
            end = len(cmd) if nl < 0 else nl
            out[i:end] = " " * (end - i)
            i = end
            continue
        i += 1
    return "".join(out)


def expansion_assignments(cmd, found):
    visible = evaluated(cmd)
    # Where bash reads text as arithmetic, a quoted value can be evaluated
    # too (x='a[$(cmd)]'; : $((x))): then the quoted text counts as well.
    arithmetic = EVALUATES.search(visible)
    scan = cmd if arithmetic else visible
    if arithmetic:
        # And bash evaluates a variable's value as arithmetic, assignments
        # included (x='PATH=5'; : $((x))), wherever the value was written.
        for name in [n for m in ARITH_ASSIGN.finditer(cmd) for n in m.groups() if n]:
            search_var(name, found)
    for kind, body in expansion_bodies(scan):
        names = [n for m in ARITH_ASSIGN.finditer(body) for n in m.groups() if n]
        if kind == "{" and DEFAULT_ASSIGN.match(body):
            names.append(DEFAULT_ASSIGN.match(body).group(1))
        for name in names:
            search_var(name, found)
    if EVALUATES.search(visible) and QUOTED_SUBSTITUTION.search(cmd):
        found.runs.append("it holds a quoted command substitution where bash evaluates text as arithmetic or as a variable's name, which runs it")


def simple_commands(cmd):
    lex = shlex.shlex(cmd, posix=True, punctuation_chars="();<>|&\n")
    lex.whitespace = " \t\r"
    lex.whitespace_split = True
    # Comments were dropped by substitutions(), where the shell drops them;
    # shlex would also start one inside a word.
    lex.commenters = ""
    words, current, skip_next = [], [], False
    for tok in lex:
        if skip_next:
            skip_next = False
            continue
        if tok in SEPARATORS:
            if current:
                words.append(current)
            current = []
        elif tok in REDIRECTS or (set(tok) <= set("<>&|") and tok):
            if current and current[-1].isdigit():
                current.pop()
            skip_next = True
        else:
            current.append(tok)
    if current:
        words.append(current)
    return words


def assignment(word):
    m = re.match(r"^([A-Za-z_][A-Za-z0-9_]*)=", word)
    return m.group(1) if m else None


def image_program(name):
    """Where `name` runs from, if the image has it."""
    if "/" in name:
        if not name.startswith("/"):
            return None
        p = os.path.realpath(name)
        if p.startswith(("/workspace", "/tmp", "/home", "/root", "/dev/shm")):
            return None
        return p if os.path.isfile(p) and os.access(p, os.X_OK) else None
    return shutil.which(name, path=PATH)


def image_module(name):
    try:
        spec = importlib.util.find_spec(name)
    except (ImportError, ValueError):
        return False
    origin = spec and (spec.origin or (spec.submodule_search_locations and list(spec.submodule_search_locations)[0]))
    return bool(spec) and (origin in ("built-in", "frozen") or os.path.realpath(str(origin)).startswith("/usr/"))


class Found:
    def __init__(self):
        self.problems = []
        self.runs = []


def check(cmd, depth=0, found=None):
    found = found or Found()
    try:
        flat, inner = substitutions(cmd)
        commands = simple_commands(flat)
    except ValueError as e:
        found.problems.append(f"the command cannot be parsed ({e})")
        return found
    expansion_assignments(cmd, found)
    for sub in inner:
        check(sub, depth + 1, found)
    for words in commands:
        check_simple(words, depth, found)
    return found


def search_var(name, found, does="sets"):
    if name in SEARCH or name.startswith("PYTHON"):
        found.problems.append(f"it {does} {name}, which changes where the check's tools come from")


def long_name(word, names):
    """The long option of `names` that `word` gives: GNU tools take any
    unambiguous abbreviation (--com for --compress-program). None when it
    abbreviates none of them, or more than one."""
    key = word.split("=", 1)[0]
    if not key.startswith("--") or len(key) < 3:
        return None
    if key in names:
        return key
    hits = [n for n in names if n.startswith(key)]
    return hits[0] if len(hits) == 1 else None


def option_value(args, j, k):
    """The value of the short option at args[j][k]: the rest of the word, or
    the next word. Returns it and how many words the option took."""
    if k + 1 < len(args[j]):
        return args[j][k + 1 :], 1
    return (args[j + 1] if j + 1 < len(args) else ""), 2


def wrapper_options(w, words, i, found):
    """Skip the wrapper's own options (and timeout's duration); return the
    index of what it runs, or None when that cannot be known."""
    flags, valued, long_flags, long_valued = WRAPPERS[w]
    clears = CLEARS.get(w)
    while i < len(words):
        a = words[i]
        if a == "--":
            i += 1
            break
        if w == "env" and (a == "-" or long_name(a, long_flags | long_valued) == "--ignore-environment"):
            found.problems.append(f"`env {a}` starts its program without the check environment")
            return None
        if a.startswith("--"):
            name = long_name(a, long_flags | long_valued)
            if name is None:
                found.runs.append(f"`{w} {a}` is an option the check cannot resolve, so what it runs is not known")
                return None
            took = 1
            if name in long_valued:
                value, took = (a.split("=", 1)[1], 1) if "=" in a else ((words[i + 1] if i + 1 < len(words) else ""), 2)
                if not wrapper_value(w, name, value, found):
                    return None
            i += took
            continue
        if a.startswith("-") and len(a) > 1:
            took = 1
            for k in range(1, len(a)):
                o = a[k]
                if o == clears:
                    found.problems.append(f"`{w} -{o}` starts its program without the check environment")
                    return None
                if o in valued:
                    value, took = option_value(words, i, k)
                    if not wrapper_value(w, "-" + o, value, found):
                        return None
                    break
                if o not in flags:
                    found.runs.append(f"`{w} {a}` is an option the check cannot resolve, so what it runs is not known")
                    return None
            i += took
            continue
        break
    if w == "timeout" and i < len(words):
        i += 1
    return i


def wrapper_value(w, option, value, found):
    """Look at the value a wrapper's option takes; False when what the
    wrapper runs can no longer be known."""
    if w == "env" and option in ("-u", "--unset"):
        search_var(value, found, "unsets")
    elif w == "env" and option in ("-S", "--split-string"):
        found.runs.append(f"`env {option}` builds the command it runs from a string")
        return False
    return True


def built(text, what, found):
    """Program text the shell expands is known only when the check runs."""
    if EXPANDED in text:
        found.runs.append(f"its {what} is built by the shell when the check runs, so what it runs is not known in advance")
        return True
    return False


def check_simple(words, depth, found):
    i = 0
    while i < len(words) and words[i] in KEYWORDS:
        i += 1
    if i < len(words) and words[i] == "for":
        # The loop sets its variable to each value in turn.
        if i + 1 < len(words):
            search_var(words[i + 1], found)
        return
    # Leading assignments, then wrappers with their options and assignments.
    while i < len(words):
        w = words[i]
        name = assignment(w)
        if name:
            search_var(name, found)
            i += 1
            continue
        # A wrapper named by its path in the image is the same wrapper.
        wb = os.path.basename(w)
        if wb in WRAPPERS and (w == wb or image_program(w)):
            if not image_program(w) and wb not in ("command", "exec", "time"):
                found.problems.append(f"`{w}` is not in the sandbox image")
            i = wrapper_options(wb, words, i + 1, found)
            if i is None:
                return
            continue
        break
    if i >= len(words):
        return
    prog, args = words[i], words[i + 1 :]
    if EXPANDED in prog:
        found.problems.append(f"`{shown(prog)}` names the program it runs only when the check runs; name an image program")
        return
    if prog in REFUSED:
        found.problems.append(f"`{prog}` {REFUSED[prog]}")
        return
    if prog == "export":
        for a in args:
            search_var(assignment(a) or a, found)
        return
    if prog in BUILTINS:
        # Builtins that set or unset the variables they name.
        if prog in ("unset", "read", "wait"):
            for a in args:
                if not a.startswith("-"):
                    search_var(a, found, "unsets" if prog == "unset" else "sets")
        elif prog == "printf":
            for j, a in enumerate(args):
                if a == "-v" and j + 1 < len(args):
                    search_var(args[j + 1], found)
                elif a.startswith("-v"):
                    search_var(a[2:], found)
                elif not a.startswith("-"):
                    break
        return
    if "/" in prog and not prog.startswith("/"):
        found.problems.append(f"`{prog}` names its program by a path from the directory the check is in; name an image program, or, for a workspace file, run it with its interpreter, such as `sh {prog}` or `python3 {prog}`")
        return
    if not image_program(prog):
        if "/" in prog:
            found.problems.append(f"`{prog}` would run a workspace file directly, and a check VM does not execute workspace files; run it with its interpreter, such as `sh {prog}` or `python3 {prog}`")
        else:
            found.problems.append(f"`{prog}` is not in the sandbox image")
        return
    base = os.path.basename(prog)
    if PYTHON.match(base):
        check_python(args, found)
    elif base in SHELLS:
        check_shell(base, args, depth, found)
    elif base in AWKS:
        check_awk(base, args, found)
    elif base == "sed":
        check_sed(args, found)
    elif base == "jq":
        check_jq(args, found)
    elif base == "find":
        if set(args) & {"-exec", "-execdir", "-ok", "-okdir"}:
            found.runs.append("`find -exec` runs other programs")
    elif base == "sort":
        if any(long_name(a, {"--compress-program"}) for a in args):
            found.runs.append("`sort --compress-program` runs another program")
    elif base not in READERS:
        found.runs.append(f"`{base}` is not one of the image's programs known only to read what they are given, so it may run workspace code")


def check_shell(base, args, depth, found):
    for j, a in enumerate(args):
        if a.startswith("-") and not a.startswith("--") and "c" in a[1:]:
            if j + 1 >= len(args):
                found.problems.append(f"`{base} {a}` has no command")
            elif built(args[j + 1], "shell code", found):
                pass
            elif depth > 3:
                found.problems.append("the command nests shells too deeply to resolve")
            else:
                # shlex keeps the backslash of "\$" and "\`" in double quotes,
                # where the shell drops it: read them as the inner shell may
                # see them. (Where they were literal, this only refuses more.)
                check(args[j + 1].replace("\\$", "$").replace("\\`", "`"), depth + 1, found)
            return
        if a == "-s" or a == "-":
            break
        if not a.startswith("-"):
            found.runs.append(f"`{base} {shown(a)}` runs a workspace script")
            return
    found.runs.append(f"`{base}` runs the commands it reads from its input")


# python3 -X options known to leave where modules and their bytecode are
# found alone. pycache_prefix moves where bytecode is read from.
SAFE_X = {
    "utf8", "dev", "faulthandler", "importtime", "tracemalloc", "int_max_str_digits", "warn_default_encoding",
    "no_debug_ranges", "frozen_modules", "showrefcount", "perf", "cpu_count",
}


def check_python(args, found):
    """Read python3's options as it does: short options cluster (-bE), and
    -c, -m, -W and -X take the rest of the word or the next word."""
    j = 0
    while j < len(args):
        a = args[j]
        if a == "-":
            break
        if a == "--":
            if j + 1 < len(args):
                found.runs.append(f"`python3 {shown(args[j + 1])}` runs a workspace script")
                return
            break
        if a.startswith("--"):
            j += 2 if a == "--check-hash-based-pycs" else 1
            continue
        if not a.startswith("-"):
            found.runs.append(f"`python3 {shown(a)}` runs a workspace script")
            return
        took = 1
        for k in range(1, len(a)):
            o = a[k]
            if o in "ES":
                found.problems.append(f"`python3 {a}` turns off the check environment")
                return
            if o not in "cmWX":
                continue
            value, took = option_value(args, j, k)
            if o == "c":
                if not built(value, "python3 -c code", found):
                    check_code(value, found)
                return
            if o == "m":
                check_module(value, found)
                return
            if o == "W" and "." in value:
                # A warning filter imports the module that names its category.
                found.runs.append(f"`python3 -W {shown(value)}` imports the module its category names")
            if o == "X":
                key = value.split("=", 1)[0]
                if key == "pycache_prefix":
                    found.problems.append("`python3 -X pycache_prefix` changes where the check's modules are read from")
                    return
                if key not in SAFE_X:
                    found.runs.append(f"`python3 -X {shown(key)}` is not an option known to leave where modules come from alone")
            break
        j += took
    found.runs.append("`python3` runs the code it reads from its input")


def check_module(mod, found):
    if not mod:
        found.problems.append("`python3 -m` names no module")
    elif EXPANDED in mod:
        found.problems.append(f"`python3 -m {shown(mod)}` names its module only when the check runs")
    elif not image_module(mod):
        found.runs.append(f"`python3 -m {mod}` runs a module from the workspace")
    elif mod not in DATA_MODULES:
        found.runs.append(f"`python3 -m {mod}` runs code from the workspace")


def check_code(code, found):
    """python3 -c code is appliance-verified only when it imports only
    modules known not to run other code, and reaches os and sys only through
    attributes known to be safe; anything else is self-checked."""
    try:
        tree = ast.parse(code)
    except SyntaxError:
        found.runs.append("its python3 -c code cannot be parsed, so what it runs is unknown")
        return
    parents = {}
    for node in ast.walk(tree):
        for child in ast.iter_child_nodes(node):
            parents[child] = node
    # Names bound to os or sys, which may only be used through safe attributes.
    restricted = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for n in node.names:
                top = n.name.split(".")[0]
                if top in RESTRICTED and (n.asname is None or n.name == top):
                    restricted[n.asname or top] = top
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for n in node.names:
                safe_module(n.name, found)
        elif isinstance(node, ast.ImportFrom):
            mod = node.module or ""
            if node.level or not mod:
                found.runs.append("its python3 -c code imports a relative module, which is not in the image")
                continue
            safe_module(mod, found)
            if mod in RESTRICTED:
                for n in node.names:
                    if n.name not in RESTRICTED[mod]:
                        found.runs.append(f"its python3 -c code imports {mod}.{n.name}, which can run other code")
        elif isinstance(node, ast.Name):
            if node.id in UNSAFE_NAMES:
                found.runs.append(f"its python3 -c code uses {node.id}, which can run other code")
            elif node.id in restricted:
                mod = restricted[node.id]
                p = parents.get(node)
                if not (isinstance(p, ast.Attribute) and p.value is node and p.attr in RESTRICTED[mod]):
                    attr = p.attr if isinstance(p, ast.Attribute) and p.value is node else None
                    found.runs.append(f"its python3 -c code uses {mod}.{attr}, which can run other code" if attr else f"its python3 -c code uses {mod} other than through known attributes")
        elif isinstance(node, ast.Attribute) and node.attr.startswith("__"):
            found.runs.append(f"its python3 -c code reaches {node.attr}, which can reach code by name")
        elif isinstance(node, ast.Constant) and isinstance(node.value, str) and "__" in node.value:
            found.runs.append("its python3 -c code holds a string naming a special attribute, which can reach code by name")


def safe_module(name, found):
    """A module is safe when it, or the package it is in, is in SAFE_MODULES;
    of os and sys, only os.path."""
    parts = name.split(".")
    if not image_module(parts[0]):
        found.runs.append(f"its python3 -c code imports {name}, which is not in the image")
    elif parts[0] in RESTRICTED and name not in SAFE_MODULES:
        found.runs.append(f"its python3 -c code imports {name}, which can run other code")
    elif not any(".".join(parts[:k]) in SAFE_MODULES for k in range(1, len(parts) + 1)):
        found.runs.append(f"its python3 -c code imports {name}, which is not known to only read data")


AWK_FILES = {"--file", "--include", "--load", "--exec"}
AWK_LONG = AWK_FILES | {"--source", "--field-separator", "--assign"}


def check_awk(base, args, found):
    """awk program text can run commands (system, pipes) and load code
    (@include, @load, -f, -i, -l, -E, mawk's -W exec). Every argument is
    looked at, options clustered (-bf) or after the program included: an
    option the check cannot place makes it self-checked."""
    texts, program, j = [], None, 0
    while j < len(args):
        a = args[j]
        took = 1
        if a.startswith("--") and a != "--":
            name = long_name(a, AWK_LONG)
            if name in AWK_FILES:
                found.runs.append(f"`{base} {a}` runs a program file from the workspace")
                return
            if name in ("--source", "--field-separator", "--assign"):
                value, took = (a.split("=", 1)[1], 1) if "=" in a else ((args[j + 1] if j + 1 < len(args) else ""), 2)
                if name == "--source":
                    texts.append(value)
        elif a.startswith("-") and len(a) > 1 and a != "--":
            for k in range(1, len(a)):
                o = a[k]
                if o in "fEilW":
                    found.runs.append(f"`{base} {a}` runs a program file from the workspace")
                    return
                if o in "Fve":
                    value, took = option_value(args, j, k)
                    if o == "e":
                        texts.append(value)
                    break
        elif program is None and not texts:
            program = args[j + 1] if a == "--" and j + 1 < len(args) else a
            took = 2 if a == "--" else 1
        j += took
    for t in texts + ([program] if program is not None else []):
        if built(t, f"{base} program", found):
            return
        if re.search(r"system|\||@|getline", t):
            found.runs.append(f"its {base} program can run commands or load code")
            return


# sed scripts known to only print or delete lines: addresses, then p, d, q,
# n or =, or a substitution with / that neither runs (e) nor writes (w).
SED_SAFE = re.compile(r"^\s*(?:(?:\d+|\$)(?:\s*,\s*(?:\d+|\$))?|/[^/\n]*/)?\s*!?\s*(?:[pdqn=]|s/(?:[^/\\]|\\.)*/(?:[^/\\]|\\.)*/[gpiI0-9]*)\s*$")


SED_LONG = {
    "--expression", "--file", "--sandbox", "--line-length", "--in-place", "--quiet", "--silent", "--debug",
    "--posix", "--regexp-extended", "--separate", "--unbuffered", "--null-data", "--zero-terminated",
    "--follow-symlinks", "--binary", "--help", "--version",
}


def check_sed(args, found):
    """GNU sed runs commands with its e command and s///e; --sandbox turns
    that off. Its options may come after the files and cluster (-ne), and
    the first operand is the script only when no -e gives one."""
    scripts, operands, j, sandboxed = [], [], 0, False
    while j < len(args):
        a = args[j]
        took = 1
        if a == "--":
            operands.extend(args[j + 1 :])
            break
        if a.startswith("--"):
            name = long_name(a, SED_LONG)
            if name is None:
                found.runs.append(f"`sed {a}` is an option the check cannot resolve")
                return
            if name == "--file":
                found.runs.append("`sed --file` runs a program file from the workspace")
                return
            if name == "--sandbox":
                sandboxed = True
            elif name in ("--expression", "--line-length"):
                value, took = (a.split("=", 1)[1], 1) if "=" in a else ((args[j + 1] if j + 1 < len(args) else ""), 2)
                if name == "--expression":
                    scripts.append(value)
        elif a.startswith("-") and len(a) > 1:
            for k in range(1, len(a)):
                o = a[k]
                if o == "f":
                    found.runs.append("`sed -f` runs a program file from the workspace")
                    return
                if o in "el":
                    value, took = option_value(args, j, k)
                    if o == "e":
                        scripts.append(value)
                    break
                if o == "i":
                    break  # the rest of the word is its suffix
                if o not in "nrEsuzb":
                    found.runs.append(f"`sed {a}` is an option the check cannot resolve")
                    return
        else:
            operands.append(a)
        j += took
    if not scripts and operands:
        scripts.append(operands[0])
    for s in scripts:
        if built(s, "sed script", found):
            return
        if not sandboxed and not all(SED_SAFE.match(part) for part in s.split(";") if part.strip()):
            found.runs.append("its sed script is not one known only to print or delete lines, and a sed script can run commands (its e command); add --sandbox")
            return


def check_jq(args, found):
    """jq runs no commands, but can load its program or modules from files.
    Its options may come anywhere, after the program too, and it reads a
    short option in any cluster (-nf is -n -f); long ones are exact."""
    program, j = None, 0
    while j < len(args):
        a = args[j]
        if a.startswith("-") and len(a) > 1 and not a.startswith("--"):
            if set(a[1:]) & set("fL"):
                found.runs.append(f"`jq {a}` loads its program or modules from the workspace")
                return
        elif a in ("--from-file", "--library-path") or a.startswith("--library-path="):
            found.runs.append(f"`jq {a}` loads its program or modules from the workspace")
            return
        elif a in ("--arg", "--argjson", "--slurpfile", "--rawfile"):
            j += 3
            continue
        elif a == "--indent":
            j += 2
            continue
        elif not a.startswith("--") and program is None:
            program = a
        j += 1
    if program is not None and not built(program, "jq program", found) and re.search(r"\b(import|include)\b", program):
        found.runs.append("its jq program loads a module from the workspace")


if __name__ == "__main__":
    commands = json.loads(base64.b64decode(sys.argv[1]))
    out = []
    for c in commands:
        f = check(c)
        out.append({"problems": [shown(p) for p in f.problems], "self_checked": list(dict.fromkeys(shown(r) for r in f.runs))})
    print(json.dumps(out))
