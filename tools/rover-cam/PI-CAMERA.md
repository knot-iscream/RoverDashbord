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
curl -s http://127.0.0.1:8081/stream.mjpg | head -c 200 | xxd | head -8
```

The stream is `multipart/x-mixed-replace`, so the first bytes are the boundary
`--frame` (`2d 2d 66 72`), then `Content-Type: image/jpeg`, then
`Content-Length`, then the JPEG itself starting at `ff d8 ff e1`. A bare
`head -c 4` returning `--fr` is correct, not a failure. Open
`http://127.0.0.1:8081/stream.mjpg` in a browser on the Pi to see motion.

From the laptop, with the broker still at `192.168.0.175`:

```sh
curl -s http://192.168.0.175:8081/stream.mjpg | head -c 200 | xxd | head -8
```

Then open `/control` and the Home dashboard. The mirror shows `ROVER CAM`
with a blinking LIVE chip; the Home card "Normal" tab shows the feed. If the
stream is down, both pages show an offline state and the rest of the dashboard
keeps working.

Two clients can hold the stream at once. If a page shows CAMERA OFFLINE while
`curl` works, check the browser console and that the page host resolved the
broker address shown by `/api/config`.

## Liveness

An MJPEG `<img>` fires neither `load` nor `error` per frame, so a stopped
camera leaves the last frame on screen forever. The service therefore exposes:

```sh
curl -s http://127.0.0.1:8081/status
# {"frames": 63, "age_ms": 45, "profile": "watch", "width": 640, "height": 480}
```

CORS is open on this endpoint. The Home card and the Control mirror poll it
every 2 seconds and re-arm the `<img>` when `age_ms` passes 2500 ms, so they
drop to the offline placeholder within a few seconds and recover on their own
when the camera returns. Do not remove that poll: without it a camera restart
shows a frozen frame with the LIVE badge still lit.

## Quality profiles

The stream runs one of two profiles, picked by `CAM_PROFILE` in the unit
(`Environment=CAM_PROFILE=watch`). Restart to switch:

```sh
sudo systemctl restart rover-cam
```

| | `watch` | `drive` |
|---|---|---|
| Frame rate | 15 fps | 15 fps |
| HDR | Night (shadow lift) | off |
| White balance | fluorescent | fluorescent |
| Noise reduction | high quality | fast |
| Saturation | 0.50 | 1.00 |
| Contrast | 0.80 | 1.00 |
| Sharpness | 1.0 | 1.2 |

`watch` lifts shadows with the Night tonemap and a flatter contrast curve.
`drive` leaves both off and keeps full saturation for daylight. Both run 15 fps,
picked on 25 Nov with `measure.py` on the OV5647 v1 module. An earlier 8 fps
attempt was reverted: the AGC still stopped at 120 ms, so it was choppier with
no extra light, and the longer exposure measured about twice as noisy
(`n_med` 3.02 vs 1.43). The final `watch` is about 2.6x brighter than the old
15 fps default (luma 36 to 93) and uses gain 4.0 instead of 8.0, so the grain is
lower. Contrast 0.8 is what lifts the shadows: p5 57 to 71 with grain 0.51 to
0.38. It also mutes the purple veil, but does not remove it.

Any value can be overridden with an environment variable, no code edit needed:

```
CAM_FPS                 frame rate
CAM_EV                  exposure bias, -2.0 .. 2.0
CAM_CONSTRAINT          normal | shadows | highlight
CAM_EXPOSURE_MODE       normal | long
CAM_NR                  off | fast | hq
CAM_SHARP               sharpness
CAM_CONTRAST            tone curve
CAM_SAT                 saturation
CAM_AWB                 auto | incandescent | tungsten | fluorescent | indoor |
                        daylight | cloudy | manual
CAM_GAINS               "r,b", only with CAM_AWB=manual
CAM_HDR                 off | night | single
CAM_TUNING              path to a custom libcamera tuning JSON
```

Set them in a systemd drop-in so the checked-in unit stays clean:

```sh
sudo systemctl edit rover-cam
# [Service]
# Environment=CAM_PROFILE=drive
```

Use `CAM_AWB=auto` outdoors in daylight. The fluorescent lock is what removes
the blue cast in the venue's indoor lighting, but it runs slightly warm in sun.

## Measuring

`measure.py` does not stop the stream, because only one process can hold the
sensor:

```sh
sudo systemctl stop rover-cam
python3 ~/rover-cam/measure.py /tmp/cam-quality
sudo systemctl start rover-cam
```

It writes one JPEG per candidate and prints mean luma, p5/p50/p95, median and
p90 spatial noise, temporal grain (median block change between two frames),
R/G and B/G ratios, and the exposure and gain the AGC actually chose. Change
the candidate list in the script to test new settings.

## Known limits

- `HdrMode=single` breaks the OV5647 and returns a flat purple wash. Leave it off.
- The Pi 5 neural AWB (`rpi.nn.awb`) needs
  `/usr/share/libcamera/ipa/rpi/pisp/awb_model.tflite`, which the Debian
  `libcamera-ipa` package does not ship. It stays disabled.
- The OV5647 is a 2013 sensor. The profiles make a dim room usable, but the
  shadows still carry noise and a purple tint from the sensor floor.
- `LensShadingCorrectionEnable` is not advertised by the PiSP pipeline, so the
  shading table cannot be toggled. A bright oval is not a shading-table fault.
- The remaining purple veil in very low light is not something AWB can remove.
  Check the lens for the translucent blue shipping film and for a light leak
  around the sensor before blaming the profile.
- Manual `ColourGains` calculated from the whole-frame mean overshoot to green.
  Do not use them without a grey card in the scene.

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

If the picture is too dark, check which profile is running
(`systemctl show rover-cam -p Environment`). The camera also needs a few seconds
after power-on for auto-exposure to settle. If it stays dark even on `watch`,
add light rather than pushing the sensor further.
