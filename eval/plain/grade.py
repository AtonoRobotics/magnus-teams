"""Graders for the plain-language suite. They check the world (the files the
worker delivered, facts fetched independently), never the worker's report.

    python3 grade.py <task-id> <result-dir>

<result-dir> holds task.json (the task view), inbox.json (the task's inbox
items), schedules.json and outputs/ (delivered files). Prints one JSON object:
{"score": 0..1, "passed": bool, "checks": [[name, ok, detail], ...]}.
"""
import glob, json, os, re, subprocess, sys, tempfile, urllib.request, zipfile

tid, rdir = sys.argv[1], sys.argv[2]
task = json.load(open(os.path.join(rdir, "task.json")))
inbox = json.load(open(os.path.join(rdir, "inbox.json")))
schedules = json.load(open(os.path.join(rdir, "schedules.json")))
outs = os.path.join(rdir, "outputs")
summary = (task.get("summary") or "")
checks = []


def check(name, ok, detail=""):
    checks.append([name, bool(ok), str(detail)[:300]])
    return bool(ok)


def files(*exts):
    return [f for f in glob.glob(os.path.join(outs, "*")) if f.lower().endswith(exts)]


def text_of(path):
    if path.lower().endswith(".pdf"):
        return subprocess.run(["pdftotext", "-layout", path, "-"], capture_output=True, text=True).stdout
    if path.lower().endswith(".docx"):
        from docx import Document
        return "\n".join(p.text for p in Document(path).paragraphs)
    if path.lower().endswith(".pptx"):
        from pptx import Presentation
        return "\n".join(sh.text_frame.text for s in Presentation(path).slides for sh in s.shapes if sh.has_text_frame)
    return open(path, errors="ignore").read()


def recalc(path):
    """Values of a workbook after LibreOffice recalculates its formulas."""
    d = tempfile.mkdtemp()
    subprocess.run(["soffice", "-env:UserInstallation=file:///tmp/lo-profile", "--headless", "--convert-to", "xlsx", "--outdir", d, path], capture_output=True, timeout=120)
    from openpyxl import load_workbook
    out = os.path.join(d, os.path.basename(path))
    return load_workbook(out if os.path.exists(out) else path, data_only=True)


def cells(wb):
    for ws in wb.worksheets:
        for row in ws.iter_rows(values_only=True):
            yield ws.title, row


def num(v):
    if isinstance(v, (int, float)):
        return float(v)
    if isinstance(v, str):
        m = re.search(r"-?[\d,]*\.?\d+", v.replace(" ", ""))
        if m:
            try:
                return float(m.group().replace(",", ""))
            except ValueError:
                return None
    return None


completed = check("task completed", task.get("status") == "completed", task.get("status"))

if tid == "planets-sheet":
    xs = files(".xlsx", ".csv")
    if check("spreadsheet delivered", xs, [os.path.basename(x) for x in xs]):
        rows = list(cells(recalc(xs[0]))) if xs[0].endswith(".xlsx") else [("csv", r) for r in __import__("csv").reader(open(xs[0]))]
        names = [str(r[0]).strip().lower() for _, r in rows if r and r[0]]
        order = ["mercury", "venus", "earth", "mars", "jupiter", "saturn", "uranus", "neptune"]
        found = [n for n in names if n in order]
        check("eight planets in order of distance", found == order, found)
        widths = {}
        for _, r in rows:
            if r and str(r[0]).strip().lower() in order:
                nums = [num(v) for v in r[1:] if num(v) is not None]
                if len(nums) >= 2:
                    widths[str(r[0]).strip().lower()] = nums
        # the width column: the one whose Jupiter value is the largest among the planets
        total_row = next((r for _, r in rows if r and str(r[0]).strip().lower().startswith("total")), None)
        ok = False
        if total_row and len(widths) == 8:
            for col in range(len(widths["jupiter"])):
                s = sum(v[col] for v in widths.values() if len(v) > col)
                tvals = [num(v) for v in total_row[1:] if num(v) is not None]
                if any(abs(t - s) <= max(1.0, 0.001 * s) for t in tvals):
                    ok = True
        check("total row equals the sum of widths", ok, total_row)
        j = widths.get("jupiter", [])
        check("Jupiter about 139,800 km wide (or ~86,900 mi)", any(130000 < v < 145000 or 85000 < v < 90000 for v in j), j)

elif tid == "expenses-summary":
    xs = files(".xlsx")
    if check("Excel file delivered", xs):
        from openpyxl import load_workbook
        wb_formulas = load_workbook(xs[0])
        wb = recalc(xs[0])
        want = {"travel": 450.49, "meals": 122.00, "software": 348.00, "office": 86.49}
        got = {}
        for _, r in cells(wb):
            if r and isinstance(r[0], str) and r[0].strip().lower() in want:
                for v in r[1:]:
                    if num(v) is not None:
                        got[r[0].strip().lower()] = num(v)
        check("a total per category, all correct", all(abs(got.get(k, -1) - v) < 0.02 for k, v in want.items()), got)
        check("more than one sheet (a summary sheet)", len(wb.sheetnames) >= 2, wb.sheetnames)
        check("a chart in the workbook", any(ws._charts for ws in wb_formulas.worksheets), [len(ws._charts) for ws in wb_formulas.worksheets])

elif tid == "landlord-letter":
    ps = files(".pdf")
    if check("PDF delivered", ps):
        n = int(re.search(r"Pages:\s+(\d+)", subprocess.run(["pdfinfo", ps[0]], capture_output=True, text=True).stdout).group(1))
        t = text_of(ps[0]).lower()
        check("one page", n == 1, n)
        check("names Dana Reyes and the address", "dana reyes" in t and "12 elm" in t, t[:200])
        check("asks to fix the heating", "heat" in t, "")
        check("mentions two weeks", "two weeks" in t or "2 weeks" in t, "")

elif tid == "bakery-slides":
    ps = files(".pptx")
    if check("slide deck delivered", ps):
        from pptx import Presentation
        prs = Presentation(ps[0])
        t = text_of(ps[0]).lower()
        check("five slides", len(prs.slides) == 5, len(prs.slides))
        check("price $6.50", "6.50" in t, "")
        check("ingredients listed", all(w in t for w in ["flour", "water", "salt", "starter"]), "")
        check("where to buy", "main street" in t and "market" in t, "")

elif tid == "docx-to-pdf":
    ps = files(".pdf")
    want = int(open("/fixtures/offsite-notes.wordcount").read())
    if check("PDF delivered", ps):
        check("PDF has the document's text", "lillehammer" in text_of(ps[0]).lower(), "")
    nums = [int(n) for n in re.findall(r"\b(\d{2,3})\b", summary)]
    check(f"word count close to {want} (within 5%)", any(abs(n - want) <= max(2, 0.05 * want) for n in nums), nums)

elif tid == "receipt-to-sheet":
    xs = files(".xlsx", ".csv")
    if check("spreadsheet delivered", xs):
        rows = list(cells(recalc(xs[0]))) if xs[0].endswith(".xlsx") else [("csv", r) for r in __import__("csv").reader(open(xs[0]))]
        vals = [num(v) for _, r in rows for v in r if num(v) is not None]
        check("all four prices", all(any(abs(v - p) < 0.005 for v in vals) for p in [18.90, 5.40, 9.60, 4.25]), vals)
        check("total 38.15", any(abs(v - 38.15) < 0.005 for v in vals), "")

elif tid == "fix-budget":
    xs = files(".xlsx")
    if check("spreadsheet delivered", xs):
        wb = recalc(xs[0])
        totals = [r for _, r in cells(wb) if r and str(r[0]).strip().lower() == "total"]
        ok = bool(totals) and abs(num(totals[0][1]) - 17150) < 0.5 and abs(num(totals[0][2]) - 18330) < 0.5
        check("Q1 total 17,150 and Q2 total 18,330", ok, totals)

elif tid == "survey-findings":
    s = summary.lower()
    check("delivery speed is the top complaint", "deliver" in s and ("41" in s or "complain" in s), "")
    check("app satisfaction rose to 78%", "78" in s, "")
    check("live chat users twice as likely to buy again", "chat" in s and ("twice" in s or "2x" in s or "two times" in s or "double" in s), "")

elif tid == "rename-zip":
    zs = files(".zip")
    if check("zip delivered", zs):
        names = sorted(os.path.basename(n) for n in zipfile.ZipFile(zs[0]).namelist() if not n.endswith("/"))
        want = ["2026-02-14_", "2026-03-03_", "2026-05-02_"]
        check("three files, each starting with its YYYY-MM-DD date and an underscore", len(names) == 3 and all(any(n.startswith(w) for n in names) for w in want), names)

elif tid == "sales-chart":
    ps = files(".png", ".jpg", ".jpeg", ".svg", ".pdf")
    if check("chart image delivered", ps):
        if ps[0].endswith((".png", ".jpg", ".jpeg")):
            from PIL import Image
            im = Image.open(ps[0])
            check("image has a reasonable size", im.width >= 300 and im.height >= 200, im.size)

elif tid == "closure-email":
    s = summary.lower()
    check("says the office is closed Friday", "friday" in s and "clos" in s, "")
    check("tells people to work from home", "home" in s or "remote" in s, "")
    check("nothing was sent (no approval asked to send)", not [i for i in inbox if i["kind"] == "approval"], "")

elif tid == "firecracker-version":
    rel = json.load(urllib.request.urlopen(urllib.request.Request("https://api.github.com/repos/firecracker-microvm/firecracker/releases/latest", headers={"User-Agent": "magnus-eval"}), timeout=30))
    tag = rel["tag_name"].lstrip("v")
    check(f"names the latest release ({tag})", tag in summary, summary[:200])
    check("gives a link", "http" in summary, "")

elif tid == "nordic-populations":
    s = summary.lower()
    check("says Sweden is largest", "sweden" in s and ("largest" in s or "most" in s or "biggest" in s), "")
    xs = files(".xlsx", ".csv", ".md", ".pdf", ".docx")
    if check("table delivered", xs):
        t = " ".join(str(v) for _, r in (cells(recalc(xs[0])) if xs[0].endswith(".xlsx") else []) for v in r) or text_of(xs[0])
        tl = t.lower()
        check("all five countries", all(c in tl for c in ["norway", "sweden", "denmark", "finland", "iceland"]), "")

elif tid == "weekday-reminder":
    props = [i for i in inbox if i["kind"] == "schedule_proposal"]
    made = props or schedules
    check("a recurring task was proposed or set", made, [i.get("details") for i in props])
    d = (props[0]["details"] if props else (schedules[0] if schedules else {}))
    check("daily at 09:00", d.get("daily_at") in ("09:00", "9:00"), d)

score = sum(1 for c in checks if c[1]) / len(checks) if checks else 0.0
print(json.dumps({"score": round(score, 3), "passed": all(c[1] for c in checks), "checks": checks}))
