#!/usr/bin/env python3
"""Broadcast the Pi 5 CSI camera as MJPEG on port 8081.

Serves /stream.mjpg as multipart/x-mixed-replace. Every frame is fanned out to
all connected clients, so the /control mirror and the Home camera card can
watch at the same time. A plain <img> does not need CORS headers.

Two quality profiles are built in. `watch` trades frame rate for light, noise
and colour, which suits a parked rover. `drive` keeps 15 fps for a responsive
feed. CAM_PROFILE picks one and any single value can be overridden with its own
CAM_* environment variable, so a bench test never needs a code edit.

Primary source is picamera2. If that module is unavailable, the script falls
back to `rpicam-vid --codec mjpeg` and splits its stdout into JPEG frames on
the SOI/EOI markers. Both paths publish the same bytes to the same hub.
"""

import io
import json
import os
import queue
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

WIDTH = int(os.environ.get("CAM_WIDTH", "640"))
HEIGHT = int(os.environ.get("CAM_HEIGHT", "480"))
PORT = int(os.environ.get("CAM_PORT", "8081"))
BOUNDARY = "frame"

# Lower frame rate means a longer maximum exposure, which is the only way this
# sensor gets more light without more noise. Values below are starting points;
# tools/rover-cam/measure.py is what actually picks the winners.
PROFILES = {
    "watch": {
        "fps": 15,
        "ev": 0.5,
        "constraint": "shadows",
        "exposure": "long",
        "nr": "hq",
        "sharpness": 1.0,
        "contrast": 0.8,
        "saturation": 0.5,
        "awb": "fluorescent",
        "gains": "",
        "hdr": "night",
        "tuning": "",
        "lens_shading": "on",
    },
    "drive": {
        "fps": 15,
        "ev": 0.0,
        "constraint": "normal",
        "exposure": "normal",
        "nr": "fast",
        "sharpness": 1.2,
        "contrast": 1.0,
        "saturation": 1.0,
        "awb": "fluorescent",
        "gains": "",
        "hdr": "off",
        "tuning": "",
        "lens_shading": "on",
    },
}

ENV_KEYS = {
    "fps": "CAM_FPS",
    "ev": "CAM_EV",
    "constraint": "CAM_CONSTRAINT",
    "exposure": "CAM_EXPOSURE_MODE",
    "nr": "CAM_NR",
    "sharpness": "CAM_SHARP",
    "contrast": "CAM_CONTRAST",
    "saturation": "CAM_SAT",
    "awb": "CAM_AWB",
    "gains": "CAM_GAINS",
    "hdr": "CAM_HDR",
    "tuning": "CAM_TUNING",
    "lens_shading": "CAM_LSC",
}


def env_str(key, default):
    value = os.environ.get(key)
    return value if value not in (None, "") else default


def resolve(profile_name=None, overrides=None, use_env=True):
    """Profile values with environment overrides on top.

    `overrides` is keyed like PROFILES and is what measure.py uses to test a
    candidate without touching the environment or the service unit.
    """
    name = profile_name or env_str("CAM_PROFILE", "watch")
    if name not in PROFILES:
        print(f"[rover-cam] unknown CAM_PROFILE {name!r}, using watch", flush=True)
        name = "watch"
    values = dict(PROFILES[name])
    if use_env:
        for key, var in ENV_KEYS.items():
            values[key] = env_str(var, values[key])
    if overrides:
        values.update(overrides)
    for key in ("fps", "ev", "sharpness", "contrast", "saturation"):
        values[key] = float(values[key])
    values["profile"] = name
    return values


def controls_for(settings):
    """Map a resolved profile to libcamera control names and enum values.

    Rejected values are dropped one by one in apply_controls, so a sensor that
    cannot do HDR still streams in linear mode instead of failing to start.
    """
    from libcamera import controls as libc

    draft = libc.draft
    controls = {
        "AeExposureMode": {
            "normal": libc.AeExposureModeEnum.Normal,
            "long": libc.AeExposureModeEnum.Long,
        }[settings["exposure"]],
        "AeConstraintMode": {
            "normal": libc.AeConstraintModeEnum.Normal,
            "shadows": libc.AeConstraintModeEnum.Shadows,
            "highlight": libc.AeConstraintModeEnum.Highlight,
        }[settings["constraint"]],
        "NoiseReductionMode": {
            "off": draft.NoiseReductionModeEnum.Off,
            "fast": draft.NoiseReductionModeEnum.Fast,
            "hq": draft.NoiseReductionModeEnum.HighQuality,
        }[settings["nr"]],
        "Sharpness": settings["sharpness"],
        "Contrast": settings["contrast"],
        "Saturation": settings["saturation"],
        "ExposureValue": settings["ev"],
        "HdrMode": {
            "off": libc.HdrModeEnum.Off,
            "night": libc.HdrModeEnum.Night,
            "single": libc.HdrModeEnum.SingleExposure,
        }[settings["hdr"]],
        "LensShadingCorrectionEnable": settings["lens_shading"] == "on",
    }
    if settings["awb"] == "manual":
        red, blue = (float(part) for part in settings["gains"].split(","))
        controls["AwbEnable"] = False
        controls["ColourGains"] = (red, blue)
    else:
        controls["AwbEnable"] = True
        controls["AwbMode"] = {
            "auto": libc.AwbModeEnum.Auto,
            "incandescent": libc.AwbModeEnum.Incandescent,
            "tungsten": libc.AwbModeEnum.Tungsten,
            "fluorescent": libc.AwbModeEnum.Fluorescent,
            "indoor": libc.AwbModeEnum.Indoor,
            "daylight": libc.AwbModeEnum.Daylight,
            "cloudy": libc.AwbModeEnum.Cloudy,
        }[settings["awb"]]
    return controls


def apply_controls(cam, controls):
    """Apply controls one at a time so one bad value cannot blank the stream."""
    for name, value in controls.items():
        try:
            cam.set_controls({name: value})
        except Exception as exc:
            print(f"[rover-cam] control {name}={value!r} rejected: {exc}", flush=True)


def open_camera(settings):
    """Start picamera2 with the profile's frame duration and controls."""
    # Resolve the control map before touching the camera, so a bad profile
    # value raises here and never leaves a half-started pipeline behind.
    controls = controls_for(settings)

    from picamera2 import Picamera2

    # The frame duration range is the exposure budget. A fixed FrameRate would
    # pin exposure to 1/fps and throw away the low-light headroom below that.
    fastest = 33_333
    slowest = max(fastest, int(1_000_000 / settings["fps"]))
    cam = Picamera2(tuning=settings["tuning"]) if settings["tuning"] else Picamera2()
    config = cam.create_video_configuration(
        main={"size": (WIDTH, HEIGHT), "format": "RGB888"},
        controls={"FrameDurationLimits": (fastest, slowest)},
    )
    cam.configure(config)
    cam.start()
    apply_controls(cam, controls)
    return cam


class Hub:
    """Latest-frame fan-out. A slow client drops frames instead of blocking."""

    # Shared with /status so the dashboard can tell a live stream from a frozen
    # one. An MJPEG <img> reports neither load nor error per frame, so the page
    # needs this to know the camera stalled.
    stats = {"frames": 0, "last": time.monotonic(), "profile": ""}

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
        type(self).stats["frames"] += 1
        type(self).stats["last"] = time.monotonic()
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
        path = self.path.split("?", 1)[0]
        if path == "/status":
            stats = Hub.stats
            age_ms = int((time.monotonic() - stats["last"]) * 1000) if stats["frames"] else -1
            body = json.dumps({
                "frames": stats["frames"],
                "age_ms": age_ms,
                "profile": stats["profile"],
                "width": WIDTH,
                "height": HEIGHT,
            }).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if path not in ("/stream.mjpg", "/"):
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
    settings = resolve()
    Hub.stats["profile"] = settings["profile"]
    print(f"[rover-cam] profile {settings['profile']} {settings}", flush=True)
    cam = open_camera(settings)
    try:
        while True:
            buf = io.BytesIO()
            cam.capture_file(buf, format="jpeg")
            HUB.publish(buf.getvalue())
    finally:
        cam.stop()


def rpicam_loop():
    # The fallback cannot apply libcamera controls, it only keeps the stream up.
    fps = int(float(resolve()["fps"]))
    cmd = [
        "rpicam-vid", "-t", "0", "--codec", "mjpeg", "--inline",
        "--width", str(WIDTH), "--height", str(HEIGHT),
        "--framerate", str(fps), "-o", "-",
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
