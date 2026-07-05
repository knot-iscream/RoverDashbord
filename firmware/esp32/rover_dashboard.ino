#include "config.h"
#include "sensors.h"
#include "mqtt_comms.h"

SensorManager sensors;
MQTTManager mqtt;

// Pin assignments for 4 motors
int dht_pins[4] = {DHT11_PIN_1, DHT11_PIN_2, DHT11_PIN_3, DHT11_PIN_4};
int vib_pins[4] = {SW420_PIN_1, SW420_PIN_2, SW420_PIN_3, SW420_PIN_4};
uint8_t ina_addrs[4] = {INA219_ADDR_1, INA219_ADDR_2, INA219_ADDR_3, INA219_ADDR_4};

unsigned long last_sample = 0;
int current_motor = 0;

// Calibration state machine
enum CalState { CAL_IDLE, CAL_WARMUP, CAL_COLLECTING };
CalState cal_state = CAL_IDLE;
unsigned long cal_state_start = 0;
unsigned long cal_last_status = 0;

void onCalibrationCommand(const char* action) {
  if (strcmp(action, "start") == 0 && cal_state == CAL_IDLE) {
    cal_state = CAL_WARMUP;
    cal_state_start = millis();
    cal_last_status = 0;
    Serial.println("[Cal] Warmup started (15 min)");
  } else if (strcmp(action, "stop") == 0 && cal_state != CAL_IDLE) {
    cal_state = CAL_IDLE;
    Serial.println("[Cal] Stopped");
    mqtt.publishCalibrationStatus(0, "idle");
  }
}

void updateCalibration() {
  unsigned long now = millis();

  if (cal_state == CAL_WARMUP) {
    unsigned long elapsed = now - cal_state_start;
    int pct = (int)((elapsed * 100) / CALIB_WARMUP_MS);

    if (elapsed >= CALIB_WARMUP_MS) {
      cal_state = CAL_COLLECTING;
      cal_state_start = now;
      Serial.println("[Cal] Warmup done — collecting data");
      mqtt.publishCalibrationStatus(100, "collecting");
    } else if (now - cal_last_status > 5000) {
      cal_last_status = now;
      mqtt.publishCalibrationStatus(pct, "warmup");
    }
  }
}

void setup() {
  Serial.begin(115200);
  delay(500);
  Serial.println("\n===== Rover Digital Twin - ESP32 =====");

  sensors.begin(dht_pins, vib_pins, ina_addrs);

  mqtt.begin(WIFI_SSID, WIFI_PASSWORD, MQTT_BROKER, MQTT_PORT, MQTT_TOPIC_PREFIX);
  mqtt.connectMQTT();
  mqtt.subscribe(MQTT_CALIB_CMD_TOPIC);
  mqtt.setCommandHandler(onCalibrationCommand);

  Serial.println("[Setup] Ready");
}

void loop() {
  mqtt.loop();
  updateCalibration();

  unsigned long now = millis();
  if (now - last_sample >= SAMPLE_INTERVAL_MS) {
    last_sample = now;

    MotorSensorData data = sensors.readMotor(current_motor);

    mqtt.publishMotorData(
      data.motor_id, data.vibration,
      data.voltage, data.current, data.temperature,
      cal_state == CAL_COLLECTING
    );

    current_motor = (current_motor + 1) % 4;
  }
}
