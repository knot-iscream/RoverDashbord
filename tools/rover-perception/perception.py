#!/usr/bin/env python3
"""NanoDet perception service for the rover camera.

Reads the MJPEG stream from `rover-cam`, runs the OpenCV Zoo NanoDet model at
about 5 fps, and publishes normalised boxes on `rover/perception/detections`.

The detector is a fair-weather aid. Smoke, darkness and bulky turnout gear
degrade it; the lidar reflex and the gas/thermal sensors carry the mission.
"""

import argparse
import json
import logging
import math
import os
import signal
import sys
import time
import urllib.request

import cv2
import numpy as np

DET_TOPIC = "rover/perception/detections"
MODEL_FILE = "object_detection_nanodet_2022nov.onnx"
HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_MODEL = os.path.join(HERE, "models", MODEL_FILE)
DEFAULT_STREAM = "http://127.0.0.1:8081/stream.mjpg"

CLASSES = (
    "person", "bicycle", "car", "motorcycle", "airplane", "bus", "train",
    "truck", "boat", "traffic light", "fire hydrant", "stop sign",
    "parking meter", "bench", "bird", "cat", "dog", "horse", "sheep", "cow",
    "elephant", "bear", "zebra", "giraffe", "backpack", "umbrella", "handbag",
    "tie", "suitcase", "frisbee", "skis", "snowboard", "sports ball", "kite",
    "baseball bat", "baseball glove", "skateboard", "surfboard",
    "tennis racket", "bottle", "wine glass", "cup", "fork", "knife", "spoon",
    "bowl", "banana", "apple", "sandwich", "orange", "broccoli", "carrot",
    "hot dog", "pizza", "donut", "cake", "chair", "couch", "potted plant",
    "bed", "dining table", "toilet", "tv", "laptop", "mouse", "remote",
    "keyboard", "cell phone", "microwave", "oven", "toaster", "sink",
    "refrigerator", "book", "clock", "vase", "scissors", "teddy bear",
    "hair drier", "toothbrush",
)


class NanoDet:
    """OpenCV DNN wrapper.

    The postprocess is adapted from the OpenCV Zoo NanoDet demo (Apache-2.0).
    It pairs classification and box outputs by shape instead of list order,
    because OpenCV 5 returns them grouped by type rather than alternating.
    """

    def __init__(self, model_path, prob_threshold=0.35, iou_threshold=0.6):
        self.image_shape = (416, 416)
        self.reg_max = 7
        self.prob_threshold = prob_threshold
        self.iou_threshold = iou_threshold
        self.project = np.arange(self.reg_max + 1)
        self.mean = np.array([103.53, 116.28, 123.675], dtype=np.float32).reshape(1, 1, 3)
        self.std = np.array([57.375, 57.12, 58.395], dtype=np.float32).reshape(1, 1, 3)
        self.net = cv2.dnn.readNet(model_path)
        self.net.setPreferableBackend(cv2.dnn.DNN_BACKEND_OPENCV)
        self.net.setPreferableTarget(cv2.dnn.DNN_TARGET_CPU)

    def pre_process(self, img_rgb):
        img = img_rgb.astype(np.float32)
        img = (img - self.mean) / self.std
        return cv2.dnn.blobFromImage(img)

    def infer(self, img_rgb):
        self.net.setInput(self.pre_process(img_rgb))
        outs = self.net.forward(self.net.getUnconnectedOutLayersNames())
        return self.post_process(outs)

    def post_process(self, outs):
        cls = [o for o in outs if o.shape[-1] == 80]
        box = [o for o in outs if o.shape[-1] == 32]
        pairs = []
        for c in cls:
            n = c.shape[1] if c.ndim == 3 else c.shape[0]
            for b in box:
                nb = b.shape[1] if b.ndim == 3 else b.shape[0]
                if n == nb:
                    pairs.append((c, b))
                    break

        boxes_mlvl = []
        scores_mlvl = []
        for cls_score, bbox_pred in pairs:
            if cls_score.ndim == 3:
                cls_score = cls_score.squeeze(axis=0)
            if bbox_pred.ndim == 3:
                bbox_pred = bbox_pred.squeeze(axis=0)
            n = cls_score.shape[0]
            feat = int(round(math.sqrt(n)))
            stride = self.image_shape[1] / feat
            shift_x = np.arange(0, feat) * stride
            shift_y = np.arange(0, feat) * stride
            xv, yv = np.meshgrid(shift_x, shift_y)
            anchors = np.column_stack(
                (xv.flatten() + 0.5 * (stride - 1), yv.flatten() + 0.5 * (stride - 1))
            )

            x_exp = np.exp(bbox_pred.reshape(-1, self.reg_max + 1))
            x_sum = np.sum(x_exp, axis=1, keepdims=True)
            distance = (x_exp / x_sum).dot(self.project).reshape(-1, 4) * stride

            x1 = np.clip(anchors[:, 0] - distance[:, 0], 0, self.image_shape[1])
            y1 = np.clip(anchors[:, 1] - distance[:, 1], 0, self.image_shape[0])
            x2 = np.clip(anchors[:, 0] + distance[:, 2], 0, self.image_shape[1])
            y2 = np.clip(anchors[:, 1] + distance[:, 3], 0, self.image_shape[0])

            boxes_mlvl.append(np.column_stack([x1, y1, x2, y2]))
            scores_mlvl.append(cls_score)

        boxes = np.concatenate(boxes_mlvl, axis=0)
        scores = np.concatenate(scores_mlvl, axis=0)
        wh = boxes.copy()
        wh[:, 2:4] = wh[:, 2:4] - wh[:, 0:2]
        class_ids = np.argmax(scores, axis=1)
        confidences = np.max(scores, axis=1)

        indices = cv2.dnn.NMSBoxes(
            wh.tolist(), confidences.tolist(), self.prob_threshold, self.iou_threshold
        )
        if isinstance(indices, tuple):
            indices = indices[0] if len(indices) else []
        indices = np.array(indices, dtype=int).flatten()
        if indices.size == 0:
            return np.array([])
        return np.concatenate(
            [
                boxes[indices],
                confidences[indices].reshape(-1, 1),
                class_ids[indices].reshape(-1, 1),
            ],
            axis=1,
        )


def letterbox(img, target_size=(416, 416)):
    """Resize with black bars, returning the mapping back to the frame."""
    top, left, newh, neww = 0, 0, target_size[0], target_size[1]
    if img.shape[0] != img.shape[1]:
        hw_scale = img.shape[0] / img.shape[1]
        if hw_scale > 1:
            newh, neww = target_size[0], int(target_size[1] / hw_scale)
            img = cv2.resize(img, (neww, newh), interpolation=cv2.INTER_AREA)
            left = int((target_size[1] - neww) * 0.5)
            img = cv2.copyMakeBorder(
                img, 0, 0, left, target_size[1] - neww - left,
                cv2.BORDER_CONSTANT, value=0,
            )
        else:
            newh, neww = int(target_size[0] * hw_scale), target_size[1]
            img = cv2.resize(img, (neww, newh), interpolation=cv2.INTER_AREA)
            top = int((target_size[0] - newh) * 0.5)
            img = cv2.copyMakeBorder(
                img, top, target_size[0] - newh - top, 0, 0,
                cv2.BORDER_CONSTANT, value=0,
            )
    else:
        img = cv2.resize(img, target_size, interpolation=cv2.INTER_AREA)
    return img, [top, left, newh, neww]


def unletterbox(box, frame_shape, scale):
    h, w = frame_shape
    top, left, newh, neww = scale
    if h == w:
        return box * (h / newh)
    ratioh, ratiow = h / newh, w / neww
    out = box.copy()
    out[0] = max((out[0] - left) * ratiow, 0)
    out[1] = max((out[1] - top) * ratioh, 0)
    out[2] = min((out[2] - left) * ratiow, w)
    out[3] = min((out[3] - top) * ratioh, h)
    return out


def detect(model, frame, conf_threshold, max_items):
    h, w = frame.shape[:2]
    rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
    blob, scale = letterbox(rgb, model.image_shape)
    t0 = time.perf_counter()
    preds = model.infer(blob)
    ms = (time.perf_counter() - t0) * 1000.0

    items = []
    for p in preds:
        score = float(p[4])
        if score < conf_threshold:
            continue
        box = unletterbox(p[:4], (h, w), scale)
        x1 = max(0.0, min(float(box[0]), w))
        y1 = max(0.0, min(float(box[1]), h))
        x2 = max(0.0, min(float(box[2]), w))
        y2 = max(0.0, min(float(box[3]), h))
        bw, bh = x2 - x1, y2 - y1
        if bw < 2.0 or bh < 2.0:
            continue
        cid = int(p[5])
        label = CLASSES[cid] if 0 <= cid < len(CLASSES) else str(cid)
        items.append(
            {
                "c": cid,
                "label": label,
                "p": round(score, 3),
                "x": round(x1 / w, 3),
                "y": round(y1 / h, 3),
                "w": round(bw / w, 3),
                "h": round(bh / h, 3),
            }
        )
    items.sort(key=lambda it: it["p"], reverse=True)
    return items[:max_items], ms


class MjpegReader:
    def __init__(self, url, timeout=5.0):
        self.url = url
        self.timeout = timeout
        self.stream = None
        self.buf = b""

    def _open(self):
        self.stream = urllib.request.urlopen(self.url, timeout=self.timeout)

    def close(self):
        if self.stream is not None:
            try:
                self.stream.close()
            except Exception:
                pass
        self.stream = None
        self.buf = b""

    def read(self):
        if self.stream is None:
            self._open()
        while True:
            a = self.buf.find(b"\xff\xd8")
            b = self.buf.find(b"\xff\xd9", a + 2) if a >= 0 else -1
            if a >= 0 and b >= 0:
                jpg = self.buf[a : b + 2]
                self.buf = self.buf[b + 2 :]
                frame = cv2.imdecode(np.frombuffer(jpg, np.uint8), cv2.IMREAD_COLOR)
                if frame is not None:
                    return frame
                continue
            chunk = self.stream.read(16384)
            if not chunk:
                raise IOError("MJPEG stream ended")
            self.buf += chunk
            if len(self.buf) > 2_000_000:
                self.buf = self.buf[-2:]


def run_service(args):
    import paho.mqtt.client as mqtt

    if not os.path.exists(args.model):
        raise SystemExit(
            f"model not found: {args.model}\n"
            "Run: python3 download_model.py"
        )
    model = NanoDet(args.model, prob_threshold=args.conf)

    client = mqtt.Client(
        mqtt.CallbackAPIVersion.VERSION2, client_id="rover-perception"
    )
    client.connect(args.broker, args.port, 30)
    client.loop_start()
    logging.info("perception: %s -> %s", args.stream, DET_TOPIC)

    reader = MjpegReader(args.stream)
    stop = {"flag": False}

    def on_signal(_signum, _frame):
        stop["flag"] = True

    signal.signal(signal.SIGINT, on_signal)
    signal.signal(signal.SIGTERM, on_signal)

    last_infer = 0.0
    last_log = 0.0
    last_err = 0.0
    fps_ema = 0.0

    while not stop["flag"]:
        try:
            frame = reader.read()
        except Exception as e:
            now = time.monotonic()
            if now - last_err > 1.0:
                last_err = now
                logging.warning("stream: %s", e)
                client.publish(
                    DET_TOPIC,
                    json.dumps({"ok": 0, "reason": "stream", "ts": round(time.time(), 2)}),
                )
            reader.close()
            time.sleep(1.0)
            continue

        now = time.monotonic()
        if now - last_infer < args.interval:
            continue
        dt = now - last_infer if last_infer > 0.0 else 0.0
        last_infer = now

        try:
            items, ms = detect(model, frame, args.conf, args.max_items)
        except Exception as e:
            logging.exception("inference failed: %s", e)
            continue

        if dt > 0.0:
            fps = 1.0 / dt
            fps_ema = fps if fps_ema == 0.0 else 0.8 * fps_ema + 0.2 * fps

        client.publish(
            DET_TOPIC,
            json.dumps(
                {
                    "ok": 1,
                    "w": int(frame.shape[1]),
                    "h": int(frame.shape[0]),
                    "ms": round(ms, 1),
                    "fps": round(fps_ema, 1),
                    "items": items,
                    "ts": round(time.time(), 2),
                }
            ),
        )
        if now - last_log >= 10.0:
            last_log = now
            logging.info(
                "detections=%d ms=%.1f fps=%.1f", len(items), ms, fps_ema
            )

    reader.close()
    client.loop_stop()
    client.disconnect()


def main():
    parser = argparse.ArgumentParser(description="Rover NanoDet perception service")
    parser.add_argument("--selftest", action="store_true", help="run the model on a blank frame and exit")
    parser.add_argument("--image", help="run the model on one image and print the detections")
    parser.add_argument("--model", default=os.environ.get("PERCEPTION_MODEL", DEFAULT_MODEL))
    parser.add_argument("--stream", default=os.environ.get("PERCEPTION_STREAM", DEFAULT_STREAM))
    parser.add_argument(
        "--interval",
        type=float,
        default=float(os.environ.get("PERCEPTION_INTERVAL", "0.20")),
        help="minimum seconds between inferences",
    )
    parser.add_argument(
        "--conf",
        type=float,
        default=float(os.environ.get("PERCEPTION_CONF", "0.35")),
    )
    parser.add_argument(
        "--max-items",
        type=int,
        default=int(os.environ.get("PERCEPTION_MAX_ITEMS", "12")),
    )
    parser.add_argument("--broker", default=os.environ.get("MQTT_BROKER", "127.0.0.1"))
    parser.add_argument("--port", type=int, default=int(os.environ.get("MQTT_PORT", "1883")))
    args = parser.parse_args()

    logging.basicConfig(
        level=logging.INFO,
        format="[perception] %(asctime)s %(levelname)s %(message)s",
    )

    if args.selftest:
        if not os.path.exists(args.model):
            raise SystemExit(f"model not found: {args.model}")
        model = NanoDet(args.model, prob_threshold=args.conf)
        frame = np.zeros((480, 640, 3), dtype=np.uint8)
        items, ms = detect(model, frame, args.conf, args.max_items)
        print(json.dumps({"ok": 1, "ms": round(ms, 1), "items": items}))
        return

    if args.image:
        if not os.path.exists(args.model):
            raise SystemExit(f"model not found: {args.model}")
        frame = cv2.imread(args.image)
        if frame is None:
            raise SystemExit(f"cannot read image: {args.image}")
        model = NanoDet(args.model, prob_threshold=args.conf)
        items, ms = detect(model, frame, args.conf, args.max_items)
        print(
            json.dumps(
                {
                    "ok": 1,
                    "w": int(frame.shape[1]),
                    "h": int(frame.shape[0]),
                    "ms": round(ms, 1),
                    "items": items,
                }
            )
        )
        return

    run_service(args)


if __name__ == "__main__":
    main()
