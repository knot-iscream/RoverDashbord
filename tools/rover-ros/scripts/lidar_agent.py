#!/usr/bin/env python3
"""Fixed-command LiDAR service agent for the Pi.

The app cannot start Docker containers by itself. This agent exposes a narrow
MQTT command surface:

  rover/lidar/service/command  {"action":"start|stop|clear_map|status","token":"..."}
  rover/lidar/service/status   retained JSON state

It only runs the fixed commands below. It is not a general shell.
"""
import glob
import http.server
import io
import json
import os
import re
import shutil
import socket
import subprocess
import threading
import time
import urllib.parse
import zipfile

import paho.mqtt.client as mqtt

BROKER = "127.0.0.1"
PORT = 1883
CMD_TOPIC = "rover/lidar/service/command"
STATUS_TOPIC = "rover/lidar/service/status"
CONTAINER = "rover-ros"
TOKEN_FILE = "/home/iscream-pi/rover-ros/agent.token"
MAPPING_LOOP = "/home/iscream-pi/rover-ros/scripts/sweep_loop.py"
LIDAR_OFF = "/home/iscream-pi/rover-ros/scripts/lidar_off.py"
HOLD_SCRIPT = "/home/iscream-pi/rover-ros/scripts/lidar_hold.py"
HOLD_LOG = "/home/iscream-pi/rover-ros/maps/lidar_hold.log"
SCANS_DIR = "/home/iscream-pi/rover-ros/maps/scans"
HTTP_PORT = 8082
SCAN_ID = re.compile(r"^[A-Za-z0-9_-]+$")
FILE_NAME = re.compile(r"^[A-Za-z0-9_.-]+$")

lock = threading.Lock()
client = None


def read_token():
    try:
        with open(TOKEN_FILE) as f:
            return f.read().strip()
    except OSError:
        return ""


def run(cmd, timeout=60):
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)


def container_running():
    try:
        out = run(["docker", "inspect", "-f", "{{.State.Running}}", CONTAINER], timeout=10)
        return out.returncode == 0 and out.stdout.strip() == "true"
    except (OSError, subprocess.SubprocessError):
        return False


def mapping_running():
    try:
        out = run(
            ["pgrep", "-f", "^python3 /home/iscream-pi/rover-ros/scripts/sweep_loop.py$"],
            timeout=5,
        )
        return out.returncode == 0 and bool(out.stdout.strip())
    except (OSError, subprocess.SubprocessError):
        return False


def lan_ip():
    try:
        out = run(["hostname", "-I"], timeout=5)
        for ip in out.stdout.split():
            if ip and not ip.startswith("127."):
                return ip
    except (OSError, subprocess.SubprocessError):
        pass
    try:
        return socket.gethostbyname(socket.gethostname())
    except OSError:
        return ""


def http_url():
    ip = lan_ip()
    return "http://%s:%d" % (ip, HTTP_PORT) if ip else ""


def scan_list():
    out = []
    if not os.path.isdir(SCANS_DIR):
        return out
    for name in sorted(os.listdir(SCANS_DIR), reverse=True):
        path = os.path.join(SCANS_DIR, name)
        meta_path = os.path.join(path, "meta.json")
        if not os.path.isdir(path) or not os.path.isfile(meta_path):
            continue
        try:
            with open(meta_path, encoding="utf-8") as f:
                meta = json.load(f)
        except (OSError, ValueError):
            continue
        total = 0
        for entry in os.listdir(path):
            fp = os.path.join(path, entry)
            if os.path.isfile(fp):
                total += os.path.getsize(fp)
        meta["size"] = total
        out.append(meta)
    return out


def scan_zip(scan_id):
    path = os.path.join(SCANS_DIR, scan_id)
    if not SCAN_ID.match(scan_id) or not os.path.isdir(path):
        return None
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        for name in ("meta.json", "display.json", "cloud.ply", "map.bt"):
            fp = os.path.join(path, name)
            if os.path.isfile(fp):
                z.write(fp, name)
    return buf.getvalue()


class ScanHandler(http.server.BaseHTTPRequestHandler):
    """Tiny read-only file service for the laptop scan archive."""

    def log_message(self, _fmt, *args):
        pass

    def _authorized(self):
        token = read_token()
        header = self.headers.get("Authorization", "")
        return bool(token) and header == "Bearer " + token

    def _json(self, code, body):
        data = json.dumps(body).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def _bytes(self, code, data, content_type, filename=""):
        self.send_response(code)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        if filename:
            self.send_header("Content-Disposition", 'attachment; filename="%s"' % filename)
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if not self._authorized():
            self._json(401, {"error": "unauthorized"})
            return
        path = urllib.parse.urlparse(self.path).path
        if path == "/scans":
            self._json(200, {"scans": scan_list()})
            return
        match = re.match(r"^/scans/([A-Za-z0-9_-]+)\.zip$", path)
        if match:
            data = scan_zip(match.group(1))
            if data is None:
                self._json(404, {"error": "not found"})
            else:
                self._bytes(200, data, "application/zip", match.group(1) + ".zip")
            return
        match = re.match(r"^/scans/([A-Za-z0-9_-]+)/([A-Za-z0-9_.-]+)$", path)
        if match:
            scan_id, name = match.group(1), match.group(2)
            fp = os.path.realpath(os.path.join(SCANS_DIR, scan_id, name))
            if not FILE_NAME.match(name) or not fp.startswith(os.path.realpath(SCANS_DIR)):
                self._json(400, {"error": "bad path"})
                return
            if not os.path.isfile(fp):
                self._json(404, {"error": "not found"})
                return
            with open(fp, "rb") as f:
                self._bytes(200, f.read(), "application/octet-stream", name)
            return
        self._json(404, {"error": "not found"})

    def do_DELETE(self):
        if not self._authorized():
            self._json(401, {"error": "unauthorized"})
            return
        match = re.match(r"^/scans/([A-Za-z0-9_-]+)$", urllib.parse.urlparse(self.path).path)
        if not match:
            self._json(404, {"error": "not found"})
            return
        path = os.path.realpath(os.path.join(SCANS_DIR, match.group(1)))
        if not path.startswith(os.path.realpath(SCANS_DIR)) or path == os.path.realpath(SCANS_DIR):
            self._json(400, {"error": "bad path"})
            return
        if not os.path.isdir(path):
            self._json(404, {"error": "not found"})
            return
        shutil.rmtree(path, ignore_errors=True)
        self._json(200, {"status": "deleted"})


def serve_http():
    try:
        httpd = http.server.ThreadingHTTPServer(("0.0.0.0", HTTP_PORT), ScanHandler)
        httpd.daemon_threads = True
        httpd.serve_forever()
    except OSError as exc:
        print("scan http failed: %s" % exc, flush=True)


def publish_status(message=""):
    usb = sorted(glob.glob("/dev/ttyUSB*"))
    payload = {
        "container": "running" if container_running() else "stopped",
        "mapping": mapping_running(),
        "hold": hold_running(),
        "usb": bool(usb),
        "ttyusb": usb,
        "http_url": http_url(),
        "message": message,
    }
    client.publish(STATUS_TOPIC, json.dumps(payload), retain=True)


def start_mapping():
    if mapping_running():
        return
    subprocess.Popen(
        ["python3", MAPPING_LOOP],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )


def stop_mapping():
    run(["pkill", "-f", "^python3 /home/iscream-pi/rover-ros/scripts/sweep_loop.py$"], timeout=10)
    for _ in range(120):
        if not mapping_running():
            return
        time.sleep(0.5)


def hold_running():
    try:
        out = run(
            ["pgrep", "-f", "^python3 /home/iscream-pi/rover-ros/scripts/lidar_hold.py$"],
            timeout=5,
        )
        return out.returncode == 0 and bool(out.stdout.strip())
    except (OSError, subprocess.SubprocessError):
        return False


def start_hold():
    """Keep the port open with PWM 0, so the clone cannot float DTR on."""
    if hold_running():
        return
    try:
        with open(HOLD_LOG, "a", encoding="utf-8") as log:
            subprocess.Popen(
                ["python3", HOLD_SCRIPT],
                stdout=log,
                stderr=log,
                start_new_session=True,
            )
    except OSError as exc:
        print("hold start failed: %s" % exc, flush=True)
        return
    time.sleep(1.0)


def stop_hold():
    if not hold_running():
        return
    run(
        ["pkill", "-f", "^python3 /home/iscream-pi/rover-ros/scripts/lidar_hold.py$"],
        timeout=10,
    )
    for _ in range(20):
        if not hold_running():
            return
        time.sleep(0.25)


def handle(action):
    with lock:
        try:
            if action == "start":
                publish_status("starting container")
                stop_hold()
                if not container_running():
                    run(["docker", "start", CONTAINER], timeout=90)
                time.sleep(3)
                start_mapping()
                publish_status("scan running")
            elif action == "stop":
                publish_status("stopping scan")
                stop_mapping()
                stop_hold()
                if container_running():
                    run(["docker", "stop", CONTAINER], timeout=90)
                run(["python3", LIDAR_OFF], timeout=20)
                start_hold()
                publish_status("scan stopped")
            elif action == "clear_map":
                publish_status("clearing map")
                if container_running():
                    reset = run(
                        [
                            "docker", "exec", CONTAINER, "bash", "-c",
                            "source /opt/ros/noetic/setup.bash; "
                            "rosservice call /octomap_server/reset 2>&1",
                        ],
                        timeout=30,
                    )
                    if reset.returncode != 0:
                        # Fallback: a restart always starts with an empty map.
                        run(["docker", "restart", CONTAINER], timeout=120)
                client.publish("rover/lidar/cloud/clear", "{}")
                publish_status("map cleared")
            else:  # status
                publish_status("")
        except (OSError, subprocess.SubprocessError) as exc:
            publish_status("error: %s" % exc)


def on_connect(c, _userdata, _flags, _rc, _props=None):
    c.subscribe(CMD_TOPIC)
    publish_status("agent online")


def on_message(c, _userdata, msg):
    try:
        body = json.loads(msg.payload.decode("utf-8", "replace"))
    except (ValueError, UnicodeDecodeError):
        return
    token = read_token()
    if not token or body.get("token") != token:
        # Keep the full retained state, add the warning as the message.
        threading.Thread(
            target=publish_status, args=("unauthorized command",), daemon=True
        ).start()
        return
    action = body.get("action", "status")
    threading.Thread(target=handle, args=(action,), daemon=True).start()


def main():
    global client
    os.makedirs(SCANS_DIR, exist_ok=True)
    threading.Thread(target=serve_http, daemon=True).start()
    # Pi boot or agent restart with the lidar service stopped: keep the motor
    # off until the app starts a scan.
    if not container_running():
        start_hold()
    client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id="rover-lidar-agent")
    client.on_connect = on_connect
    client.on_message = on_message
    client.connect(BROKER, PORT, 30)
    client.loop_forever()


if __name__ == "__main__":
    main()
