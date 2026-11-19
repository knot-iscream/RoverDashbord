#!/usr/bin/env python3
"""RPLIDAR A1 -> MQTT bridge for the rover dashboard.

Runs on the Pi. Reads 360 degree scans from /dev/ttyUSB0 (CP210x adapter) and
publishes each completed revolution to rover/lidar/scan as JSON:

    {"ok":1, "model":12, "count":360, "ts":1234.5,
     "points":[[angle_deg, distance_mm, quality], ...]}

No angle is invented: the A1 reports its own heading for every sample.
"""
import argparse
import json
import sys
import time

import serial
import paho.mqtt.client as mqtt

SYNC = b"\xA5"
SYNC2 = b"\x5A"
SCAN_BYTE = b"\x20"
STOP_BYTE = b"\x25"
RESET_BYTE = b"\x40"
INFO_BYTE = b"\x50"
HEALTH_BYTE = b"\x52"
DESCRIPTOR_LEN = 7
NODE_LEN = 5
BAUD = 115200
TOPIC = "rover/lidar/scan"


class LidarError(RuntimeError):
    pass


def send_cmd(ser, cmd):
    ser.write(SYNC + cmd)
    ser.flush()


def read_exact(ser, n):
    data = ser.read(n)
    if len(data) != n:
        raise LidarError("serial read: wanted %d bytes, got %d" % (n, len(data)))
    return data


def read_descriptor(ser):
    d = read_exact(ser, DESCRIPTOR_LEN)
    if d[:2] != SYNC + SYNC2:
        raise LidarError("bad descriptor header: %s" % d[:2].hex())
    return d[2], d[-2], d[-1]  # dsize, is_single(0), dtype


def get_info(ser):
    send_cmd(ser, INFO_BYTE)
    dsize, single, dtype = read_descriptor(ser)
    if dsize != 20 or dtype != 4 or single != 0:
        raise LidarError("bad info descriptor: %s %s %s" % (dsize, single, dtype))
    raw = read_exact(ser, dsize)
    return {
        "model": raw[0],
        "firmware": (raw[2] << 8) | raw[1],
        "hardware": raw[3],
        "serial": raw[4:].hex().upper(),
    }


def get_health(ser):
    send_cmd(ser, HEALTH_BYTE)
    dsize, single, dtype = read_descriptor(ser)
    if dsize != 3 or dtype != 6 or single != 0:
        raise LidarError("bad health descriptor")
    raw = read_exact(ser, dsize)
    return raw[0], (raw[1] << 8) | raw[2]


def parse_node(raw):
    new_scan = bool(raw[0] & 0x01)
    inverse = bool((raw[0] >> 1) & 0x01)
    if new_scan == inverse:
        raise LidarError("scan flag mismatch")
    if not (raw[1] & 0x01):
        raise LidarError("check bit not set")
    quality = raw[0] >> 2
    angle = ((raw[1] >> 1) + (raw[2] << 7)) / 64.0
    distance = (raw[3] + (raw[4] << 8)) / 4.0  # mm
    return new_scan, quality, angle, distance


def start_scan(ser):
    send_cmd(ser, SCAN_BYTE)
    dsize, single, dtype = read_descriptor(ser)
    # Scan is a multi-response stream: descriptor[-2] is nonzero here.
    if dsize != NODE_LEN or dtype != 129 or single == 0:
        raise LidarError("bad scan descriptor: %s %s %s" % (dsize, single, dtype))


def stop_scan(ser):
    try:
        send_cmd(ser, STOP_BYTE)
    except OSError:
        pass


def make_client(host, port):
    client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id="rover-lidar")
    client.connect_async(host, port, keepalive=30)
    client.loop_start()
    return client


def probe(port):
    ser = serial.Serial(port, BAUD, timeout=2, dsrdtr=False)
    ser.dtr = False  # A1 motor on (CP210x DTR line)
    try:
        info = get_info(ser)
        status, code = get_health(ser)
        print("info: %s" % info)
        print("health: status=%d code=%d" % (status, code))
        if status == 2:
            print("hardware error, reset not attempted in probe mode")
        start_scan(ser)
        for _ in range(8):
            new, q, a, d = parse_node(read_exact(ser, NODE_LEN))
            print("new=%d q=%2d a=%7.2f d=%7.1f" % (new, q, a, d))
    finally:
        stop_scan(ser)
        ser.dtr = True
        ser.close()


def stream(port, host, mqtt_port):
    client = make_client(host, mqtt_port)
    scan = []
    info = {}
    try:
        while True:
            ser = None
            try:
                ser = serial.Serial(port, BAUD, timeout=2, dsrdtr=False)
                ser.dtr = False
                info = get_info(ser)
                status, _ = get_health(ser)
                if status == 2:
                    raise LidarError("sensor health error")
                start_scan(ser)
                print("scanning: model=%s" % info.get("model"), flush=True)
                scan = []
                while True:
                    new, q, a, d = parse_node(read_exact(ser, NODE_LEN))
                    if new:
                        if len(scan) > 5:
                            payload = {
                                "ok": 1,
                                "model": info.get("model", 0),
                                "count": len(scan),
                                "ts": round(time.time(), 3),
                                "points": scan,
                            }
                            client.publish(TOPIC, json.dumps(payload), qos=0)
                        scan = []
                    if d > 0:
                        scan.append([round(a, 2), round(d, 1), q])
            except (LidarError, OSError, serial.SerialException) as exc:
                print("lidar error: %s; reconnecting" % exc, file=sys.stderr, flush=True)
                if ser is not None:
                    stop_scan(ser)
                    try:
                        ser.dtr = True
                        ser.close()
                    except OSError:
                        pass
                time.sleep(2)
    except KeyboardInterrupt:
        pass
    finally:
        client.loop_stop()
        client.disconnect()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["probe", "stream"])
    ap.add_argument("--port", default="/dev/ttyUSB0")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--mqtt-port", type=int, default=1883)
    args = ap.parse_args()
    if args.mode == "probe":
        probe(args.port)
    else:
        stream(args.port, args.host, args.mqtt_port)


if __name__ == "__main__":
    main()
