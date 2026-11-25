#!/usr/bin/env python3
"""Stop the RPLIDAR A1 motor: motor PWM 0 plus DTR/RTS high.

The A1 adapter starts the motor when DTR is low. This CP210x clone ignores
DTR and RTS, so the motor can keep spinning after the ROS node exits. The A1
also accepts the standard SET_MOTOR_PWM command, so send PWM 0 first and then
raise both control lines. Some adapters honour one, some the other.

Call this on shutdown, or by hand as a safety switch.
"""
import sys
import time

import serial

# A1M8 command: A5 F0 02 <pwm low> <pwm high> <checksum>. PWM 0, checksum 57.
MOTOR_PWM_ZERO = bytes([0xA5, 0xF0, 0x02, 0x00, 0x00, 0x57])


def main():
    port = sys.argv[1] if len(sys.argv) > 1 else "/dev/ttyUSB0"
    s = serial.Serial(port, 115200, timeout=1)
    try:
        s.write(MOTOR_PWM_ZERO)
        s.flush()
        time.sleep(0.3)
    except serial.SerialException as exc:
        print(f"motor pwm command failed: {exc}")
    # Official A1 adapters use DTR for MOTOCTL. Some clones use RTS instead,
    # so drive both high before closing.
    s.dtr = True
    s.rts = True
    time.sleep(0.1)
    s.close()
    print(f"lidar motor off ({port}, pwm 0 + DTR/RTS high)")


if __name__ == "__main__":
    main()
