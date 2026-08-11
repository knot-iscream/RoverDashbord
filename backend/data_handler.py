import json
import os
import time

CALIBRATION_FILE = "calibration_baseline.json"


def _num(value, default=0.0):
    """Coerce a JSON value (may be None/NaN from ESP32) into a float.
    Returns default for missing/NaN — used only for health math."""
    try:
        v = float(value)
        if v != v:   # NaN
            return default
        return v
    except (TypeError, ValueError):
        return default


def _raw(value):
    """Preserve the raw sensor value for display. NaN/None -> None so the
    WS snapshot carries null (pages render '--') instead of a fake 0.0."""
    if value is None:
        return None
    try:
        v = float(value)
    except (TypeError, ValueError):
        return None
    return None if v != v else v


class DataHandler:
    def __init__(self):
        self.latest_data = {}
        self.baselines = {}
        self.load_calibration()

    def process_motor_data(self, data):
        motor_id = data.get("motor", data.get("id", 0))

        # Sensor validity flags. A flag explicitly set to false (from ESP32)
        # means that sensor is NOT trustworthy. Missing flag (old firmware /
        # unplugged module) is ALSO treated as not valid — a packet that
        # doesn't vouch for its sensor is never trusted.
        vibration_valid = bool(data.get(
            "vibration_valid", data.get("vib_valid", False)))
        ina_ok = bool(data.get("ina_ok", False))
        temp_valid = bool(data.get("temp_valid", False))

        self.latest_data[motor_id] = {
            "id": motor_id,
            "vibration": 1 if data.get("vibration") else 0,
            "vibration_valid": vibration_valid,
            "ina_ok": ina_ok,
            "temp_valid": temp_valid,
            "voltage": _raw(data.get("voltage")),
            "current": _raw(data.get("current")),
            "temp": _raw(data.get("temp", data.get("temperature"))),
            "timestamp": time.time(),
        }
        return self.compute_health(motor_id)

    def compute_health(self, motor_id):
        data = self.latest_data.get(motor_id)
        if not data:
            return {"health": 100, "anomalies": []}

        baseline = self.baselines.get(motor_id)
        health = 100.0
        anomalies = []

        temp = _num(data.get("temp"), 35)
        voltage = _num(data.get("voltage"), 12)
        current = _num(data.get("current"), 0.5)
        vibration = data.get("vibration", 0)
        ina_ok = data.get("ina_ok", True)
        temp_valid = data.get("temp_valid", True)
        vibration_valid = data.get("vibration_valid", True)

        # If NO sensor is trustworthy (INA219 missing, no temp probe, no
        # wired vibration module) the number would be made-up 100% — report
        # Nothing so the UI can show "--".
        if not (ina_ok or temp_valid or vibration_valid):
            return {"health": None, "anomalies": []}

        # Each penalty only applies when that sensor is actually valid —
        # never penalize health from a missing/unplugged sensor.
        if temp_valid:
            if temp > 50:
                penalty = (temp - 50) * 2
                health -= penalty
                anomalies.append(f"High temperature: {temp:.1f}°C")

        if ina_ok:
            if voltage < 11.5:
                penalty = (11.5 - voltage) * 10
                health -= penalty
                anomalies.append(f"Low voltage: {voltage:.2f}V")

            if current > 2.0:
                penalty = (current - 2.0) * 20
                health -= penalty
                anomalies.append(f"Over-current: {current:.3f}A")

        if vibration_valid:
            if vibration:
                health -= 5
                anomalies.append("Vibration spike detected")

        # Compare to baseline if available
        if baseline:
            if temp_valid and temp > baseline.get("avg_temp", 35) + 10:
                health -= 3
            if ina_ok and voltage < baseline.get("avg_voltage", 12) * 0.9:
                health -= 3

        health = max(0, min(100, health))
        return {"health": round(health, 1), "anomalies": anomalies}

    def set_baseline(self, motor_id, baseline_data):
        self.baselines[motor_id] = baseline_data
        self.save_calibration()

    def get_latest(self):
        return list(self.latest_data.values())

    def load_calibration(self):
        if os.path.exists(CALIBRATION_FILE):
            try:
                with open(CALIBRATION_FILE, "r") as f:
                    self.baselines = json.load(f)
                print(f"[DataHandler] Loaded calibration for "
                      f"{len(self.baselines)} motors")
            except Exception as e:
                print(f"[DataHandler] Failed to load calibration: {e}")

    def save_calibration(self):
        try:
            with open(CALIBRATION_FILE, "w") as f:
                json.dump(self.baselines, f, indent=2)
            print("[DataHandler] Calibration saved")
        except Exception as e:
            print(f"[DataHandler] Failed to save calibration: {e}")
