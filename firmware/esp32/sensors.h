#ifndef SENSORS_H
#define SENSORS_H

#include <OneWire.h>
#include <DallasTemperature.h>
#include <Adafruit_INA219.h>
#include "config.h"

#define NUM_MOTORS 4
#define DS18B20_CONVERSION_MS 750

struct MotorSensorData {
  int motor_id;
  bool vibration;
  float voltage;
  float current;
  float temperature;
};

class SensorManager {
  private:
    OneWire* one_wire;
    DallasTemperature* ds18b20;
    DeviceAddress temp_addresses[NUM_MOTORS];
    int temp_count;              // number of DS18B20 found
    float temp_cache[NUM_MOTORS];// last good temps
    bool conversion_pending;
    unsigned long conversion_start;

    Adafruit_INA219* ina_sensors[NUM_MOTORS];
    int vib_pins[NUM_MOTORS];
    uint8_t ina_addresses[NUM_MOTORS];

    void requestConversion() {
      if (ds18b20->requestTemperatures()) {
        conversion_pending = true;
        conversion_start = millis();
      }
    }

  public:
    SensorManager() : one_wire(nullptr), ds18b20(nullptr),
                      temp_count(0), conversion_pending(false),
                      conversion_start(0) {
      for (int i = 0; i < NUM_MOTORS; i++) {
        temp_cache[i] = 25.0f;
      }
    }

    void begin(int vib_pins_arr[NUM_MOTORS], uint8_t ina_addrs[NUM_MOTORS]) {
      for (int i = 0; i < NUM_MOTORS; i++) {
        vib_pins[i] = vib_pins_arr[i];
        ina_addresses[i] = ina_addrs[i];

        pinMode(vib_pins[i], INPUT);

        ina_sensors[i] = new Adafruit_INA219(ina_addresses[i]);
        if (!ina_sensors[i]->begin()) {
          Serial.print("[Sensors] INA219 on addr 0x");
          Serial.print(ina_addresses[i], HEX);
          Serial.println(" not found!");
        } else {
          Serial.print("[Sensors] INA219 on addr 0x");
          Serial.print(ina_addresses[i], HEX);
          Serial.println(" initialized");
        }
      }

      // DS18B20 — all probes share one OneWire bus
      one_wire = new OneWire(DS18B20_DATA_PIN);
      ds18b20 = new DallasTemperature(one_wire);
      ds18b20->begin();

      temp_count = ds18b20->getDeviceCount();
      Serial.print("[Sensors] DS18B20 probes found: ");
      Serial.println(temp_count);

      for (int i = 0; i < temp_count && i < NUM_MOTORS; i++) {
        if (ds18b20->getAddress(temp_addresses[i], i)) {
          ds18b20->setResolution(temp_addresses[i], DS18B20_RESOLUTION);
        }
      }
      if (temp_count < NUM_MOTORS) {
        Serial.print("[Sensors] WARNING: expected ");
        Serial.print(NUM_MOTORS);
        Serial.println(" DS18B20, check wiring/pull-up");
      }

      // Kick off first conversion cycle
      requestConversion();
    }

    // Call often from loop(). Drives the async DS18B20 conversion:
    // request -> wait ~750ms -> read all into cache -> request again.
    void update() {
      if (!conversion_pending) {
        requestConversion();
        return;
      }
      if (millis() - conversion_start < DS18B20_CONVERSION_MS) {
        return;
      }

      for (int i = 0; i < temp_count && i < NUM_MOTORS; i++) {
        float t = ds18b20->getTempC(temp_addresses[i]);
        if (t > -50.0f && t < 150.0f) {   // valid reading
          temp_cache[i] = t;
        }
      }
      conversion_pending = false;
    }

    MotorSensorData readMotor(int motor_idx) {
      MotorSensorData data;
      data.motor_id = motor_idx + 1;

      // Vibration — digital read from SW-420 (NC = normally closed)
      // SW-420 outputs LOW when vibration detected
      data.vibration = (digitalRead(vib_pins[motor_idx]) == LOW);

      // Temperature from DS18B20 cache (falls back to last good value)
      data.temperature = temp_cache[motor_idx];

      // Voltage and current from INA219
      float shunt_v = ina_sensors[motor_idx]->getShuntVoltage_mV();
      float bus_v = ina_sensors[motor_idx]->getBusVoltage_V();
      float current_mA = ina_sensors[motor_idx]->getCurrent_mA();

      data.voltage = bus_v + (shunt_v / 1000.0);
      data.current = current_mA / 1000.0;

      if (data.voltage < 0.1) data.voltage = 12.0;  // fallback
      if (data.current < 0) data.current = 0;

      return data;
    }
};

#endif