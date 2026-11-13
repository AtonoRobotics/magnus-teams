"""The two guest images sandbox/Dockerfile builds from one set of sources
(decisions/2026-11-13-minimal-computer-image.md).

The computer's base image is the minimal one that gets its work done (owner,
2026-11-13; spec: the base image holds "OS, desktop, browser, Magnus guest
agent"). It must hold every program the guest, the desktop and the kernel
run on a computer, and none of the sandbox's document toolchain. The
per-task sandbox's image keeps everything it had.
"""
import os
import re
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

# What the per-task sandbox's image held before the computer had its own,
# and must go on holding: the agent's document work uses it.
SANDBOX_APT = set(
    """coreutils findutils grep sed gawk procps file less zip unzip xz-utils bzip2
    poppler-utils qpdf ghostscript imagemagick tesseract-ocr
    libreoffice-writer libreoffice-calc libreoffice-impress fonts-dejavu fonts-liberation fonts-noto-core
    xvfb xdotool x11-utils x11-xserver-utils x11vnc wmctrl
    xfwm4 xfce4-panel xfdesktop4 xfce4-settings xfconf thunar xfce4-terminal dbus
    fonts-inter fonts-jetbrains-mono papirus-icon-theme librsvg2-bin librsvg2-common libnss3-tools
    sqlite3 git jq ca-certificates curl iproute2 chromium nodejs""".split()
)
SANDBOX_PIP = set(
    """python-docx openpyxl xlsxwriter python-pptx reportlab pypdf pdfplumber pymupdf
    pandas numpy matplotlib pillow markdown beautifulsoup4 lxml tabulate jinja2
    scipy scikit-learn pytest python-xlib==0.33""".split()
)

# Left out of the computer: the document toolchain, OCR, the office suite,
# the data-science stack and the sandbox's extra tools. The agent installs
# what it needs with computer_install_app.
DROPPED_APT = set(
    """poppler-utils qpdf ghostscript tesseract-ocr libreoffice-writer libreoffice-calc
    libreoffice-impress fonts-noto-core sqlite3 git jq iproute2 gawk file less zip bzip2""".split()
)
DROPPED_PIP = SANDBOX_PIP - {"python-xlib==0.33"}

# What runs on a computer, where it is run from, and the package it needs.
# Each program must still be named in its source, so this list follows them.
NEEDED = [
    ("x11vnc", "sandbox/guest/src/main.rs", "x11vnc"),
    ("pgrep", "sandbox/guest/src/main.rs", "procps"),
    ("/usr/bin/node", "sandbox/guest/src/browser.rs", "nodejs"),
    ("/usr/bin/chromium", "sandbox/guest/src/browser.rs", "chromium"),
    ("xdotool", "crates/magnus/src/boundary.rs", "xdotool"),
    ("import -window root", "crates/magnus/src/boundary.rs", "imagemagick"),
    ("update-ca-certificates", "crates/magnus/src/computer/service.rs", "ca-certificates"),
    ("curl -fLo", "crates/magnus/instructions/tools.json", "curl"),
    ("Xvfb :0", "sandbox/desktop/magnus-desktop", "xvfb"),
    ("xdpyinfo", "sandbox/desktop/magnus-desktop", "x11-utils"),
    ("xset s off", "sandbox/desktop/magnus-desktop", "x11-xserver-utils"),
    ("dbus-daemon", "sandbox/desktop/magnus-desktop", "dbus"),
    ("xfconf-query", "sandbox/desktop/magnus-desktop", "xfconf"),
    ("xfsettingsd", "sandbox/desktop/magnus-desktop", "xfce4-settings"),
    ("xfwm4 --compositor=off", "sandbox/desktop/magnus-desktop", "xfwm4"),
    ("(xfdesktop ", "sandbox/desktop/magnus-desktop", "xfdesktop4"),
    ("(xfce4-panel ", "sandbox/desktop/magnus-desktop", "xfce4-panel"),
    ("thunar", "sandbox/desktop/applications/magnus-files.desktop", "thunar"),
    ("xfce4-terminal", "sandbox/desktop/applications/magnus-terminal.desktop", "xfce4-terminal"),
    ("exec chromium", "sandbox/desktop/magnus-browser", "chromium"),
    ("certutil", "sandbox/desktop/magnus-browser", "libnss3-tools"),
    ("rsvg-convert", "sandbox/desktop/install.sh", "librsvg2-bin"),
    ("Papirus", "sandbox/desktop/install.sh", "papirus-icon-theme"),
    ("Inter 10", "sandbox/desktop/xfconf/xsettings.xml", "fonts-inter"),
    ("JetBrains Mono 10", "sandbox/desktop/xfconf/xsettings.xml", "fonts-jetbrains-mono"),
]
NEEDED_PIP = [("from Xlib import", "sandbox/desktop/arrange.py", "python-xlib==0.33")]


def stages(text):
    """Each stage: the stage it is built from, its apt and pip packages, and what it copies."""
    out, cur = {}, None
    lines = re.sub(r"\\\n", " ", text).splitlines()
    for line in lines:
        line = line.strip()
        m = re.match(r"FROM (\S+)(?: AS (\S+))?$", line)
        if m:
            cur = m.group(2) or m.group(1)
            out[cur] = {"from": m.group(1), "apt": set(), "pip": set(), "copy": [], "run": []}
            continue
        if cur is None or line.startswith("#"):
            continue
        if line.startswith("RUN"):
            out[cur]["run"].append(line)
            m = re.search(r"apt-get install -y --no-install-recommends (.*?)(?:&&|$)", line)
            if m:
                out[cur]["apt"] |= set(m.group(1).split())
            m = re.search(r"pip install --no-cache-dir (.*?)(?:&&|$)", line)
            if m:
                out[cur]["pip"] |= set(m.group(1).split())
        elif line.startswith("COPY"):
            out[cur]["copy"].append(line)
    return out


def built(st, name):
    """A stage's packages and copies, with those of the stages it is built from."""
    apt, pip, copy = set(), set(), []
    while name in st:
        s = st[name]
        apt |= s["apt"]
        pip |= s["pip"]
        copy += s["copy"]
        name = s["from"]
    return apt, pip, copy


def read(rel):
    with open(os.path.join(ROOT, rel), encoding="utf-8") as f:
        return f.read()


class Images(unittest.TestCase):
    def setUp(self):
        self.st = stages(read("sandbox/Dockerfile"))

    def test_both_images_are_exported_from_their_stages(self):
        export = " ".join(self.st["export"]["copy"])
        self.assertIn("/rootfs.ext4", export)
        self.assertIn("/computer.ext4", export)
        packs = {n: s for n, s in self.st.items() if any(".ext4" in r for r in s["run"])}
        made = {re.search(r"/(\w+\.ext4) ", " ".join(s["run"])).group(1): " ".join(s["copy"]) for s in packs.values()}
        self.assertIn("--from=rootfs /", made["rootfs.ext4"])
        self.assertIn("--from=computer /", made["computer.ext4"])

    def test_the_computer_leaves_out_the_document_toolchain(self):
        apt, pip, copy = built(self.st, "computer")
        self.assertFalse(apt & DROPPED_APT, f"the computer image carries {sorted(apt & DROPPED_APT)}")
        self.assertFalse(pip & DROPPED_PIP, f"the computer image carries {sorted(pip & DROPPED_PIP)}")
        self.assertFalse([p for p in apt if p.startswith("libreoffice")], "the computer image carries LibreOffice")
        copied = " ".join(copy)
        for f in ("check_tools.py", "sitecustomize.py"):
            self.assertNotIn(f, copied, f"{f} is the sandbox's contract checks, not the computer's")

    def test_the_computer_has_what_runs_on_it(self):
        apt, pip, copy = built(self.st, "computer")
        for program, source, package in NEEDED:
            self.assertIn(program, read(source), f"{source} no longer runs {program!r}: update this list")
            self.assertIn(package, apt, f"{source} runs {program!r} on the computer, from {package}, which the image leaves out")
        for module, source, package in NEEDED_PIP:
            self.assertIn(module, read(source))
            self.assertIn(package, pip)
        copied = " ".join(copy)
        for f in ("magnus-guest /sbin/magnus-guest", "browser /usr/local/lib/magnus/browser", "desktop /tmp/magnus-desktop-src"):
            self.assertIn(f, copied)
        self.assertTrue(any("desktop-src/install.sh" in r for r in self.st["computer"]["run"]), "the desktop is not installed")
        self.assertTrue(any("useradd -u 1000" in r for r in self.st["computer"]["run"]), "no user for the identity browser")

    def test_the_computer_is_built_from_debian_with_its_package_manager(self):
        base = "computer"
        while base in self.st:
            base = self.st[base]["from"]
        self.assertTrue(base.startswith("python:3.12-slim-bookworm"), base)

    def test_the_sandbox_keeps_everything_it_had(self):
        apt, pip, copy = built(self.st, "rootfs")
        self.assertEqual(apt, SANDBOX_APT)
        self.assertEqual(pip, SANDBOX_PIP)
        copied = " ".join(copy)
        for f in ("magnus-guest", "sitecustomize.py", "check_tools.py", "browser", "desktop"):
            self.assertIn(f, copied)

    def test_the_images_share_their_common_layers(self):
        self.assertEqual(self.st["computer"]["from"], self.st["rootfs"]["from"])


if __name__ == "__main__":
    unittest.main()
