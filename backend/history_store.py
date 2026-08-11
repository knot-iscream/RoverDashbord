import json
import os
import threading
import time
from datetime import datetime

from openpyxl import Workbook

HISTORY_DIR = "history"
GAP_SECONDS = 5.0          # max gap within one contiguous segment


def _day_str(ts):
    return datetime.fromtimestamp(ts).strftime("%Y-%m-%d")


def _day_path(day):
    return os.path.join(HISTORY_DIR, f"history_{day}.jsonl")


def _num(value, default=0.0):
    """Coerce a JSON value (may be None/NaN from ESP32) into a float."""
    try:
        v = float(value)
        if v != v:   # NaN
            return default
        return v
    except (TypeError, ValueError):
        return default


class HistoryStore:
    def __init__(self, history_dir=None):
        self.history_dir = history_dir or HISTORY_DIR
        self._lock = threading.Lock()
        os.makedirs(self.history_dir, exist_ok=True)

    # ── Writing ──────────────────────────────────────────

    def add_sample(self, data):
        """Append one motor sample to its day-sharded JSONL file."""
        try:
            now = time.time()
            sample = {
                "ts": now,
                "iso": datetime.fromtimestamp(now).strftime("%Y-%m-%d %H:%M:%S"),
                "motor": data.get("motor", data.get("id", 0)),
                "speed": int(data.get("speed", 0) or 0),
                "temp": _num(data.get("temp", data.get("temperature"))),
                "voltage": _num(data.get("voltage")),
                "current": _num(data.get("current")),
                "vibration": 1 if data.get("vibration") else 0,
            }
            with self._lock:
                with open(_day_path(_day_str(now)), "a") as f:
                    f.write(json.dumps(sample) + "\n")
        except Exception as e:
            print(f"[History] add_sample error: {e}")

    # ── Read / query ─────────────────────────────────────

    def days(self):
        """Return the list of day strings (desc) that have data."""
        if not os.path.isdir(self.history_dir):
            return []
        out = []
        for name in sorted(os.listdir(self.history_dir)):
            if name.startswith("history_") and name.endswith(".jsonl"):
                out.append(name[len("history_"):-len(".jsonl")])
        return list(reversed(out))

    def _read_day(self, day):
        path = _day_path(day)
        if not os.path.exists(path):
            return []
        rows = []
        with open(path, "r") as f:
            for line in f:
                line = line.strip()
                if line:
                    try:
                        rows.append(json.loads(line))
                    except Exception:
                        pass
        return rows

    def segments(self, day):
        """Aggregate a day's samples into `(motor, direction, speed)` segments."""
        rows = self._read_day(day)
        if not rows:
            return []

        segments = []
        cur = None

        for r in rows:
            ts = r.get("ts", 0.0)
            motor = r.get("motor", 0)
            speed = int(r.get("speed", 0) or 0)
            if speed > 0:
                direction = "FWD"
            elif speed < 0:
                direction = "REV"
            else:
                direction = "IDLE"

            temp = _num(r.get("temp"))
            voltage = _num(r.get("voltage"))
            current = _num(r.get("current"))
            vib = int(r.get("vibration", 0) or 0)

            new_seg = (
                cur is None
                or cur["motor"] != motor
                or cur["direction"] != direction
                or cur["speed"] != speed
                or (ts - cur["end_ts"]) > GAP_SECONDS
            )

            if new_seg:
                cur = {
                    "motor": motor, "speed": speed, "direction": direction,
                    "start_ts": ts, "end_ts": ts,
                    "sum_temp": 0.0, "max_temp": None,
                    "sum_voltage": 0.0, "sum_current": 0.0,
                    "vib_count": 0, "samples": 0,
                }
                segments.append(cur)

            cur["end_ts"] = ts
            cur["samples"] += 1
            cur["sum_temp"] += temp
            cur["sum_voltage"] += voltage
            cur["sum_current"] += current
            cur["vib_count"] += vib
            if cur["max_temp"] is None or temp > cur["max_temp"]:
                cur["max_temp"] = temp

        out = []
        for s in segments:
            n = max(1, s["samples"])
            speed_pct = round(abs(s["speed"]) / 255.0 * 100)
            out.append({
                "start_ts": round(s["start_ts"], 2),
                "start_iso": datetime.fromtimestamp(s["start_ts"]).strftime("%H:%M:%S"),
                "duration_s": round(s["end_ts"] - s["start_ts"], 1),
                "motor": s["motor"],
                "speed": s["speed"],
                "speed_pct": speed_pct,
                "direction": s["direction"],
                "avg_temp": round(s["sum_temp"] / n, 2),
                "max_temp": round(s["max_temp"], 2),
                "avg_voltage": round(s["sum_voltage"] / n, 2),
                "avg_current": round(s["sum_current"] / n, 2),
                "vibration_count": s["vib_count"],
                "samples": s["samples"],
            })
        return out

    # ── Export ───────────────────────────────────────────

    def export_excel(self, filepath=None):
        """Export ALL recorded telemetry to an Excel workbook."""
        rows = []
        for day in self.days():
            rows.extend(self._read_day(day))
        if not rows:
            return None

        if filepath is None:
            ts = int(time.time())
            filepath = f"history_export_{ts}.xlsx"

        wb = Workbook()
        ws = wb.active
        ws.title = "Telemetry"

        ws.append(["motor_id", "epoch", "timestamp", "speed",
                   "speed_pct", "direction", "temp",
                   "voltage", "current", "vibration"])

        for r in rows:
            speed = int(r.get("speed", 0) or 0)
            direction = "FWD" if speed > 0 else ("REV" if speed < 0 else "IDLE")
            speed_pct = round(abs(speed) / 255.0 * 100)
            ws.append([
                r.get("motor", 0),
                r.get("ts", r.get("timestamp", "")),
                r.get("iso", r.get("timestamp", "")),
                speed,
                speed_pct,
                direction,
                r.get("temp", 0.0),
                r.get("voltage", 0.0),
                r.get("current", 0.0),
                r.get("vibration", 0),
            ])

        wb.save(filepath)
        print(f"[History] Exported {len(rows)} samples to {filepath}")
        return filepath