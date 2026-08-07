#include "config.h"
#include "sensors.h"
#include "mqtt_comms.h"
#include "motor_driver.h"

SensorManager sensors;
MQTTManager mqtt;
MotorDriver driver;

// Pin assignments
int vib_pins[4] = {SW420_PIN_1, SW420_PIN_2, SW420_PIN_3, SW420_PIN_4};
uint8_t ina_addrs[4] = {INA219_ADDR_1, INA219_ADDR_2, INA219_ADDR_3, INA219_ADDR_4};

unsigned long last_sample = 0;
int current_motor = 0;
unsigned long last_status_pub = 0;

// Calibration state machine
enum CalState { CAL_IDLE, CAL_WARMUP, CAL_SWEEP };
CalState cal_state = CAL_IDLE;
unsigned long cal_state_start = 0;
unsigned long cal_last_status = 0;
int cal_warmup_half = 0;   // 0 = +255, 1 = -255
int cal_pct = 1;            // current sweep speed % (1..100)
int cal_dir = 1;            // +1 fwd, -1 rev
int cal_cycle = 1;          // full-sweep loop counter

int dutyForPct(int pct) {
  return (255 * pct + 50) / 100;   // round(255*pct/100)
}

void publishCal() {
  unsigned long elapsed = millis() - cal_state_start;
  int warm_pct = 0;
  int speed_pct = 0;
  int dir = 0;
  int rem = 0;
  int cycle = cal_cycle;
  const char* state = "idle";

  if (cal_state == CAL_WARMUP) {
    state = "warmup";
    warm_pct = (int)((elapsed * 100) / CALIB_WARMUP_MS);
    if (warm_pct > 100) warm_pct = 100;
    rem = (int)((CALIB_WARMUP_MS - elapsed) / 1000);
    dir = ((elapsed / CALIB_HALF_STEP_MS) % 2 == 0) ? 1 : -1;
    speed_pct = 100;
  } else if (cal_state == CAL_SWEEP) {
    state = "sweep";
    long within = elapsed % CALIB_SWEEP_STEP_MS;
    speed_pct = cal_pct;
    dir = cal_dir;
    rem = (int)((CALIB_SWEEP_STEP_MS - within) / 1000);
  }
  mqtt.publishCalibrationStatus(warm_pct, state, speed_pct, dir, rem, cycle);
}

void onCalibrationCommand(const char* action) {
  if (strcmp(action, "start") == 0 && cal_state == CAL_IDLE) {
    cal_state = CAL_WARMUP;
    cal_state_start = millis();
    cal_last_status = 0;
    cal_warmup_half = 0;
    cal_pct = 1;
    cal_dir = 1;
    cal_cycle = 1;
    driver.runAll(255);
    Serial.println("[Cal] Warmup started (2 min) — +255");
    publishCal();
  } else if (strcmp(action, "stop") == 0 && cal_state != CAL_IDLE) {
    cal_state = CAL_IDLE;
    driver.stopAll();
    Serial.println("[Cal] Stopped — motors idle");
    mqtt.publishCalibrationStatus(0, "idle", 0, 0, 0, 0);
  }
}

void onMotorCommand(const char* json) {
  if (cal_state != CAL_IDLE) {
    Serial.println("[Motor] Ignoring command during calibration");
    return;
  }
  driver.applyCommand(json);
}

void updateCalibration() {
  unsigned long now = millis();
  unsigned long elapsed = now - cal_state_start;

  if (cal_state == CAL_WARMUP) {
    int half = (int)(elapsed / CALIB_HALF_STEP_MS) % 2;
    if (half != cal_warmup_half) {
      cal_warmup_half = half;
      driver.runAll(half == 0 ? 255 : -255);
    }
    if (elapsed >= CALIB_WARMUP_MS) {
      cal_state = CAL_SWEEP;
      cal_state_start = now;
      cal_pct = 1;
      cal_dir = 1;
      driver.runAll(dutyForPct(1));
      Serial.println("[Cal] Warmup done — sweeping 1..100% (+/-)");
    } else if (now - cal_last_status >= 2000) {
      cal_last_status = now;
      publishCal();
    }
  } else if (cal_state == CAL_SWEEP) {
    long pct_raw = elapsed / CALIB_SWEEP_STEP_MS;
    int pct = (int)(pct_raw % 100) + 1;
    int dir = ((elapsed % CALIB_SWEEP_STEP_MS) / CALIB_HALF_STEP_MS) == 0 ? 1 : -1;
    int cycle = (int)(pct_raw / 100) + 1;

    if (pct != cal_pct || dir != cal_dir) {
      cal_pct = pct;
      cal_dir = dir;
      cal_cycle = cycle;
      driver.runAll(cal_dir * dutyForPct(cal_pct));
      Serial.printf("[Cal] Sweep %d%% %s (cycle %d)\n",
                    cal_pct, dir > 0 ? "FWD" : "REV", cal_cycle);
    }
    if (now - cal_last_status >= 2000) {
      cal_last_status = now;
      publishCal();
    }
  }
}

void setup() {
  Serial.begin(115200);
  delay(500);
  Serial.println("\n===== Rover Digital Twin - ESP32 =====");

  sensors.begin(vib_pins, ina_addrs);
  driver.begin();

  mqtt.begin(WIFI_SSID, WIFI_PASSWORD, MQTT_BROKER, MQTT_PORT, MQTT_TOPIC_PREFIX);
  mqtt.connectMQTT();
  mqtt.subscribe(MQTT_CALIB_CMD_TOPIC);
  mqtt.subscribe(MQTT_MOTOR_CMD_TOPIC);
  mqtt.setCommandHandler(onCalibrationCommand);
  mqtt.setMotorCommandHandler(onMotorCommand);

  Serial.println("[Setup] Ready");
}

void loop() {
  mqtt.loop();
  updateCalibration();
  sensors.update();   // async DS18B20 conversion

  unsigned long now = millis();
  if (now - last_sample >= SAMPLE_INTERVAL_MS) {
    last_sample = now;

    MotorSensorData data = sensors.readMotor(current_motor);

    mqtt.publishMotorData(
      data.motor_id, data.vibration,
      data.voltage, data.current, data.temperature,
      driver.getSpeed(current_motor)
    );

    current_motor = (current_motor + 1) % 4;
  }

  // Publish motor status periodically (~2s)
  if (now - last_status_pub >= 2000) {
    last_status_pub = now;
    int speeds[4] = {
      driver.getSpeed(0), driver.getSpeed(1),
      driver.getSpeed(2), driver.getSpeed(3)
    };
    mqtt.publishMotorStatus(cal_state != CAL_IDLE, speeds);
  }
}