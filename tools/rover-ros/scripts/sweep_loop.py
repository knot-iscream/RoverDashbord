#!/usr/bin/env python3
"""Auto scan loop for the parked rover.

The app starts this process through the lidar agent. It watches the motor
speeds on MQTT and sweeps the lidar only while the rover is parked:

  parked for 3 s  -> start a scan session and sweep
  any motor moves -> stop the sweep, park the servo, save the session

One parked period is one session. A session can contain several sweeps. It is
saved as partial when the rover moved before the first sweep finished.

State is published retained on `rover/scan/state`; a finished session emits
`rover/scan/saved` with its metadata. Files land in maps/scans/<id>/ on the
host and in /root/rover-ros/maps/scans/<id>/ inside the container.
"""
import json
import os
import shlex
import shutil
import signal
import subprocess
import threading
import time
from datetime import datetime, timezone

import paho.mqtt.client as mqtt

BROKER = "127.0.0.1"
PORT = 1883
MOTOR_TOPIC = "rover/motor/+"
AUTO_TOPIC = "rover/lidar/auto_scan"
STATE_TOPIC = "rover/scan/state"
SAVED_TOPIC = "rover/scan/saved"
SCAN_CMD_TOPIC = "rover/scan/command"
CMD_TOPIC = "rover/lidar/command"
CONTAINER = "rover-ros"
SCANS_DIR = "/home/iscream-pi/rover-ros/maps/scans"
LOG_FILE = "/home/iscream-pi/rover-ros/maps/scan.log"
SAVE_SCRIPT = "/root/rover-ros/scripts/save_scan.py"

PARK_S = 3.0
SWEEP_S = 52.0
CHECKPOINT_SWEEPS = 5
TICK_S = 0.5
SWEEP_CMD = {
    "action": "sweep",
    "min": -30,
    "max": 30,
    "step": 0.5,
    "settle_ms": 200,
    "dwell_ms": 200,
}

lock = threading.Lock()
client = None
running = True
auto = True
phase = "armed"
session = None
session_start = None
sweeps_done = 0
sweep_started = 0.0
last_motion = 0.0
speeds = {}
save_requested = False
last_state = 0.0
last_container_check = 0.0
container_ok = False
container_fail = 0


def log(msg):
    line = "%s %s" % (datetime.now().strftime("%H:%M:%S"), msg)
    print(line, flush=True)
    try:
        with open(LOG_FILE, "a", encoding="utf-8") as f:
            f.write(line + "\n")
    except OSError:
        pass


def now_iso():
    return datetime.now().astimezone().isoformat(timespec="seconds")


def publish_state(message="", force=False):
    global last_state
    now = time.monotonic()
    if not force and now - last_state < 1.0:
        return
    last_state = now
    payload = {
        "auto": auto,
        "phase": phase,
        "session": session or "",
        "sweeps": sweeps_done,
        "partial": bool(phase == "saving" and session and sweeps_done == 0),
        "message": message,
        "updated": now_iso(),
    }
    client.publish(STATE_TOPIC, json.dumps(payload), retain=True)


def send(action, **extra):
    body = {"action": action}
    body.update(extra)
    client.publish(CMD_TOPIC, json.dumps(body))


def container_running():
    for _ in range(2):
        try:
            out = subprocess.run(
                ["docker", "inspect", "-f", "{{.State.Running}}", CONTAINER],
                capture_output=True,
                text=True,
                timeout=10,
            )
            return out.returncode == 0 and out.stdout.strip() == "true"
        except (OSError, subprocess.SubprocessError):
            time.sleep(0.5)
    return False


def save_scan(session_id, start, end, sweeps, partial, checkpoint=False):
    """Run the in-container saver. Returns the parsed metadata or None."""
    args = [
        "python3",
        SAVE_SCRIPT,
        "--dir",
        "/root/rover-ros/maps/scans/%s" % session_id,
        "--id",
        session_id,
    ]
    if checkpoint:
        args.append("--checkpoint")
    else:
        args += [
            "--start",
            start,
            "--end",
            end,
            "--sweeps",
            str(sweeps),
        ]
        if partial:
            args.append("--partial")
    quoted = " ".join(shlex.quote(a) for a in args)
    cmd = [
        "docker",
        "exec",
        CONTAINER,
        "bash",
        "-c",
        "source /opt/ros/noetic/setup.bash; %s" % quoted,
    ]
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=90)
    except (OSError, subprocess.SubprocessError) as exc:
        log("save failed: %s" % exc)
        return None
    if out.returncode != 0:
        log("save failed: %s" % (out.stderr or out.stdout).strip()[:500])
        return None
    if checkpoint:
        log("checkpoint saved for %s" % session_id)
        return None
    try:
        with open(os.path.join(SCANS_DIR, session_id, "meta.json"), encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError) as exc:
        log("meta read failed: %s" % exc)
        return None


def checkpoint(session_id, start):
    threading.Thread(
        target=save_scan,
        args=(session_id, start, now_iso(), sweeps_done, False, True),
        daemon=True,
    ).start()


def start_session():
    global session, session_start, sweeps_done, sweep_started, phase
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    session = stamp
    n = 2
    while os.path.isdir(os.path.join(SCANS_DIR, session)):
        session = "%s-%d" % (stamp, n)
        n += 1
    session_start = now_iso()
    os.makedirs(os.path.join(SCANS_DIR, session), exist_ok=True)
    sweeps_done = 0
    sweep_started = time.monotonic()
    phase = "scanning"
    send("sweep", **{k: v for k, v in SWEEP_CMD.items() if k != "action"})
    log("session %s started" % session)
    publish_state("Scanning session %s" % session, force=True)


def is_moving():
    with lock:
        return any(v != 0.0 for v in speeds.values())


def close_session(partial):
    """Stop the sweep, save the session and return to armed/paused."""
    global phase, session, session_start, sweeps_done
    sid = session
    moving = is_moving()
    if not sid:
        phase = "paused" if moving else "armed"
        return
    phase = "saving"
    publish_state("Saving scan %s" % sid, force=True)
    send("stop")
    # Give the servo time to park before the map is dumped.
    time.sleep(0.8)
    meta = save_scan(
        sid,
        session_start or "",
        now_iso(),
        sweeps_done,
        partial,
    )
    if meta and meta.get("points", 0) == 0:
        # A failed lidar start or a lost scan leaves an empty octree. Do not
        # put that in the history; remove the folder and report it.
        log("session %s empty, discarded" % sid)
        shutil.rmtree(os.path.join(SCANS_DIR, sid), ignore_errors=True)
        meta = None
    if meta:
        client.publish(SAVED_TOPIC, json.dumps(meta))
        log("session %s saved (%d sweeps, %d points)" %
            (sid, sweeps_done, meta.get("points", 0)))
    else:
        log("session %s not saved" % sid)
    session = None
    session_start = None
    sweeps_done = 0
    phase = "paused" if moving else "armed"
    publish_state(
        "Rover moving, waiting for stop" if moving else "Waiting for the rover to stop",
        force=True,
    )


def on_connect(c, _userdata, _flags, _rc, _props=None):
    c.subscribe(MOTOR_TOPIC)
    c.subscribe(AUTO_TOPIC)
    c.subscribe(SCAN_CMD_TOPIC)
    log("mqtt connected")
    publish_state("Auto scan armed" if auto else "Auto scan off", force=True)


def on_message(_c, _userdata, msg):
    global auto, last_motion, save_requested
    try:
        body = json.loads(msg.payload.decode("utf-8", "replace"))
    except (ValueError, UnicodeDecodeError):
        return
    if msg.topic == SCAN_CMD_TOPIC:
        if body.get("action") == "save":
            with lock:
                save_requested = True
        return
    if msg.topic == AUTO_TOPIC:
        value = body.get("enabled", body.get("auto"))
        if isinstance(value, bool):
            with lock:
                auto = value
            publish_state("Auto scan armed" if auto else "Auto scan off", force=True)
        return
    if not msg.topic.startswith("rover/motor/"):
        return
    speed = body.get("speed")
    try:
        speed = float(speed)
    except (TypeError, ValueError):
        speed = 0.0
    key = str(body.get("motor", msg.topic.rsplit("/", 1)[-1]))
    with lock:
        speeds[key] = speed
        if speed != 0.0:
            last_motion = time.monotonic()


def on_sigterm(_signum, _frame):
    global running
    running = False


def state_tick(now, moving):
    """Advance the state machine one tick."""
    global phase, sweeps_done, sweep_started, container_ok, last_container_check
    global container_fail, save_requested
    # Motor telemetry can go quiet on the bench with the MID board off. Never
    # clear the last known speed here: a board that dies while the rover is
    # moving must not look parked and trigger a scan mid-drive.

    if now - last_container_check > 3.0:
        last_container_check = now
        if container_running():
            container_ok = True
            container_fail = 0
        else:
            # One slow `docker inspect` must not end a good session.
            container_fail += 1
            if container_fail >= 2:
                if container_ok:
                    log("container check failed twice, stopping the scan")
                container_ok = False

    if not container_ok:
        if phase == "scanning" and session:
            close_session(partial=sweeps_done == 0)
        if phase != "idle":
            phase = "idle"
            publish_state("Lidar container is not running", force=True)
        return

    if not auto:
        if phase == "scanning" and session:
            close_session(partial=sweeps_done == 0)
        if phase != "idle":
            phase = "idle"
            publish_state("Auto scan off", force=True)
        return

    if save_requested and phase != "scanning":
        save_requested = False
    if phase == "scanning" and save_requested:
        save_requested = False
        log("manual save requested")
        close_session(partial=sweeps_done == 0)
        return

    if phase == "idle":
        phase = "armed"
        publish_state("Auto scan armed", force=True)

    if phase == "scanning":
        if moving:
            close_session(partial=sweeps_done == 0)
            publish_state("Rover moving, scan saved", force=True)
            return
        elapsed = now - sweep_started
        if elapsed >= SWEEP_S:
            sweeps_done += 1
            if sweeps_done % CHECKPOINT_SWEEPS == 0:
                checkpoint(session, session_start or "")
            sweep_started = now
            send("sweep", **{k: v for k, v in SWEEP_CMD.items() if k != "action"})
            publish_state(
                "Scanning session %s, %d sweeps" % (session, sweeps_done),
                force=True,
            )
        return

    if phase in ("armed", "paused"):
        if moving:
            if phase != "paused":
                phase = "paused"
                publish_state("Rover moving, waiting for stop", force=True)
            return
        if now - last_motion >= PARK_S:
            start_session()
            return
        if phase != "armed":
            phase = "armed"
            publish_state("Waiting for the rover to stop", force=True)


def main():
    global client, last_motion
    os.makedirs(SCANS_DIR, exist_ok=True)
    signal.signal(signal.SIGTERM, on_sigterm)
    signal.signal(signal.SIGINT, on_sigterm)
    client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id="rover-sweep-loop")
    client.on_connect = on_connect
    client.on_message = on_message
    client.connect(BROKER, PORT, 30)
    client.loop_start()
    last_motion = time.monotonic() - PARK_S
    publish_state("Auto scan starting", force=True)
    log("sweep loop started")
    try:
        while running:
            state_tick(time.monotonic(), is_moving())
            time.sleep(TICK_S)
    finally:
        if session:
            close_session(partial=sweeps_done == 0)
        send("stop")
        publish_state("Sweep loop stopped", force=True)
        log("sweep loop stopped")
        client.loop_stop()
        client.disconnect()


if __name__ == "__main__":
    main()
