import time
import threading

# Calibration is now a state + status tracker only.
# Telemetry is persisted by HistoryStore for every motor sample;
# this manager follows the ESP32 sweep state machine and relays its
# progress (warmup pct, current speed %, direction, step time, cycle).


class CalibrationManager:
    def __init__(self, mqtt_client=None):
        self.state = "idle"  # idle | warmup | sweep
        self.warmup_start_time = 0
        self.sample_count = 0
        self.mqtt_client = mqtt_client
        self._lock = threading.Lock()

        # Fields relayed from the ESP32 sweep status
        self.speed_pct = 0
        self.direction = 0
        self.step_remaining_s = 0
        self.cycle = 0

    # ── Lifecycle ────────────────────────────────────────

    def start(self):
        with self._lock:
            self.state = "warmup"
            self.warmup_start_time = time.time()
            self.sample_count = 0

        if self.mqtt_client:
            self.mqtt_client.publish("rover/calibration/command",
                                     '{"action":"start"}')

        print("[Calibration] Started — warmup (2 min @255) then sweep 1..100%")
        return {"state": "warmup", "warmup_duration_s": 120}

    def stop(self):
        with self._lock:
            self.state = "idle"
            self.speed_pct = 0
            self.direction = 0
            self.step_remaining_s = 0

        if self.mqtt_client:
            self.mqtt_client.publish("rover/calibration/command",
                                     '{"action":"stop"}')

        print(f"[Calibration] Stopped — {self.sample_count} samples kept")
        return {"state": "idle", "samples_collected": self.sample_count}

    def count_sample(self):
        with self._lock:
            self.sample_count += 1

    def get_status(self):
        with self._lock:
            now = time.time()
            elapsed = 0
            warmup_pct = 0

            if self.state == "warmup" and self.warmup_start_time:
                elapsed = now - self.warmup_start_time
                warmup_pct = min(100, round((elapsed / 120) * 100, 1))
                if elapsed >= 120:
                    self.state = "sweep"
                    warmup_pct = 100

            return {
                "state": self.state,
                "warmup_pct": warmup_pct,
                "warmup_remaining_s": max(0, 120 - round(elapsed, 1)),
                "samples_collected": self.sample_count,
                "speed_pct": self.speed_pct,
                "direction": self.direction,
                "step_remaining_s": self.step_remaining_s,
                "cycle": self.cycle,
            }

    def handle_esp32_status(self, status):
        with self._lock:
            esp_state = status.get("state", "")
            if esp_state == "sweep":
                self.state = "sweep"
                self.speed_pct = status.get("speed_pct", self.speed_pct)
                self.direction = status.get("dir", self.direction)
                self.step_remaining_s = status.get("step_remaining_s",
                                                   self.step_remaining_s)
                self.cycle = status.get("cycle", self.cycle)
            elif esp_state == "warmup":
                self.state = "warmup"
            elif esp_state == "idle" and self.state != "idle":
                self.state = "idle"
                self.speed_pct = 0
                self.direction = 0
                print("[Calibration] ESP32 signaled idle — stopped")