# The sandbox guest root filesystem. Built by build.sh; mounted read-only by
# every microVM, with a per-run writable /workspace disk.
FROM python:3.12-slim-bookworm AS rootfs
RUN apt-get update && apt-get install -y --no-install-recommends \
      coreutils findutils grep sed gawk procps file less zip unzip xz-utils bzip2 \
      poppler-utils qpdf ghostscript imagemagick tesseract-ocr \
      libreoffice-writer libreoffice-calc libreoffice-impress fonts-dejavu fonts-liberation fonts-noto-core \
      xvfb xdotool x11-utils x11-xserver-utils fluxbox xterm x11vnc \
      sqlite3 git jq ca-certificates curl iproute2 \
    && rm -rf /var/lib/apt/lists/*
RUN pip install --no-cache-dir \
      python-docx openpyxl xlsxwriter python-pptx reportlab pypdf pdfplumber pymupdf \
      pandas numpy matplotlib pillow markdown beautifulsoup4 lxml tabulate jinja2 \
      scipy scikit-learn pytest
RUN mkdir -p /workspace /sbin
COPY magnus-guest /sbin/magnus-guest
COPY sitecustomize.py /usr/local/lib/python3.12/site-packages/sitecustomize.py
COPY check_tools.py /usr/local/lib/magnus/check_tools.py

FROM debian:bookworm-slim AS pack
RUN apt-get update && apt-get install -y --no-install-recommends e2fsprogs && rm -rf /var/lib/apt/lists/*
COPY --from=rootfs / /rootfs
RUN rm -rf /rootfs/proc/* /rootfs/sys/* /rootfs/dev/* && : > /rootfs/etc/resolv.conf && \
    mke2fs -q -t ext4 -L rootfs -d /rootfs -F /rootfs.ext4 4G

FROM scratch AS export
COPY --from=pack /rootfs.ext4 /rootfs.ext4
