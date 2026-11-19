# Rover camera on the Pi 5

The CSI camera rides on the rover with the Pi. The Pi keeps its other jobs:
venue WiFi (`iscream-hotspot`, PSK `789632145`, address `10.42.0.1`) and MQTT
relay. The laptop joins that hotspot and the dashboard picks up the stream
automatically from `/api/config`, so there is no address to configure.

Power comes from the 5.1 V / 8 A XL4016 rail that already feeds the Pi and the
sensors. Mount the Pi so the ribbon is not stretched or pulled by the wheels.

## Install (once, on the Pi)

```sh
sudo apt update
sudo apt install -y python3-picamera2

# Confirm the camera is seen. A preview window should appear; close it.
rpicam-hello

mkdir -p ~/rover-cam
# Copy camera_stream.py and rover-cam.service here from the repo's
# tools/rover-cam/ folder (USB stick, scp, or paste over SSH).

sudo install -m 644 ~/rover-cam/rover-cam.service /etc/systemd/system/rover-cam.service
sudo systemctl daemon-reload
sudo systemctl enable --now rover-cam
```

If `python3-picamera2` is not in the package list, the script falls back to
`rpicam-vid --codec mjpeg` automatically, so the install still works without
that package as long as `rpicam-vid` is present.

## Verify

On the Pi:

```sh
systemctl status rover-cam
curl -s http://127.0.0.1:8081/stream.mjpg | head -c 4 | xxd
```

The four bytes should be `ff d8 ff d8` or `ff d8 ff e0` (JPEG start markers).
Open `http://127.0.0.1:8081/stream.mjpg` in a browser on the Pi to see motion.

From the laptop, with the broker still at `192.168.0.175`:

```sh
curl -s http://192.168.0.175:8081/stream.mjpg | head -c 4 | xxd
```

Then open `/control` and the Home dashboard. The mirror shows `ROVER CAM`
with a blinking LIVE chip; the Home card "Normal" tab shows the feed. If the
stream is down, both pages show an offline state and the rest of the dashboard
keeps working.

Two clients can hold the stream at once. If a page shows CAMERA OFFLINE while
`curl` works, check the browser console and that the page host resolved the
broker address shown by `/api/config`.

## Service management

```sh
sudo systemctl restart rover-cam
sudo systemctl stop rover-cam
journalctl -u rover-cam -f
```

## Saturday notes

The hotspot must be up before the laptop joins it. Bringing it up drops the
Pi's home WiFi and the SSH session, so do that test last, from the Pi's own
screen. The camera service starts at boot and does not depend on which network
the Pi is on; it simply listens on all interfaces at `:8081`.

If the picture is too dark, the camera auto-exposure needs a few seconds after
power-on. If it never settles, put a small light near the rover rather than
changing exposure in firmware.
