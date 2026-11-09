"""Create the input files the plain-language tasks attach. Run inside the
magnus-grader image (it has the office libraries):

    docker run --rm -v $PWD/eval/plain:/e -w /e magnus-grader python3 make_fixtures.py
"""
import csv, io, os, random, zipfile

from docx import Document
from openpyxl import Workbook
from PIL import Image, ImageDraw, ImageFont
from reportlab.lib.pagesizes import letter
from reportlab.pdfgen import canvas

out = "fixtures"
os.makedirs(out, exist_ok=True)
random.seed(7)

# 1. Expenses CSV: known per-category totals.
rows = []
cats = {"Travel": [120.50, 89.99, 240.00], "Meals": [32.10, 18.75, 44.00, 27.15], "Software": [49.00, 299.00], "Office": [12.30, 65.20, 8.99]}
for c, amounts in cats.items():
    for i, a in enumerate(amounts):
        rows.append({"date": f"2026-09-{10 + i:02d}", "category": c, "description": f"{c} item {i + 1}", "amount": f"{a:.2f}"})
random.shuffle(rows)
with open(f"{out}/expenses.csv", "w", newline="") as f:
    w = csv.DictWriter(f, fieldnames=["date", "category", "description", "amount"])
    w.writeheader()
    w.writerows(rows)

# 2. Word document with a known word count.
doc = Document()
doc.add_heading("Team offsite notes", 1)
paras = [
    "We met on Tuesday to plan the spring offsite in the mountains near Lillehammer.",
    "Everyone agreed that the agenda should leave more time for open discussion and less for slides.",
    "The budget is fixed at forty thousand kroner, which covers travel, meals and the venue for two nights.",
    "Next steps are to confirm the venue by Friday and to send the invitation to the whole team next week.",
]
for p in paras:
    doc.add_paragraph(p)
doc.save(f"{out}/offsite-notes.docx")
words = sum(len(p.split()) for p in ["Team offsite notes"] + paras)
open(f"{out}/offsite-notes.wordcount", "w").write(str(words))

# 3. Receipt photo: items and prices drawn as text.
items = [("Coffee beans 1kg", 18.90), ("Oat milk x2", 5.40), ("Croissants x4", 9.60), ("Orange juice", 4.25)]
img = Image.new("RGB", (640, 520), "white")
d = ImageDraw.Draw(img)
try:
    font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 26)
except OSError:
    font = ImageFont.load_default()
y = 30
d.text((40, y), "CORNER MARKET", fill="black", font=font)
y += 60
for name, price in items:
    d.text((40, y), name, fill="black", font=font)
    d.text((470, y), f"{price:.2f}", fill="black", font=font)
    y += 50
d.line((40, y, 600, y), fill="black", width=2)
d.text((40, y + 20), "TOTAL", fill="black", font=font)
d.text((470, y + 20), f"{sum(p for _, p in items):.2f}", fill="black", font=font)
img.save(f"{out}/receipt.png")

# 4. Budget spreadsheet with wrong total formulas.
wb = Workbook()
ws = wb.active
ws.title = "Budget"
ws.append(["Item", "Q1", "Q2"])
data = [("Rent", 3000, 3000), ("Salaries", 12000, 12500), ("Marketing", 1500, 2200), ("Utilities", 400, 380), ("Insurance", 250, 250)]
for r in data:
    ws.append(list(r))
ws.append(["Total", "=SUM(B2:B4)", "=SUM(C3:C6)"])  # both wrong: should be B2:B6, C2:C6
wb.save(f"{out}/budget.xlsx")

# 5. PDF report with three known findings.
c = canvas.Canvas(f"{out}/survey-report.pdf", pagesize=letter)
text = c.beginText(60, 720)
text.setFont("Helvetica", 11)
lines = [
    "Customer Survey 2026 - Summary Report",
    "",
    "We surveyed 1,240 customers between June and August.",
    "Finding 1: Delivery speed is the top complaint; 41% rated it poor.",
    "Finding 2: Satisfaction with the mobile app rose to 78%, up from 64% last year.",
    "Finding 3: Customers who used live chat were twice as likely to buy again.",
    "",
    "Methodology: online questionnaire, 22 questions, weighted by region.",
    "Appendix tables follow on later pages in the full version.",
]
for line in lines:
    text.textLine(line)
c.drawText(text)
c.showPage()
c.save()

# 6. A zip of loosely named files with dates in their names.
buf = io.BytesIO()
with zipfile.ZipFile(f"{out}/photos.zip", "w") as z:
    for name in ["holiday 3 March 2026 beach.txt", "party 14 Feb 2026.txt", "hike 2 May 2026 summit.txt"]:
        z.writestr(name, f"placeholder for {name}\n")

print("fixtures written; offsite words =", words)
