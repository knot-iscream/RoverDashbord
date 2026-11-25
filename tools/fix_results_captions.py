#!/usr/bin/env python3
"""Move figure numbers out of image files and into Markdown captions."""

import re
from pathlib import Path

path = Path(__file__).resolve().parents[1] / "docs" / "RESULTS-DRAFT.md"
text = path.read_text(encoding="utf-8")

text = text.replace("Two-dimensional polar scan", "Isometric view of the room cloud")
text = text.replace("Three-dimensional room cloud", "Second isometric angle of the same room")

pattern = re.compile(r"!\[Figure (\d+)\. ([^\]]+)\]\(([^)]+)\)")

def repl(match):
    number, caption, target = match.groups()
    return f"![{caption}]({target})\n\n**Figure {number}.** {caption}."

text, count = pattern.subn(repl, text)
path.write_text(text, encoding="utf-8")
print(f"moved {count} captions below their images")
