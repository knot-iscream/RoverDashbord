#!/usr/bin/env python3
"""Build a lossless PDF of the results chapter from the Markdown source.

Charts are SVG, so they stay vector in the PDF. Screenshots are inserted as
the original PNG bytes by Chromium, with no re-encoding step in this script.
The output is checked with PyMuPDF for page count, blank pages, text and the
embedded raster images.

Usage:  python tools/results_pdf.py
"""
from __future__ import annotations

import hashlib
import subprocess
import tempfile
from pathlib import Path

import markdown2
import pymupdf as fitz

ROOT = Path(__file__).resolve().parents[1]
MD = ROOT / "docs" / "RESULTS-DRAFT.md"
HTML = ROOT / "docs" / ".RESULTS-DRAFT.build.html"
PDF = ROOT / "docs" / "RESULTS-DRAFT.pdf"
EDGE = Path(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe")

CSS = """@page { size: A4; margin: 13mm 12mm 14mm 12mm; }
html, body { background: #ffffff; color: #111827; }
body {
  font-family: 'Segoe UI', Arial, sans-serif;
  font-size: 9.5pt; line-height: 1.34;
}
h1 { font-size: 20pt; margin: 0 0 10pt; color: #0f172a; }
h2 {
  font-size: 14pt; margin: 17pt 0 6pt; color: #9a3412;
  border-bottom: 1px solid #d6d3d1; padding-bottom: 3pt;
  break-after: avoid; page-break-after: avoid;
}
p { margin: 5pt 0; }
table {
  border-collapse: collapse; width: 100%; margin: 7pt 0 12pt;
  font-size: 8.2pt;
}
thead { display: table-header-group; }
th, td {
  border: 1px solid #a8a29e; padding: 4pt 5pt;
  text-align: left; vertical-align: top;
  break-inside: avoid; page-break-inside: avoid;
}
th { background: #f1f5f9; }
tr { break-inside: avoid; page-break-inside: avoid; }
code { font-family: Consolas, monospace; background: #f8fafc; }
img {
  display: block; max-width: 100%; height: auto;
  margin: 7pt auto 2pt; break-inside: avoid; page-break-inside: avoid;
}
strong { color: #9a3412; }
"""

NEEDLES = [
    "Results", "Figure 1.", "Figure 36.", "Disclosure",
    "Isometric view of the room", "Filtered occupancy map",
    "no navigation stack was built",
]


def main() -> None:
    if not MD.exists():
        raise SystemExit(f"missing {MD}")
    if not EDGE.exists():
        raise SystemExit(f"Edge not found at {EDGE}")

    body = markdown2.markdown(
        MD.read_text(encoding="utf-8"),
        extras=["tables", "fenced-code-blocks"],
    )
    HTML.write_text(
        '<!doctype html><html><head><meta charset="utf-8">'
        f"<style>{CSS}</style></head><body>{body}</body></html>",
        encoding="utf-8",
    )

    if PDF.exists():
        PDF.unlink()
    with tempfile.TemporaryDirectory(prefix="rover-results-pdf-") as profile:
        cmd = [
            str(EDGE), "--headless", "--disable-gpu", "--no-first-run",
            f"--user-data-dir={profile}", "--no-pdf-header-footer",
            f"--print-to-pdf={PDF}", HTML.as_uri(),
        ]
        result = subprocess.run(cmd, capture_output=True, timeout=300)
    HTML.unlink(missing_ok=True)
    if result.returncode != 0 or not PDF.exists():
        raise SystemExit(
            "Edge print failed: " + result.stderr.decode("utf-8", "replace")[:500]
        )

    # Chromium resamples and recompresses raster images during printing.
    # Put the original PNG bytes back so the screenshots stay lossless.
    source_pngs = sorted((ROOT / "docs" / "results-figures").glob("*.png"))
    doc = fitz.open(PDF)
    replaced = 0
    for page in doc:
        for image in page.get_images(full=True):
            if replaced < len(source_pngs):
                page.replace_image(image[0], filename=str(source_pngs[replaced]))
                replaced += 1
    if replaced:
        compressed = PDF.with_suffix(".tmp.pdf")
        doc.save(compressed, garbage=4, deflate=True)
        doc.close()
        compressed.replace(PDF)
    else:
        doc.close()

    doc = fitz.open(PDF)
    pages = len(doc)
    text = "\n".join(page.get_text() for page in doc)
    folded = text.casefold()
    missing = [s for s in NEEDLES if s.casefold() not in folded]
    blank = [i + 1 for i, page in enumerate(doc) if len(page.get_text().strip()) < 20]

    png_paths = sorted((ROOT / "docs" / "results-figures").glob("*.png"))
    source_pngs = {
        hashlib.sha256(p.read_bytes()).hexdigest(): p.name for p in png_paths
    }
    expected, source_pixels = {}, {}
    for p in png_paths:
        pix = fitz.Pixmap(str(p))
        expected[p.name] = f"{pix.width}x{pix.height}"
        fingerprint = hashlib.sha256(
            f"{pix.n}:{pix.alpha}:".encode() + pix.samples
        ).hexdigest()
        source_pixels[fingerprint] = p.name

    embedded, pixel_matched = {}, []
    for page in doc:
        for image in page.get_images(full=True):
            xref = image[0]
            info = doc.extract_image(xref)
            embedded[hashlib.sha256(info["image"]).hexdigest()] = (
                f"{info['width']}x{info['height']} {info['ext']}"
            )
            pix = fitz.Pixmap(doc, xref)
            fingerprint = hashlib.sha256(
                f"{pix.n}:{pix.alpha}:".encode() + pix.samples
            ).hexdigest()
            if fingerprint in source_pixels:
                pixel_matched.append(source_pixels[fingerprint])
    doc.close()

    if missing:
        raise SystemExit(f"PDF missing expected text: {missing}")
    if blank:
        raise SystemExit(f"PDF has blank pages: {blank}")

    matched = sorted(set(pixel_matched))
    print(f"wrote {PDF.relative_to(ROOT)} ({pages} pages, {PDF.stat().st_size:,} bytes)")
    print(f"embedded raster images: {len(embedded)}, pixel-identical screenshots: {len(matched)}/{len(source_pngs)}")
    if len(matched) != len(source_pngs):
        raise SystemExit("not all screenshots survived the PDF layout unchanged")
    print("source sizes: " + ", ".join(f"{k}={v}" for k, v in expected.items()))
    print("pdf sizes: " + ", ".join(embedded.values()))


if __name__ == "__main__":
    main()
