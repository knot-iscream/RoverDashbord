#!/usr/bin/env python3
"""Broadcast the Pi 5 CSI camera as MJPEG on port 8081.

Serves /stream.mjpg as multipart/x-mixed-replace. Every frame is fanned out to
all connected clients, so the /control mirror and the Home camera card can
watch at the same time. A plain <img> does not need CORS headers.

Primary source is picamera2. If that module is unavailable, the script falls
back to `rpicam-vid --codec mjpeg` and splits its stdout into JPEG frames on
the SOI/EOI markers. Both paths publish the same bytes to the same hub.
"""

import io
import os
import queue
import subprocess
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

WIDTH = int(os.environ.get("CAM_WIDTH", "640"))
HEIGHT = int(os.environ.get("CAM_HEIGHT", "480"))
FPS = int(os.environ.get("CAM_FPS", "15"))
PORT = int(os.environ.get("CAM_PORT", "8081"))
BOUNDARY = "frame"


class Hub:
    """Latest-frame fan-out. A slow client drops frames instead of blocking."""

    def __init__(self):
        self.clients = set()
        self.lock = threading.Lock()

    def add(self, q):
        with self.lock:
            self.clients.add(q)

    def remove(self, q):
        with self.lock:
            self.clients.discard(q)

    def publish(self, frame):
        with self.lock:
            dead = []
            for q in self.clients:
                try:
                    q.put_nowait(frame)
                except queue.Full:
                    dead.append(q)
            for q in dead:
                self.clients.discard(q)


HUB = Hub()


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        if self.path.split("?", 1)[0] not in ("/stream.mjpg", "/"):
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", f"multipart/x-mixed-replace; boundary={BOUNDARY}")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Connection", "close")
        self.end_headers()
        q = queue.Queue(maxsize=2)
        HUB.add(q)
        try:
            while True:
                frame = q.get(timeout=15)
                self.wfile.write(b"--" + BOUNDARY.encode() + b"\r\n")
                self.wfile.write(b"Content-Type: image/jpeg\r\n")
                self.wfile.write(f"Content-Length: {len(frame)}\r\n\r\n".encode())
                self.wfile.write(frame)
                self.wfile.write(b"\r\n")
                self.wfile.flush()
        except Exception:
            pass
        finally:
            HUB.remove(q)

    def log_message(self, fmt, *args):
        pass


def picamera_loop():
    from picamera2 import Picamera2

    cam = Picamera2()
    config = cam.create_video_configuration(
        main={"size": (WIDTH, HEIGHT), "format": "RGB888"},
        controls={"FrameRate": FPS},
    )
    cam.configure(config)
    cam.start()
    try:
        while True:
            buf = io.BytesIO()
            cam.capture_file(buf, format="jpeg")
            HUB.publish(buf.getvalue())
    finally:
        cam.stop()


def rpicam_loop():
    cmd = [
        "rpicam-vid", "-t", "0", "--codec", "mjpeg", "--inline",
        "--width", str(WIDTH), "--height", str(HEIGHT),
        "--framerate", str(FPS), "-o", "-",
    ]
    proc = subprocess.Popen(
        cmd, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, bufsize=0
    )
    buf = bytearray()
    try:
        while True:
            chunk = proc.stdout.read(4096)
            if not chunk:
                break
            buf.extend(chunk)
            while True:
                start = buf.find(b"\xff\xd8")
                if start < 0:
                    # Keep a possible partial SOI at the tail.
                    if len(buf) > 1:
                        del buf[:-1]
                    break
                if start:
                    del buf[:start]
                end = buf.find(b"\xff\xd9", 2)
                if end < 0:
                    break
                end += 2
                HUB.publish(bytes(buf[:end]))
                del buf[:end]
    finally:
        proc.terminate()


def main():
    try:
        import picamera2  # noqa: F401

        source = "picamera2"
        loop = picamera_loop
    except ImportError:
        source = "rpicam-vid"
        loop = rpicam_loop

    threading.Thread(target=loop, daemon=True).start()
    server = ThreadingHTTPServer(("0.0.0.0", PORT), Handler)
    server.daemon_threads = True
    print(f"[rover-cam] {source} -> http://0.0.0.0:{PORT}/stream.mjpg", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
