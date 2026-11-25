#!/usr/bin/env python3
"""Hold the RPLIDAR A1 motor off by keeping the serial port open.

The CP210x clone on this lidar floats DTR when the port closes, so the motor
restarts a few seconds after lidar_off.py exits. This process opens the port,
sends SET_MOTOR_PWM 0, raises DTR/RTS and keeps them there. The agent runs it
only while the lidar service is stopped; it must exit before rplidarNode
starts, otherwise the node cannot open the port.

The command is resent every 2 s in case the lidar firmware resets its PWM.
"""
import signal
import sys
import time

import serial

MOTOR_PWM_ZERO = bytes([0xA5, 0xF0, 0x02, 0x00, 0x00, 0x57])
HOLD_EVERY_S = 2.0
OPEN_TRIES = 12
OPEN_WAIT_S = 0.5

running = True


def on_signal(_signum, _frame):
    global running
    running = False


def open_port(port):
    last = None
    for attempt in range(OPEN_TRIES):
        try:
            return serial.Serial(port, 115200, timeout=1)
        except serial.SerialException as exc:
            last = exc
            if attempt < OPEN_TRIES - 1:
                time.sleep(OPEN_WAIT_S)
    raise last


def main():
    global running
    port = sys.argv[1] if len(sys.argv) > 1 else "/dev/ttyUSB0"
    signal.signal(signal.SIGTERM, on_signal)
    signal.signal(signal.SIGINT, on_signal)
    try:
        s = open_port(port)
    except serial.SerialException as exc:
        print("hold open failed: %s" % exc, flush=True)
        return 1
    print("holding lidar off (%s)" % port, flush=True)
    try:
        while running:
            try:
                s.write(MOTOR_PWM_ZERO)
                s.flush()
                s.dtr = True
                s.rts = True
                s.reset_input_buffer()
            except serial.SerialException as exc:
                print("hold lost: %s" % exc, flush=True)
                return 1
            time.sleep(HOLD_EVERY_S)
    finally:
        try:
            s.close()
        except serial.SerialException:
            pass
        print("lidar hold released (%s)" % port, flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
