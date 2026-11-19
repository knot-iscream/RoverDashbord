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
    s.dtr = True
    s.close()
    print(f"lidar motor off ({port})")


if __name__ == "__main__":
    main()
