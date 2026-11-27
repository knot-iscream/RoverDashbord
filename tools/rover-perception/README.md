# Rover perception (NanoDet)

Runs the OpenCV Zoo NanoDet-m-plus-1.5x-416 detector on the Pi camera stream
and publishes normalised boxes for the dashboard and the autopilot.

- Model: COCO 80 classes, Apache-2.0.
- Input: the `rover-cam` MJPEG stream on `http://127.0.0.1:8081/stream.mjpg`.
- Output: `rover/perception/detections` at about 5 fps.
- No training, no fine-tuning. Person is class 0 and the class the autopilot
  uses for its stop veto.

The detector is a fair-weather aid. Smoke, darkness and bulky turnout gear
degrade it. It is not the fire detector; the flame sensor, MLX pair and gas
suite carry that.

## Install on the Pi

```sh
sudo apt install python3-opencv python3-paho-mqtt
mkdir -p ~/rover-perception
# copy the files from tools/rover-perception/ into ~/rover-perception/
cd ~/rover-perception
python3 download_model.py
python3 perception.py --selftest
```

If `python3-paho-mqtt` is not in the Debian repo:

```sh
pip3 install --user paho-mqtt
```

Test on a real frame:

```sh
python3 perception.py --image /path/to/frame.jpg
```

Enable the service:

```sh
sudo cp rover-perception.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now rover-perception
journalctl -u rover-perception -f
```

The unit starts after `rover-cam.service` and only needs localhost.

## Configuration

Environment variables, all optional:

| Variable | Default | Use |
|---|---|---|
| `PERCEPTION_STREAM` | `http://127.0.0.1:8081/stream.mjpg` | camera stream |
| `PERCEPTION_MODEL` | `models/object_detection_nanodet_2022nov.onnx` | model file |
| `PERCEPTION_INTERVAL` | `0.20` | minimum seconds between inferences |
| `PERCEPTION_CONF` | `0.35` | confidence floor |
| `PERCEPTION_MAX_ITEMS` | `12` | boxes per payload |
| `PERCEPTION_CAM_HFOV` | `53.5` | camera horizontal FOV, degrees (OV5647 v1) |
| `PERCEPTION_CAM_YAW` | `0.0` | lidar angle at the image centre, degrees; positive = left |
| `PERCEPTION_RANGE_WINDOW` | `2.5` | scan window around a box bearing, degrees |
| `PERCEPTION_SCAN_STALE` | `1.0` | drop the scan cache after this many seconds |
| `MQTT_BROKER` | `127.0.0.1` | broker host |
| `MQTT_PORT` | `1883` | broker port |

## Payload

```json
{
  "ok": 1,
  "w": 640,
  "h": 480,
  "ms": 26.4,
  "fps": 5.0,
  "items": [
    {"c": 0, "label": "person", "p": 0.72,
     "x": 0.41, "y": 0.30, "w": 0.18, "h": 0.40,
     "range_m": 1.42, "bearing_deg": -3.2}
  ]
}
```

Boxes are normalised to the frame, origin top-left. `ok:0` with
`reason:"stream"` means the camera stream failed; the page should clear the
boxes.

`range_m` and `bearing_deg` are optional. The service subscribes to
`rover/lidar/scan` and, for each box, takes the closest return within
`PERCEPTION_RANGE_WINDOW` degrees of the box centre. When the lidar container
is off, or the return is older than `PERCEPTION_SCAN_STALE`, the fields are
simply absent and the box is still published. `PERCEPTION_CAM_YAW` is the
one-time calibration: the lidar angle that lands on the image centre. Set it
by putting an object on the camera centreline and reading its angle from the
scan. Positive angles are to the left.

## Performance

On this PC, 416x416 FP32 through OpenCV DNN is about 26 to 33 ms. The Pi 5
will be slower; measure with `--selftest` before committing to 5 fps. If it
is short, raise `PERCEPTION_INTERVAL` to 0.33 or 0.5 first. The int8 model in
the same OpenCV Zoo folder is the next step, but OpenCV DNN quantisation
support is less predictable than the FP32 path.

At rover speed, 3 fps is still usable: the stop veto is a slow, conservative
action and the firmware deadman covers a dead service.

## Model licence

The model directory in OpenCV Zoo is Apache-2.0. The model is
NanoDet-m-plus-1.5x-416, COCO val mAP 30.4, person AP50 67.5. Postprocess is
adapted from the zoo demo with output pairing by shape, because OpenCV 5
returns the classification and box tensors grouped rather than alternating.
