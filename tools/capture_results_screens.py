#!/usr/bin/env python3
"""Capture dashboard screenshots for the results chapter with live data.

Starts the bundled broker, the backend and the throwaway publisher if they
are not already running, then drives headless Edge through the DevTools
Protocol. Before each shot it clicks a motor tab when the page still shows
the placeholder text, which forces the imperative paint path to run.
"""

import base64
import json
import os
import socket
import subprocess
import tempfile
import time
import urllib.request
from pathlib import Path

from websockets.sync.client import connect

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "docs" / "results-figures"
EDGE = "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"
BIN = ROOT / "desktop" / "binaries"
DEBUG_PORT = 9231
PAGES = [
    ("fig32-home.png", "http://127.0.0.1:8000/"),
    ("fig33-detailed.png", "http://127.0.0.1:8000/detailed/"),
    ("fig34-control.png", "http://127.0.0.1:8000/control/"),
]
started = []


def listening(port):
    s = socket.socket()
    rc = s.connect_ex(("127.0.0.1", port))
    s.close()
    return rc == 0


def launch(exe, args=(), cwd=None, env=None):
    proc = subprocess.Popen(
        [str(exe), *args], cwd=str(cwd) if cwd else None,
        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        creationflags=subprocess.DETACHED_PROCESS,
    )
    started.append(proc)
    return proc


def get_json(url):
    with urllib.request.urlopen(url, timeout=2) as r:
        return json.loads(r.read())


def wait_for_env(seconds=40):
    end = time.time() + seconds
    while time.time() < end:
        try:
            data = get_json("http://127.0.0.1:8000/api/env")
            if data.get("env"):
                return True
        except Exception:
            pass
        time.sleep(0.5)
    return False


# services -----------------------------------------------------------------
if not listening(1883):
    launch(BIN / "mosquitto-x86_64-pc-windows-msvc.exe", ["-c", "mosquitto.conf"], BIN)
    time.sleep(2)

if not listening(8000):
    launch(ROOT / "target" / "debug" / "digital-twin-dashboard.exe", cwd=ROOT)
    for _ in range(40):
        if listening(8000):
            break
        time.sleep(0.5)

cfg = get_json("http://127.0.0.1:8000/api/config")
env = os.environ.copy()
env["MQTT_BROKER"] = cfg["mqtt_broker"]
env["MQTT_PORT"] = str(cfg["mqtt_port"])
publisher = ROOT / "target" / "debug" / "examples" / "fake_telemetry.exe"
if publisher.exists() and not any("fake_telemetry" in (p.args[0] if p.args else "") for p in started):
    launch(publisher, cwd=ROOT, env=env)

if not wait_for_env():
    raise SystemExit("no live telemetry reached the backend")

# browser ------------------------------------------------------------------
profile = tempfile.mkdtemp(prefix="edge-results-")
edge = subprocess.Popen(
    [EDGE, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--no-first-run",
     "--remote-allow-origins=*", f"--remote-debugging-port={DEBUG_PORT}",
     f"--user-data-dir={profile}", "--window-size=1400,900", "about:blank"],
    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
)
started.append(edge)

target = None
for _ in range(60):
    try:
        pages = get_json(f"http://127.0.0.1:{DEBUG_PORT}/json/list")
        target = next((p for p in pages if p.get("type") == "page"), None)
        if target:
            break
    except Exception:
        pass
    time.sleep(0.5)
if not target:
    raise SystemExit("Edge DevTools did not come up")

ws = connect(target["webSocketDebuggerUrl"], max_size=20_000_000, open_timeout=20)
seq = 0


def cmd(method, params=None):
    global seq
    seq += 1
    mine = seq
    ws.send(json.dumps({"id": mine, "method": method, "params": params or {}}))
    while True:
        msg = json.loads(ws.recv())
        if msg.get("id") == mine:
            return msg


def evaluate(expression):
    res = cmd("Runtime.evaluate", {"expression": expression, "returnByValue": True})
    return res.get("result", {}).get("result", {}).get("value")


cmd("Page.enable")
cmd("Runtime.enable")

for name, url in PAGES:
    cmd("Page.navigate", {"url": url})
    time.sleep(3.5)
    waiting = evaluate("document.body.innerText.includes('Waiting for live data')")
    if waiting:
        evaluate("const b=document.querySelector('.detail-motor-tab[data-motor=\"1\"]'); if(b) b.click();")
        time.sleep(2)
        evaluate("const b=document.querySelector('.detail-motor-tab[data-motor=\"2\"]'); if(b) b.click();")
        time.sleep(1.5)
    shot = cmd("Page.captureScreenshot", {"format": "png", "captureBeyondViewport": False})
    data = shot.get("result", {}).get("data")
    if data:
        (OUT / name).write_bytes(base64.b64decode(data))
    print(name, "ok" if data else "capture failed", "repaint" if waiting else "clean")

ws.close()
for proc in reversed(started):
    try:
        proc.terminate()
    except Exception:
        pass
print("done")
