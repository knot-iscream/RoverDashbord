import json
import os
import time
import threading
from openpyxl import Workbook

CALIBRATION_SESSION_FILE = "calibration_session.jsonl"


class CalibrationManager:
    def __init__(self, mqtt_client=None):
        self.state = "idle"  # idle | warmup | collecting
        self.samples = []
        self.warmup_start_time = 0
        self.collecting_start_time = 0
        self.sample_count = 0
        self.mqtt_client = mqtt_client
        self._lock = threading.Lock()
        self._load_session()

    # ── Session persistence ──────────────────────────────

    def _load_session(self):
        if not os.path.exists(CALIBRATION_SESSION_FILE):
            return
        try:
            with open(CALIBRATION_SESSION_FILE, "r") as f:
                for line in f:
                    line = line.strip()
                    if line:
                        self.samples.append(json.loads(line))
            self.sample_count = len(self.samples)
            print(f"[Calibration] Loaded {self.sample_count} samples from disk")
        except Exception as e:
            print(f"[Calibration] Failed to load session: {e}")

    def _flush(self):
        if self.sample_count == 0:
            return
        try:
            with open(CALIBRATION_SESSION_FILE, "w") as f:
                for s in self.samples:
                    f.write(json.dumps(s) + "\n")
        except Exception as e:
            print(f"[Calibration] Flush error: {e}")

    def _delete_session_file(self):
        if os.path.exists(CALIBRATION_SESSION_FILE):
            try:
                os.remove(CALIBRATION_SESSION_FILE)
            except Exception as e:
                print(f"[Calibration] Failed to delete session: {e}")

    # ── Lifecycle ────────────────────────────────────────

    def start(self):
        with self._lock:
            self.samples = []
            self.sample_count = 0
            self.state = "warmup"
            self.warmup_start_time = time.time()
            self.collecting_start_time = 0
            self._delete_session_file()

        if self.mqtt_client:
            self.mqtt_client.publish("rover/calibration/command",
                                     '{"action":"start"}')

        print("[Calibration] Started — warmup phase (15 min)")
        return {"state": "warmup", "warmup_duration_s": 900}

    def stop(self):
        with self._lock:
            old_state = self.state
            self.state = "idle"
            self._flush()

        if self.mqtt_client:
            self.mqtt_client.publish("rover/calibration/command",
                                     '{"action":"stop"}')

        print(f"[Calibration] Stopped (was {old_state}) — "
              f"{self.sample_count} samples kept")
        return {"state": "idle", "samples_collected": self.sample_count}

    def get_status(self):
        with self._lock:
            now = time.time()
            if self.state == "warmup" and self.warmup_start_time:
                elapsed = now - self.warmup_start_time
            else:
                elapsed = 0

            warmup_pct = 0
            if self.state == "warmup":
                warmup_pct = min(100, round((elapsed / 900) * 100, 1))
            elif self.state == "collecting":
                warmup_pct = 100

            # Auto-transition from warmup to collecting after 900s
            if self.state == "warmup" and elapsed >= 900:
                self.state = "collecting"
                self.collecting_start_time = now
                warmup_pct = 100
                print("[Calibration] Warmup timer expired — auto-transitioned "
                      "to collecting")

            return {
                "state": self.state,
                "warmup_elapsed_s": round(elapsed, 1),
                "warmup_remaining_s": max(0, 900 - round(elapsed, 1)),
                "warmup_pct": warmup_pct,
                "samples_collected": self.sample_count,
            }

    def handle_esp32_status(self, status):
        with self._lock:
            esp_state = status.get("state", "")
            if esp_state == "collecting" and self.state == "warmup":
                self.state = "collecting"
                self.collecting_start_time = time.time()
                print("[Calibration] ESP32 signaled collecting — "
                      "transitioned")
            elif esp_state == "idle" and self.state != "idle":
                self.state = "idle"
                self._flush()
                print("[Calibration] ESP32 signaled idle — stopped")

    # ── Recording ────────────────────────────────────────

    def record_sample(self, data):
        with self._lock:
            if self.state != "collecting":
                return

            sample = {
                "motor_id": data.get("motor", data.get("id", 0)),
                "timestamp": time.time(),
                "temp": data.get("temp", data.get("temperature", 0.0)),
                "voltage": data.get("voltage", 0.0),
                "current": data.get("current", 0.0),
                "vibration": data.get("vibration", 0),
            }
            self.samples.append(sample)
            self.sample_count += 1

            if self.sample_count % 50 == 0:
                self._flush()

    # ── Export ───────────────────────────────────────────

    def export_excel(self, filepath=None):
        with self._lock:
            if not self.samples:
                return None

            if filepath is None:
                ts = int(time.time())
                filepath = f"calibration_export_{ts}.xlsx"

            wb = Workbook()
            ws = wb.active
            ws.title = "Calibration Data"

            headers = ["motor_id", "timestamp", "temp",
                       "voltage", "current", "vibration"]
            ws.append(headers)

            for s in self.samples:
                ws.append([
                    s["motor_id"],
                    s["timestamp"],
                    s["temp"],
                    s["voltage"],
                    s["current"],
                    s["vibration"],
                ])

            wb.save(filepath)
            print(f"[Calibration] Exported {self.sample_count} samples "
                  f"to {filepath}")
            return filepath
