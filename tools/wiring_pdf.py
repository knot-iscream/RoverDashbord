#!/usr/bin/env python3
"""Build docs/WIRING.pdf from docs/WIRING.md.

Uses markdown2 for the HTML conversion and headless Edge for the print step.
No extra installs: both are already present on this PC. The resulting PDF is
checked with PyMuPDF (fitz) for page count, blank pages and a few key strings.

Usage:  python tools/wiring_pdf.py
"""
from __future__ import annotations

import subprocess
import tempfile
from pathlib import Path

import markdown2
import pymupdf as fitz

ROOT = Path(__file__).resolve().parents[1]
MD = ROOT / "docs" / "WIRING.md"
HTML = ROOT / "docs" / ".WIRING.build.html"
PDF = ROOT / "docs" / "WIRING.pdf"
EDGE = Path(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe")

CSS = """@page { size: A4; margin: 15mm 14mm 16mm 14mm; }
html, body { background: #ffffff; color: #111827; }
body {
  font-family: Consolas, 'DejaVu Sans Mono', monospace;
  font-size: 11pt; line-height: 1.4;
}
h1 { font-size: 21pt; margin: 0 0 12pt; color: #0f172a; }
h2 {
  font-size: 15pt; margin: 18pt 0 7pt; color: #9a3412;
  border-bottom: 1px solid #d6d3d1; padding-bottom: 3pt;
  page-break-after: avoid;
}
p { margin: 6pt 0; }
table {
  border-collapse: collapse; width: 100%; margin: 8pt 0 14pt;
  font-size: 9.6pt; page-break-inside: auto;
}
thead { display: table-header-group; }
th, td {
  border: 1px solid #a8a29e; padding: 5pt 7pt;
  text-align: left; vertical-align: top;
  page-break-inside: avoid;
}
th { background: #f1f5f9; }
tr { page-break-inside: avoid; }
code, pre { font-family: Consolas, 'DejaVu Sans Mono', monospace; }
code { background: #f8fafc; }
pre {
  background: #f8fafc; border: 1px solid #d6d3d1; padding: 6pt;
  white-space: pre-wrap; font-size: 8.5pt; page-break-inside: avoid;
}
ul, ol { margin: 4pt 0 9pt; padding-left: 18pt; }
li { margin: 2pt 0; page-break-inside: avoid; }
strong { color: #9a3412; }
a { color: #b45309; }
hr { border: 0; border-top: 1px solid #d6d3d1; margin: 11pt 0; }
"""

NEEDLES = [
    "IN1", "0x40", "0x70", "DS18B20",
    "Not connected on purpose", "10 k", "0.1",
    "IRLZ44N", "GPIO 25", "R100",
]


def main() -> None:
    if not MD.exists():
        raise SystemExit(f"missing {MD}")
    if not EDGE.exists():
        raise SystemExit(f"Edge not found at {EDGE}")

    body = markdown2.markdown(
        MD.read_text(encoding="utf-8"),
        extras=["tables", "fenced-code-blocks", "break-on-newline"],
    )
    HTML.write_text(
        '<!doctype html><html><head><meta charset="utf-8">'
        f"<style>{CSS}</style></head><body>{body}</body></html>",
        encoding="utf-8",
    )

    if PDF.exists():
        PDF.unlink()
    with tempfile.TemporaryDirectory(prefix="rover-pdf-") as profile:
        cmd = [
            str(EDGE), "--headless", "--disable-gpu", "--no-first-run",
            f"--user-data-dir={profile}", "--no-pdf-header-footer",
            f"--print-to-pdf={PDF}", HTML.as_uri(),
        ]
        r = subprocess.run(cmd, capture_output=True, timeout=180)
    HTML.unlink(missing_ok=True)
    if r.returncode != 0 or not PDF.exists():
        raise SystemExit(
            "Edge print failed: " + r.stderr.decode("utf-8", "replace")[:500]
        )

    doc = fitz.open(PDF)
    pages = len(doc)
    all_text = "\n".join(page.get_text() for page in doc)
    folded = all_text.casefold()
    missing = [s for s in NEEDLES if s.casefold() not in folded]
    blank = [i + 1 for i, page in enumerate(doc) if len(page.get_text().strip()) < 20]
    doc.close()
    if missing:
        raise SystemExit(f"PDF missing expected text: {missing}")
    if blank:
        raise SystemExit(f"PDF has blank pages: {blank}")
    print(f"wrote {PDF.relative_to(ROOT)} ({pages} pages, {PDF.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
