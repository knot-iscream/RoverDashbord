/*
 * Rover Digital Twin — ESP32 Firmware
 *
 * Reads 4 motors: DHT11 (temp), INA219 (voltage/current), SW-420 (vibration)
 * Publishes data via MQTT every 250ms per motor.
 *
 * Topic: rover/motor/{1..4}
 * Payload: {"motor":1, "vibration":0, "voltage":12.3, "current":0.65, "temp":38.2}
 */

#include "config.h"
#include "sensors.h"
#include "mqtt_comms.h"

SensorManager sensors;
MQTTManager mqtt;

// Pin assignments for 4 motors
int dht_pins[4] = {DH11_PIN_1, DH11_PIN_2, DH11_PIN_3, DH11_PIN_4};
int vib_pins[4] = {SW420_PIN_1, SW420_PIN_2, SW420_PIN_3, SW420_PIN_4};
uint8_t ina_addrs[4] = {INA219_ADDR_1, INA219_ADDR_2, INA219_ADDR_3, INA219_ADDR_4};

unsigned long last_sample = 0;
int current_motor = 0;

void setup() {
  Serial.begin(115200);
  delay(500);
  Serial.println("\n===== Rover Digital Twin - ESP32 =====");

  sensors.begin(dht_pins, vib_pins, ina_addrs);

  mqtt.begin(WIFI_SSID, WIFI_PASSWORD, MQTT_BROKER, MQTT_PORT, MQTT_TOPIC_PREFIX);
  mqtt.connectMQTT();

  Serial.println("[Setup] Ready");
}

void loop() {
  mqtt.loop();

  unsigned long now = millis();
  if (now - last_sample >= SAMPLE_INTERVAL_MS) {
    last_sample = now;

    // Read one motor per cycle (cycles through 1..4)
    MotorSensorData data = sensors.readMotor(current_motor);

    mqtt.publishMotorData(
      data.motor_id,
      data.vibration,
      data.voltage,
      data.current,
      data.temperature
    );

    current_motor = (current_motor + 1) % 4;
  }
}
