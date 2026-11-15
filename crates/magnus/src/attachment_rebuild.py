"""A file from outside, rebuilt clean for the run's workspace.

Runs in the attachment's own sandbox (no network, nothing of the person's),
started by `import_attachment` in attachment.rs. The original is at
/workspace/inputs/attachment and its name at /workspace/inputs/name. Clean
copies are written under /workspace/clean, and a manifest goes to stdout:
{"files": [{"path", "kind", "from"}], "skipped": [{"name", "why"}]}.

Each kind is rebuilt from its content alone, so nothing else of the original
(macros, scripts, metadata, embedded files, a second file hidden after the
first, bytes shaped to break a reader) comes with it:
- a picture from its pixels (PNG where it may be transparent, else JPEG);
- a PDF re-rendered from its pages (no JavaScript, forms or attachments);
- an Office or OpenDocument file re-saved by LibreOffice in a macro-free
  format of the same family;
- text, data and code as clean UTF-8, with `.txt` after anything a system
  would run by its name;
- an archive unpacked here (no links, no paths out, limits on count, size
  and depth), each member rebuilt by these same rules.
An executable is never imported. Audio, video and other data with no
rebuild here come in unchanged, never executable.
"""

import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import zipfile

SOURCE = "/workspace/inputs/attachment"
NAME_FILE = "/workspace/inputs/name"
OUT = "/workspace/clean"
WORK = "/workspace/unpacked"

MAX_FILES = 500
MAX_TOTAL_BYTES = 512 * 1024 * 1024
MAX_DEPTH = 3
MAX_SIDE_PX = 4096
JPEG_QUALITY_PERCENT = 92
MAX_NAME_CHARS = 80
TOOL_TIMEOUT_SECONDS = 120

TRANSPARENT = {"image/png", "image/gif", "image/webp", "image/bmp", "image/tiff", "image/svg+xml", "image/x-icon", "image/vnd.microsoft.icon"}
# The format each is saved in: the same family, with no macros.
OFFICE = {
    "docx": "docx:MS Word 2007 XML", "docm": "docx:MS Word 2007 XML", "dotx": "docx:MS Word 2007 XML", "dotm": "docx:MS Word 2007 XML",
    "doc": "docx:MS Word 2007 XML", "dot": "docx:MS Word 2007 XML", "rtf": "docx:MS Word 2007 XML", "wpd": "docx:MS Word 2007 XML",
    "xlsx": "xlsx:Calc MS Excel 2007 XML", "xlsm": "xlsx:Calc MS Excel 2007 XML", "xltx": "xlsx:Calc MS Excel 2007 XML", "xls": "xlsx:Calc MS Excel 2007 XML",
    "pptx": "pptx:Impress MS PowerPoint 2007 XML", "pptm": "pptx:Impress MS PowerPoint 2007 XML", "ppsx": "pptx:Impress MS PowerPoint 2007 XML", "ppt": "pptx:Impress MS PowerPoint 2007 XML", "pps": "pptx:Impress MS PowerPoint 2007 XML",
    "odt": "odt", "ott": "odt", "ods": "ods", "ots": "ods", "odp": "odp", "otp": "odp",
}
OFFICE_MIME = ("application/msword", "application/vnd.ms-", "application/vnd.openxmlformats-officedocument", "application/vnd.oasis.opendocument", "text/rtf", "application/rtf")
EXECUTABLE_MIME = (
    "application/x-executable", "application/x-pie-executable", "application/x-sharedlib", "application/x-dosexec",
    "application/x-mach-binary", "application/vnd.microsoft.portable-executable", "application/x-msi", "application/x-ms-installer",
    "application/x-msdownload", "application/java-archive", "application/vnd.android.package-archive", "application/x-elf",
    "application/x-object", "application/x-coff", "application/x-ms-shortcut", "application/x-apple-diskimage",
)
EXECUTABLE_EXT = {"exe", "dll", "com", "scr", "msi", "msp", "sys", "cpl", "jar", "apk", "app", "dmg", "deb", "rpm", "so", "dylib", "lnk", "elf", "bin"}
# Run by name somewhere, so they come in as text with `.txt` after.
RUNNABLE_EXT = {"sh", "bash", "zsh", "csh", "ksh", "fish", "command", "bat", "cmd", "ps1", "psm1", "vbs", "vbe", "js", "jse", "mjs", "wsf", "wsh", "hta", "py", "pyw", "pl", "rb", "php", "lua", "applescript", "scpt", "reg", "inf", "desktop", "url", "service"}
ARCHIVE_MIME = {"application/zip", "application/x-tar", "application/gzip", "application/x-gzip", "application/x-bzip2", "application/x-xz", "application/x-compressed-tar"}
TEXT_MIME = ("text/", "application/json", "application/xml", "application/x-ndjson", "application/javascript", "application/x-sh", "application/x-shellscript", "application/x-yaml", "application/sql", "application/x-subrip", "application/ld+json", "message/rfc822", "inode/x-empty")
CONTROL = re.compile("[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]")

files, skipped, total = [], [], [0]


def mime(path):
    try:
        return subprocess.run(["file", "-b", "--mime-type", path], capture_output=True, text=True, timeout=TOOL_TIMEOUT_SECONDS).stdout.strip()
    except Exception:
        return "application/octet-stream"


def safe(part):
    """One path part from outside as a safe name: letters, digits, . - _"""
    s = re.sub(r"[^A-Za-z0-9._-]", "_", part)[:MAX_NAME_CHARS].lstrip(".")
    return s or "file"


def split_ext(name):
    stem, dot, ext = name.rpartition(".")
    return (stem, ext.lower()) if dot and stem else (name, "")


def place(rel_dir, name):
    """A path under OUT that nothing else has taken."""
    stem, ext = split_ext(name)
    n = 1
    while True:
        cand = f"{stem}{'' if n == 1 else f'-{n}'}{'.' + ext if ext else ''}"
        rel = os.path.join(rel_dir, cand) if rel_dir else cand
        if not os.path.exists(os.path.join(OUT, rel)):
            os.makedirs(os.path.join(OUT, rel_dir), exist_ok=True)
            return rel
        n += 1


def room(size):
    if len(files) >= MAX_FILES or total[0] + size > MAX_TOTAL_BYTES:
        return False
    return True


def kept(rel, kind, origin):
    p = os.path.join(OUT, rel)
    size = os.path.getsize(p)
    if not room(size):
        os.remove(p)
        skipped.append({"name": origin, "why": "over the limit of what one import brings in"})
        return
    os.chmod(p, stat.S_IRUSR | stat.S_IWUSR | stat.S_IRGRP | stat.S_IROTH)
    total[0] += size
    files.append({"path": rel, "kind": kind, "from": origin})


def run(cmd):
    return subprocess.run(cmd, capture_output=True, timeout=TOOL_TIMEOUT_SECONDS).returncode == 0


def picture(path, rel_dir, stem, origin, m):
    ext = "png" if m in TRANSPARENT else "jpg"
    rel = place(rel_dir, f"{stem}.{ext}")
    out = os.path.join(OUT, rel)
    cmd = ["convert", f"{path}[0]", "-auto-orient", "-strip", "-resize", f"{MAX_SIDE_PX}x{MAX_SIDE_PX}>"]
    cmd += [f"png:{out}"] if ext == "png" else ["-quality", str(JPEG_QUALITY_PERCENT), f"jpg:{out}"]
    if run(cmd) and os.path.getsize(out) > 0:
        kept(rel, "picture", origin)
    else:
        if os.path.exists(out):
            os.remove(out)
        skipped.append({"name": origin, "why": "a picture that could not be rebuilt from its pixels"})


def pdf(path, rel_dir, stem, origin):
    rel = place(rel_dir, f"{stem}.pdf")
    out = os.path.join(OUT, rel)
    if run(["pdftocairo", "-pdf", path, out]) and os.path.getsize(out) > 0:
        kept(rel, "pdf", origin)
    else:
        if os.path.exists(out):
            os.remove(out)
        skipped.append({"name": origin, "why": "a PDF that could not be re-rendered from its pages"})


def office_target(ext, m):
    """What to save as: by the name's extension, else by what `file` saw."""
    if ext in OFFICE:
        return OFFICE[ext]
    for word, ext_ in (("spreadsheet", "ods" if "opendocument" in m else "xlsx"), ("ms-excel", "xlsx"), ("presentation", "odp" if "opendocument" in m else "pptx"), ("powerpoint", "pptx"), ("opendocument.text", "odt")):
        if word in m:
            return OFFICE[ext_]
    return OFFICE["docx"]


def office(path, rel_dir, stem, ext, origin, m):
    target = office_target(ext, m)
    new_ext = target.split(":")[0]
    tmp = os.path.join(WORK, "office-out")
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    src = os.path.join(WORK, f"in.{ext or 'doc'}")
    shutil.copyfile(path, src)
    ok = run(["soffice", "-env:UserInstallation=file:///tmp/lo-profile", "--headless", "--norestore", "--convert-to", target, "--outdir", tmp, src])
    made = os.path.join(tmp, f"in.{new_ext}")
    os.remove(src)
    if ok and os.path.exists(made) and os.path.getsize(made) > 0:
        rel = place(rel_dir, f"{stem}.{new_ext}")
        shutil.move(made, os.path.join(OUT, rel))
        kept(rel, "document", origin)
    else:
        skipped.append({"name": origin, "why": "a document that LibreOffice could not re-save"})


def text(path, rel_dir, name, ext, origin):
    raw = open(path, "rb").read()
    if raw.startswith(b"\xff\xfe") or raw.startswith(b"\xfe\xff"):
        s = raw.decode("utf-16", errors="replace")
    else:
        s = raw.decode("utf-8-sig", errors="replace")
    s = CONTROL.sub("", s)
    runnable = ext in RUNNABLE_EXT or s.startswith("#!")
    rel = place(rel_dir, f"{name}.txt" if runnable else name)
    with open(os.path.join(OUT, rel), "w", encoding="utf-8") as f:
        f.write(s)
    kept(rel, "text", origin)


def data(path, rel_dir, name, origin):
    rel = place(rel_dir, name)
    shutil.copyfile(path, os.path.join(OUT, rel))
    kept(rel, "data", origin)


def unpack(path, rel_dir, stem, m, origin, depth):
    if depth >= MAX_DEPTH:
        skipped.append({"name": origin, "why": "an archive nested too deep"})
        return
    dest = os.path.join(WORK, f"d{depth}-{len(files)}-{len(skipped)}")
    os.makedirs(dest)
    members = []
    try:
        if m == "application/zip":
            with zipfile.ZipFile(path) as z:
                infos = [i for i in z.infolist() if not i.is_dir()]
                if len(infos) > MAX_FILES or sum(i.file_size for i in infos) > MAX_TOTAL_BYTES:
                    raise ValueError("too big unpacked")
                for i in infos:
                    if stat.S_ISLNK(i.external_attr >> 16):
                        skipped.append({"name": f"{origin}/{i.filename}", "why": "a link"})
                        continue
                    members.append((i.filename, z.read(i)))
        elif m in ("application/gzip", "application/x-gzip", "application/x-bzip2", "application/x-xz") and not tarfile.is_tarfile(path):
            import bz2, gzip, lzma
            opener = {"application/x-bzip2": bz2.open, "application/x-xz": lzma.open}.get(m, gzip.open)
            with opener(path, "rb") as f:
                body = f.read(MAX_TOTAL_BYTES + 1)
            if len(body) > MAX_TOTAL_BYTES:
                raise ValueError("too big unpacked")
            members.append((split_ext(stem)[0] if stem.endswith(".tar") else stem, body))
        else:
            with tarfile.open(path) as t:
                infos = [i for i in t.getmembers() if i.isfile()]
                for i in t.getmembers():
                    if not i.isfile() and not i.isdir():
                        skipped.append({"name": f"{origin}/{i.name}", "why": "a link or device"})
                if len(infos) > MAX_FILES or sum(i.size for i in infos) > MAX_TOTAL_BYTES:
                    raise ValueError("too big unpacked")
                for i in infos:
                    members.append((i.name, t.extractfile(i).read()))
    except Exception as e:
        skipped.append({"name": origin, "why": f"an archive that could not be unpacked safely ({type(e).__name__})"})
        return
    for n, (inner, body) in enumerate(members):
        parts = [safe(p) for p in inner.replace("\\", "/").split("/") if p not in ("", ".", "..")]
        if not parts:
            continue
        member = os.path.join(dest, str(n))
        with open(member, "wb") as f:
            f.write(body)
        sub = os.path.join(rel_dir, stem, *parts[:-1])
        rebuild(member, sub, parts[-1], f"{origin}/{inner}", depth + 1)
        if os.path.exists(member):
            os.remove(member)


def rebuild(path, rel_dir, name, origin, depth):
    name = safe(name)
    stem, ext = split_ext(name)
    m = mime(path)
    if m.startswith(EXECUTABLE_MIME) or ext in EXECUTABLE_EXT:
        skipped.append({"name": origin, "why": "a program, which never comes into the workspace"})
    elif m.startswith("image/"):
        picture(path, rel_dir, stem, origin, m)
    elif m == "application/pdf":
        pdf(path, rel_dir, stem, origin)
    elif m.startswith(OFFICE_MIME) or (ext in OFFICE and m in ("application/zip", "application/octet-stream", "application/x-ole-storage", "application/CDFV2")):
        office(path, rel_dir, stem, ext, origin, m)
    elif m in ARCHIVE_MIME:
        unpack(path, rel_dir, stem, m, origin, depth)
    elif m in ("application/x-7z-compressed", "application/vnd.rar", "application/x-rar", "application/x-rar-compressed"):
        skipped.append({"name": origin, "why": "an archive of a kind that cannot be unpacked here (7z, RAR)"})
    elif m.startswith(TEXT_MIME):
        text(path, rel_dir, name, ext, origin)
    else:
        data(path, rel_dir, name, origin)


def main():
    os.makedirs(OUT, exist_ok=True)
    os.makedirs(WORK, exist_ok=True)
    name = open(NAME_FILE, encoding="utf-8", errors="replace").read().strip() or "attachment"
    rebuild(SOURCE, "", name.replace("\\", "/").split("/")[-1], name, 0)
    json.dump({"files": files, "skipped": skipped}, sys.stdout)


if __name__ == "__main__":
    main()
