"""The [admin] section release/admin-on-appliance.sh keeps in an appliance's
magnus.toml: written the first time, then kept to the console's names and
networks, with its database password as it was. A section someone else
wrote (without the marker line) is left as it is.

    python3 -I release/admin-section.py CONFIG LAN TAILSCALE TSNAME PORT TLS_DIR

LAN is "address/prefix"; TAILSCALE an address or ""; TSNAME the machine's
Tailscale name or "". Prints "changed|same|no", the certificate's names
(openssl subjectAltName form) and the console's hostname, on one line.
"""
import datetime
import ipaddress
import re
import secrets
import shutil
import sys
import tomllib
import urllib.parse as u

MARK = "# The admin console (written by release/admin-on-appliance.sh; admin console spec)."

path, lan, ts, tsname, port, tls = sys.argv[1:7]
text = open(path).read()
c = tomllib.loads(text)
addr = lan.split("/")[0]
tsaddr = ts.split("/")[0] if ts else ""
names = ([tsname] if tsname else []) + [addr] + ([tsaddr] if tsaddr else [])
listen = [f"{addr}:{port}"] + ([f"{tsaddr}:{port}"] if tsaddr else [])
allow = [str(ipaddress.ip_interface(lan).network)] + (["100.64.0.0/10"] if tsaddr else [])
sans = ",".join(("IP:" if re.fullmatch(r"[0-9.]+", n) else "DNS:") + n for n in names)

if "admin" in c and MARK not in text:
    print("no", sans, c["admin"]["hostname"])
    sys.exit()

url = c.get("admin", {}).get("database_url")
if not url:
    d = u.urlsplit(c["database"]["url"])
    url = f"postgres://magnus_root:{secrets.token_hex(24)}@{d.hostname}:{d.port}{d.path}"


def q(xs):
    return "[" + ", ".join(f'"{x}"' for x in xs) + "]"


block = "\n".join([
    MARK,
    "[admin]",
    f'hostname = "{names[0]}"',
    f"aliases = {q(names[1:])}",
    f"listen = {q(listen)}",
    f"allow = {q(allow)}",
    f'tls_cert = "{tls}/console.pem"',
    f'tls_key = "{tls}/console.key"',
    # On a tailnet, the console's name there gets a certificate browsers
    # already trust, issued through tailscaled (Let's Encrypt, DNS-01).
    *([
        'trusted_via = "tailscale"',
        f'trusted_cert = "{tls}/trusted.pem"',
        f'trusted_key = "{tls}/trusted.key"',
    ] if tsname else []),
    f'database_url = "{url}"',
    "",
])

if MARK in text:
    head, rest = text.split(MARK, 1)
    lines = rest.split("\n")
    # The section runs from the marker to the next table, or to the end.
    end = next((i for i in range(2, len(lines)) if lines[i].startswith("[")), len(lines))
    tail = "\n".join(lines[end:])
    new = head + block + ("\n" + tail if tail.strip() else "")
else:
    new = text.rstrip("\n") + "\n\n" + block

if new != text:
    shutil.copy2(path, path + ".bak-" + datetime.datetime.now().strftime("%Y%m%d%H%M%S"))
    open(path, "w").write(new)
    print("changed", sans, names[0])
else:
    print("same", sans, names[0])
