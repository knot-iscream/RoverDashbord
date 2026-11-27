#!/usr/bin/env python3
"""Bridge ESP32 USB serial JSON lines to the local MQTT broker.

The ESPs print one JSON object per line:

    {"topic":"rover/mid/status","payload":{...}}

The bridge publishes ``payload`` to ``topic`` on the local broker. Command
topics from MQTT are written back in the same envelope. The lidar serial port
is never opened; only the ports listed in ``BRIDGE_PORTS``.

The node name comes from the hello line:

    {"topic":"rover/node/hello","payload":{"node":"mid"}}

That matters because the CP210x adapters all report serial 0001.
"""

import json
import os
import signal
import threading
import time

import paho.mqtt.client as mqtt
import serial

MQTT_BROKER = os.environ.get("MQTT_BROKER", "127.0.0.1")
MQTT_PORT = int(os.environ.get("MQTT_PORT", "1883"))
PORTS = [p.strip() for p in os.environ.get("BRIDGE_PORTS", "/dev/ttyUSB1").split(",") if p.strip()]
BAUD = int(os.environ.get("BRIDGE_BAUD", "115200"))
DEFAULT_NODE = os.environ.get("BRIDGE_DEFAULT_NODE", "mid").strip() or None
STATUS_TOPIC = "rover/serial/status"

# topic -> node that owns the command
COMMAND_NODES = {
    "rover/motor/command": "corner",
    "rover/calibration/command": "corner",
    "rover/mid/command": "mid",
    "rover/lidar/command": "mid",
    "rover/lidar/tilt_deg": "mid",
    "rover/lidar/tilt_zero": "mid",
    "rover/env/command": "env",
}


class Port:
    def __init__(self, path, client, default_node):
        self.path = path
        self.client = client
        self.node = default_node
        self.ser = None
        self.last_rx = 0.0
        self.last_err = 0.0
        self.lock = threading.Lock()
        self.stop = threading.Event()
        self.thread = None

    def start(self):
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def open(self):
        self.ser = serial.Serial(self.path, BAUD, timeout=0.5)
        self.ser.reset_input_buffer()
        # This CP210x clone holds the ESP in reset with RTS high. Pulse both
        # lines and leave them low; the app then boots and prints its hello.
        self.ser.dtr = True
        self.ser.rts = True
        time.sleep(0.2)
        self.ser.dtr = False
        self.ser.rts = False
        print("[bridge] opened %s" % self.path, flush=True)

    def close(self):
        if self.ser is not None:
            try:
                self.ser.close()
            except Exception:
                pass
        self.ser = None

    def run(self):
        while not self.stop.is_set():
            try:
                if self.ser is None:
                    self.open()
                line = self.ser.readline()
                if not line:
                    continue
                text = line.decode("utf-8", "replace").strip()
                if not text.startswith("{"):
                    continue
                self.handle(text)
            except Exception as e:
                now = time.monotonic()
                if now - self.last_err >= 30.0:
                    self.last_err = now
                    print("[bridge] %s: %s" % (self.path, e), flush=True)
                self.close()
                time.sleep(2.0)

    def handle(self, text):
        try:
            msg = json.loads(text)
        except ValueError:
            return
        topic = msg.get("topic")
        payload = msg.get("payload")
        if not topic or payload is None:
            return
        self.last_rx = time.monotonic()
        if topic == "rover/node/hello":
            node = payload.get("node")
            if node:
                self.node = node
                print("[bridge] %s -> node %s" % (self.path, node), flush=True)
            self.client.publish(topic, json.dumps(payload), retain=True)
            return
        self.client.publish(topic, json.dumps(payload), retain=False)

    def write(self, topic, payload):
        if self.ser is None:
            return False
        line = json.dumps({"topic": topic, "payload": payload}) + "\n"
        with self.lock:
            try:
                self.ser.write(line.encode("utf-8"))
                self.ser.flush()
                return True
            except Exception as e:
                print("[bridge] write %s: %s" % (self.path, e), flush=True)
                self.close()
                return False


def main():
    client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id="rover-serial-bridge")
    ports = [Port(p, client, DEFAULT_NODE) for p in PORTS]

    def on_message(_client, _userdata, msg):
        node = COMMAND_NODES.get(msg.topic)
        if node is None:
            return
        try:
            payload = json.loads(msg.payload.decode("utf-8"))
        except ValueError:
            return
        for p in ports:
            if p.node == node:
                p.write(msg.topic, payload)
                return
        print("[bridge] no port for %s" % node, flush=True)

    def on_connect(_client, _userdata, _flags, reason_code, _properties=None):
        # paho reconnects on its own after a broker restart, but the old
        # subscriptions are gone. Without this the bridge keeps publishing
        # telemetry while silently ignoring every command.
        if reason_code != 0:
            print("[bridge] mqtt connect failed: %s" % reason_code, flush=True)
            return
        for topic in COMMAND_NODES:
            _client.subscribe(topic)
        print("[bridge] mqtt connected, %d command topics" % len(COMMAND_NODES), flush=True)

    client.on_message = on_message
    client.on_connect = on_connect
    client.connect(MQTT_BROKER, MQTT_PORT, 30)
    client.loop_start()
    for p in ports:
        p.start()

    stop = threading.Event()

    def on_signal(_signum, _frame):
        stop.set()

    signal.signal(signal.SIGINT, on_signal)
    signal.signal(signal.SIGTERM, on_signal)

    try:
        while not stop.wait(1.0):
            status = {
                "ok": 1,
                "ports": [
                    {
                        "port": p.path,
                        "node": p.node,
                        "age_s": round(time.monotonic() - p.last_rx, 1) if p.last_rx else None,
                    }
                    for p in ports
                ],
            }
            client.publish(STATUS_TOPIC, json.dumps(status), retain=True)
    finally:
        for p in ports:
            p.stop.set()
            p.close()
        client.loop_stop()
        client.disconnect()


if __name__ == "__main__":
    main()
