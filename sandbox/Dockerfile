# The guest root filesystems, built by build.sh from one set of sources
# (decisions/2026-11-13-minimal-computer-image.md):
# - rootfs.ext4, the per-task sandbox's (stage `rootfs`), mounted read-only
#   by every run's microVM with a per-run writable /workspace disk. It holds
#   the document and data toolchain the agent's work uses.
# - computer.ext4, the persistent computer's base image (stage `computer`),
#   which magnus-computerd boots computers, identity browsers and helpers
#   from. It holds the OS, the desktop, the browser and the guest; the agent
#   installs any other app itself (computer_install_app).
# Both start from `common`, so they share its layers.

# What both have: the shell tools the guest and the kernel run, the screen
# (Xvfb, x11vnc, xdotool, ImageMagick's `import` for screenshots), the
# desktop with its theme, icons and fonts, Chromium, Node for the identity
# browser's sidecar, curl to fetch an app's installer, and Python with
# python-xlib for the desktop's scripts.
FROM python:3.12-slim-bookworm AS common
RUN apt-get update && apt-get install -y --no-install-recommends \
      coreutils findutils grep sed procps xz-utils unzip imagemagick fonts-dejavu fonts-liberation \
      xvfb xdotool x11-utils x11-xserver-utils x11vnc wmctrl \
      xfwm4 xfce4-panel xfdesktop4 xfce4-settings xfconf thunar xfce4-terminal dbus \
      fonts-inter fonts-jetbrains-mono papirus-icon-theme librsvg2-bin librsvg2-common libnss3-tools \
      ca-certificates curl chromium nodejs \
    && rm -rf /var/lib/apt/lists/*
RUN pip install --no-cache-dir python-xlib==0.33

# The per-task sandbox: everything above, and the document toolchain.
FROM common AS rootfs
RUN apt-get update && apt-get install -y --no-install-recommends \
      gawk file less zip bzip2 \
      poppler-utils qpdf ghostscript tesseract-ocr \
      libreoffice-writer libreoffice-calc libreoffice-impress fonts-noto-core \
      sqlite3 git jq iproute2 \
    && rm -rf /var/lib/apt/lists/*
RUN pip install --no-cache-dir \
      python-docx openpyxl xlsxwriter python-pptx reportlab pypdf pdfplumber pymupdf \
      pandas numpy matplotlib pillow markdown beautifulsoup4 lxml tabulate jinja2 \
      scipy scikit-learn pytest
RUN mkdir -p /workspace /sbin
COPY magnus-guest /sbin/magnus-guest
COPY sitecustomize.py /usr/local/lib/python3.12/site-packages/sitecustomize.py
COPY check_tools.py /usr/local/lib/magnus/check_tools.py
COPY browser /usr/local/lib/magnus/browser
# The desktop (spec rev 145): XFCE themed as Magnus, sandbox/desktop.
COPY desktop /tmp/magnus-desktop-src
RUN sh /tmp/magnus-desktop-src/install.sh && rm -rf /tmp/magnus-desktop-src
RUN useradd -u 1000 -M -d /tmp/browser-home browser

# The computer's base image: the OS, the desktop, the browser and the guest.
FROM common AS computer
RUN mkdir -p /workspace /sbin
COPY magnus-guest /sbin/magnus-guest
COPY browser /usr/local/lib/magnus/browser
COPY desktop /tmp/magnus-desktop-src
RUN sh /tmp/magnus-desktop-src/install.sh && rm -rf /tmp/magnus-desktop-src
RUN useradd -u 1000 -M -d /tmp/browser-home browser

FROM debian:bookworm-slim AS pack-tools
RUN apt-get update && apt-get install -y --no-install-recommends e2fsprogs && rm -rf /var/lib/apt/lists/*

FROM pack-tools AS pack
COPY --from=rootfs / /rootfs
RUN rm -rf /rootfs/proc/* /rootfs/sys/* /rootfs/dev/* && : > /rootfs/etc/resolv.conf && \
    mke2fs -q -t ext4 -L rootfs -d /rootfs -F /rootfs.ext4 5G

# Read-only under the computer's overlay: its size is only room for the image.
FROM pack-tools AS pack-computer
COPY --from=computer / /rootfs
RUN rm -rf /rootfs/proc/* /rootfs/sys/* /rootfs/dev/* && : > /rootfs/etc/resolv.conf && \
    mke2fs -q -t ext4 -L rootfs -d /rootfs -F /computer.ext4 3G

FROM scratch AS export
COPY --from=pack /rootfs.ext4 /rootfs.ext4
COPY --from=pack-computer /computer.ext4 /computer.ext4
