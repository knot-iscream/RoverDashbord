#!/usr/bin/env python3
"""Render a few result figures to PNG for visual inspection."""

import subprocess
from pathlib import Path

EDGE = "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"
ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "docs" / "results-figures"
OUT = Path("C:/Users/iscream/AppData/Local/Temp/opencode")
OUT.mkdir(parents=True, exist_ok=True)

for name in ["fig19-architecture.svg", "fig30-flame.svg"]:
    url = (SRC / name).as_uri()
    dest = OUT / (name + ".png")
    subprocess.run(
        [EDGE, "--headless=new", "--disable-gpu", "--hide-scrollbars",
         "--window-size=1000,900", f"--screenshot={dest}", url],
        capture_output=True, timeout=90,
    )
    print(dest, dest.exists())
