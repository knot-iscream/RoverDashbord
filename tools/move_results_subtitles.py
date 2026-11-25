#!/usr/bin/env python3
"""Put the figure subtitles into the Markdown, not inside the SVG files."""

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOC = ROOT / "docs" / "RESULTS-DRAFT.md"
SUBS = ROOT / "docs" / "results-figures" / "subtitles.json"

subtitles = json.loads(SUBS.read_text(encoding="utf-8"))
text = DOC.read_text(encoding="utf-8")
lines = text.splitlines()
out = []
inserted = 0
caption = re.compile(r"^\*\*Figure (\d+)\.\*\*\s+")

for i, line in enumerate(lines):
    out.append(line)
    match = caption.match(line)
    if not match:
        continue
    number = match.group(1)
    subtitle = subtitles.get(number)
    if not subtitle:
        continue
    following = lines[i + 1] if i + 1 < len(lines) else ""
    if following.strip() != subtitle:
        out.append(subtitle)
        inserted += 1

DOC.write_text("\n".join(out) + "\n", encoding="utf-8")
print(f"inserted {inserted} subtitles into the chapter text")
