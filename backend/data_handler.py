import json
import os
import time

CALIBRATION_FILE = "calibration_baseline.json"


class DataHandler:
    def __init__(self):
        self.latest_data = {}
        self.baselines = {}
        self.load_calibration()

    def process_motor_data(self, data):
        motor_id = data.get("motor", data.get("id", 0))
        self.latest_data[motor_id] = {
            "id": motor_id,
            "vibration": data.get("vibration", 0),
            "voltage": data.get("voltage", 0.0),
            "current": data.get("current", 0.0),
            "temp": data.get("temp", data.get("temperature", 0.0)),
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

        temp = data.get("temp", 35)
        voltage = data.get("voltage", 12)
        current = data.get("current", 0.5)
        vibration = data.get("vibration", 0)

        # Temperature penalty
        if temp > 50:
            penalty = (temp - 50) * 2
            health -= penalty
            anomalies.append(f"High temperature: {temp:.1f}°C")

        # Voltage penalty
        if voltage < 11.5:
            penalty = (11.5 - voltage) * 10
            health -= penalty
            anomalies.append(f"Low voltage: {voltage:.2f}V")

        # Current penalty (overload)
        if current > 2.0:
            penalty = (current - 2.0) * 20
            health -= penalty
            anomalies.append(f"Over-current: {current:.3f}A")

        # Vibration penalty
        if vibration:
            health -= 5
            anomalies.append("Vibration spike detected")

        # Compare to baseline if available
        if baseline:
            if temp > baseline.get("avg_temp", 35) + 10:
                health -= 3
            if voltage < baseline.get("avg_voltage", 12) * 0.9:
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
