# The computer has its own minimal base image

The owner (2026-11-13): "The computer should have the minimal image possible
to get the work done." The spec agrees. Its layer table says the computer's
Base image holds "OS, desktop, browser, Magnus guest agent", immutable and
versioned by Magnus, and every other app is the agent's to install: "The
agent decides when an app is needed and installs it itself, with
administrator rights inside the VM" (computer_install_app, recorded in the
manifest and replayed after a rebuild).

Until now one image served two roles. `sandbox/Dockerfile` built a single
`rootfs.ext4` (about 2.7 GB) holding LibreOffice, Tesseract, Ghostscript,
poppler, qpdf, the data-science Python stack and the document libraries.
The per-task sandbox needs them; the persistent computer does not.

Choice: `sandbox/Dockerfile` builds two images from the same sources. Stage
`common` holds what both need; `rootfs` adds the document toolchain and is
the per-task sandbox's `rootfs.ext4`, unchanged in content; `computer` adds
only the guest, the browser sidecar and the desktop, and is packed as
`computer.ext4`. The export holds both. A release carries both and links
`computer.ext4` beside the programs, and names it in `images.sha256`.
`magnus-computerd self-update` installs the release's `computer.ext4`, never
its `rootfs.ext4`, under the name computerd always gave its base image
(`images/rootfs.ext4`), so installed machines keep their layout. Its
`base.id` is the computer image's sha256, so the first install of it is a
changed base: computers get a fresh overlay and replay their recorded apps,
and snapshots made on the old base are discarded, as for any base change.
The kernel compares computerd's base id with `computer.ext4`'s hash, and the
update command in Admin › Operations names `computer.ext4`.

What the computer image keeps, and why:
- Debian (python:3.12-slim-bookworm) with apt and dpkg: the agent installs
  apps with them, and pip with Python.
- Python 3 and python-xlib: the desktop's window arranger (`arrange.py`)
  and its settings check (`magnus-desktop`) run on them.
- procps: the guest starts x11vnc only when `pgrep` finds none.
- Xvfb, x11-utils (xdpyinfo), x11-xserver-utils (xset), x11vnc, xdotool,
  dbus: the screen, the takeover's VNC server and the agent's clicks and keys.
- ImageMagick: every computer screenshot is `import -window root`
  (`crates/magnus/src/boundary.rs`, shared with the sandbox's desktop).
- XFCE (xfwm4, xfce4-panel, xfdesktop4, xfce4-settings, xfconf, Thunar,
  xfce4-terminal), Papirus, librsvg (the wallpaper is rendered at build time
  and the icons are SVG), Inter, JetBrains Mono, DejaVu and Liberation: the
  desktop and its theme (spec rev 145).
- Chromium and libnss3-tools: the computer's Browser, which trusts the
  computer's proxy with `certutil`.
- Node.js, the browser sidecar and the `browser` user: computerd boots
  identity browser VMs (`Purpose::Browser`) from the same base image, and
  their guest runs `/usr/bin/node` with the sidecar.
- ca-certificates, curl, xz-utils, unzip: the kernel runs
  `update-ca-certificates` to trust the proxy, and the agent fetches an
  app's installer with curl (computer_install_app's description) and
  unpacks the common archive kinds.

Left out: LibreOffice, Tesseract, Ghostscript, poppler, qpdf, Noto, wmctrl,
sqlite3, git, jq, iproute2, gawk, file, less, zip, bzip2, the document and
data-science Python packages, and the sandbox's contract-check files
(`check_tools.py`, `sitecustomize.py`). Nothing on the computer's path runs
them; the agent installs any of them when a task needs one.

Tests: `sandbox/test_images.py` (in the fast tier) checks the computer
stage leaves the toolchain out, has a package for every program the guest,
the desktop and the kernel run on a computer (each still named in its
source), and that the sandbox's package list is what it was.
`crates/computerd/src/self_update_tests.rs` checks a release's
`computer.ext4`, not its `rootfs.ext4`, reaches the installer, and
`crates/magnus/src/computer/service_version_tests.rs` that the kernel
compares and installs `computer.ext4`.

Transition: computerd's update runs the installed computerd, which still
takes `rootfs.ext4`. The first release with this change is installed by it,
so the base stays the sandbox image until the next release's update (by the
new computerd) or the update command in Admin › Operations, which shows
computerd behind meanwhile because its base id differs.
