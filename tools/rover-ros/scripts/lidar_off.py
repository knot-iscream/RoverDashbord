#!/usr/bin/env python3
"""Force the RPLIDAR A1 motor off by driving the CP210x DTR line high.

The A1 adapter starts the motor when DTR is low. The rplidar ROS node lowers
DTR and does not always restore it on exit, so the head keeps spinning after
the container stops. Call this on shutdown, or by hand as a safety switch.
"""
import sys

import serial


def main():
    port = sys.argv[1] if len(sys.argv) > 1 else "/dev/ttyUSB0"
    s = serial.Serial(port, 115200)
    # Official A1 adapters use DTR for MOTOCTL. Some clones use RTS instead,
    # so drive both high before closing.
    s.dtr = True
    s.rts = True
    s.close()
    print(f"lidar motor off ({port}, DTR+RTS high)")


if __name__ == "__main__":
    main()
