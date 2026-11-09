"""In a contract check (MAGNUS_CHECK=1), the checking tool is the image's.

The guest sets PYTHONSAFEPATH, so neither the working directory nor a
script's directory is put first on the path. Here every path from outside
the image (PYTHONPATH, .pth files) moves behind the image's own, and the
working directory is added last: the work under test can still import itself,
but nothing it writes can stand in for a module the image has.
"""
import os
import sys

if os.environ.get("MAGNUS_CHECK") == "1":
    def _image(p):
        return os.path.realpath(p or ".").startswith("/usr/")

    sys.path[:] = [p for p in sys.path if _image(p)] + [p for p in sys.path if not _image(p)] + [os.getcwd()]
