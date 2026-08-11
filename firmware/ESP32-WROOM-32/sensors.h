#ifndef SENSORS_H
#define SENSORS_H

#include <OneWire.h>
#include <DallasTemperature.h>
#include <Adafruit_INA219.h>
#include "config.h"

#define NUM_MOTORS 4
#define DS18B20_CONVERSION_MS 750

// A vibration reading is only trusted if the SW-420 pin has been observed
// HIGH within this window. A wired module pulls DO HIGH when quiet; a
// disconnected/floating input-only GPIO (34-39, no internal pull-up) reads
// LOW forever -> vibration_valid goes false and vibration is reported as 0.
#define VIB_VALID_WINDOW_MS 10000

struct MotorSensorData {
  int motor_id;
  bool vibration;
  bool vibration_valid;
  bool ina_ok;
  float voltage;
  float current;
  float temperature;
  bool temp_valid;
};

class SensorManager {
  private:
    OneWire* one_wire;
    DallasTemperature* ds18b20;
    DeviceAddress temp_addresses[NUM_MOTORS];
    int temp_count;              // number of DS18B20 found
    float temp_cache[NUM_MOTORS];// last good temps
    bool temp_valid[NUM_MOTORS]; // true only when a probe was found and read
    bool conversion_pending;
    unsigned long conversion_start;

    Adafruit_INA219* ina_sensors[NUM_MOTORS];
    int vib_pins[NUM_MOTORS];
    uint8_t ina_addresses[NUM_MOTORS];
    bool ina_ok[NUM_MOTORS];
    unsigned long vib_last_high[NUM_MOTORS];
    unsigned long last_ina_check;   // last time INA219 presence was re-verified
    static const unsigned long INA_RECHECK_MS = 5000;

    void requestConversion() {
      if (ds18b20->requestTemperatures()) {
        conversion_pending = true;
        conversion_start = millis();
      }
    }

    // Re-run begin() on each INA219 so a chip unplugged AFTER boot is
    // detected (begin() fails when the chip vanishes from the I2C bus).
    void recheckIna() {
      for (int i = 0; i < NUM_MOTORS; i++) {
        if (ina_sensors[i]) {
          ina_ok[i] = ina_sensors[i]->begin();
        }
      }
    }

  public:
    SensorManager() : one_wire(nullptr), ds18b20(nullptr),
                      temp_count(0), conversion_pending(false),
                      conversion_start(0), last_ina_check(0) {
      for (int i = 0; i < NUM_MOTORS; i++) {
        temp_cache[i] = NAN;
        temp_valid[i] = false;
        vib_last_high[i] = 0;
        ina_ok[i] = false;
      }
    }

    void begin(int vib_pins_arr[NUM_MOTORS], uint8_t ina_addrs[NUM_MOTORS]) {
      for (int i = 0; i < NUM_MOTORS; i++) {
        vib_pins[i] = vib_pins_arr[i];
        ina_addresses[i] = ina_addrs[i];

        pinMode(vib_pins[i], INPUT);

        ina_sensors[i] = new Adafruit_INA219(ina_addresses[i]);
        ina_ok[i] = ina_sensors[i]->begin();
        if (!ina_ok[i]) {
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
      // Periodically re-verify INA219 presence (detects unplug mid-run).
      if (millis() - last_ina_check >= INA_RECHECK_MS) {
        last_ina_check = millis();
        recheckIna();
      }

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
          temp_valid[i] = true;
        } else {
          // Probe unplugged/broken (-127 = DEVICE_DISCONNECTED). Flag the
          // reading invalid so the backend/UI show "--" instead of the last
          // cached value. Never clear the cache — only the validity.
          temp_valid[i] = false;
        }
      }
      conversion_pending = false;
    }

    MotorSensorData readMotor(int motor_idx) {
      MotorSensorData data;
      data.motor_id = motor_idx + 1;

      // Vibration — digital read from SW-420 (NC = normally closed)
      // SW-420 outputs LOW when vibration detected. Only trusted if the
      // pin has read HIGH recently (proves the module is wired); a floating
      // disconnected pin reads LOW forever and must NOT count as vibration.
      bool vib_pin_low = (digitalRead(vib_pins[motor_idx]) == LOW);
      unsigned long now = millis();
      if (!vib_pin_low) {
        vib_last_high[motor_idx] = now;
      }
      data.vibration_valid = (vib_last_high[motor_idx] != 0) &&
                             (now - vib_last_high[motor_idx]) < VIB_VALID_WINDOW_MS;
      data.vibration = data.vibration_valid && vib_pin_low;

      // Temperature from DS18B20 cache (only valid if a probe was found+read
      // — otherwise NAN so we NEVER publish a fake 25 C idle value)
      data.temperature = temp_cache[motor_idx];
      data.temp_valid = temp_valid[motor_idx];

      // Voltage and current from INA219. Only trusted when the chip is
      // present (ina_ok) AND producing sane numbers. A chip that begins
      // but reports NaN current, or bus voltage way out of range, means no
      // shunt/load is actually wired — treat the whole reading as
      // unreliable (report NAN so the backend/UI render "--").
      data.ina_ok = ina_ok[motor_idx];
      float shunt_v = 0.0f, bus_v = 0.0f, current_mA = 0.0f;
      if (data.ina_ok) {
        shunt_v = ina_sensors[motor_idx]->getShuntVoltage_mV();
        bus_v = ina_sensors[motor_idx]->getBusVoltage_V();
        current_mA = ina_sensors[motor_idx]->getCurrent_mA();
      }
      bool bad_current = (current_mA != current_mA);   // NaN
      bool bad_bus = !(bus_v > 3.0f && bus_v < 16.0f); // out of sane range
      if (data.ina_ok && (bad_current || bad_bus)) {
        data.ina_ok = false;
      }

      data.voltage = data.ina_ok ? (bus_v + (shunt_v / 1000.0)) : NAN;
      data.current = data.ina_ok ? (current_mA / 1000.0) : NAN;
      if (data.voltage != data.voltage) data.voltage = NAN;
      if (data.current != data.current) data.current = NAN;

      return data;
    }
};

#endif