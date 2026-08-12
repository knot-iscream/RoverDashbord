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

// One dedicated OneWire bus per motor (pin = motor slot). Indexing by motor
// means no ROM matching: whichever probe is on that pin reports as that motor.
// Pins set to -1 in config.h are disabled (published as temp_valid:0).
static const int DS18B20_PINS[NUM_MOTORS] = {
  DS18B20_PIN_1, DS18B20_PIN_2, DS18B20_PIN_3, DS18B20_PIN_4
};

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
    OneWire* one_wire[NUM_MOTORS];          // one bus instance per motor
    DallasTemperature* ds18b20[NUM_MOTORS]; // one chip manager per motor
    DeviceAddress temp_addresses[NUM_MOTORS];
    bool temp_configured[NUM_MOTORS];       // config.h pin >= 0
    bool temp_has_device[NUM_MOTORS];       // a probe was found on the pin
    float temp_cache[NUM_MOTORS];           // last good temps
    bool temp_valid[NUM_MOTORS];            // live validity (hysteresis-gated)
    int bad_read_streak[NUM_MOTORS];        // consecutive bad windows
    bool conversion_pending;
    unsigned long conversion_start;

    unsigned long last_reauth;              // periodic probe re-find
    static const unsigned long REAUTH_MS = 10000;
    // Hard cap on the per-window read loop so a slow/flaky bus can never
    // stall the main loop (OneWire search can spin on a marginal line).
    static const unsigned long READ_DEADLINE_MS = 350;
    static const int MAX_READ_RETRIES = 3;  // getTempC() native retries
    static const int MAX_BAD_STREAK = 3;    // hysteresis: clear valid after N

    Adafruit_INA219* ina_sensors[NUM_MOTORS];
    int vib_pins[NUM_MOTORS];
    uint8_t ina_addresses[NUM_MOTORS];
    bool ina_ok[NUM_MOTORS];
    unsigned long vib_last_high[NUM_MOTORS];
    unsigned long last_ina_check;   // last time INA219 presence was re-verified
    static const unsigned long INA_RECHECK_MS = 5000;

    void requestConversion() {
      bool any = false;
      for (int i = 0; i < NUM_MOTORS; i++) {
        if (!temp_configured[i]) continue;
        // Re-assert internal pull-up: the OneWire lib drives GPIO registers
        // directly and never enables the ESP32 pull-up itself. This shores
        // the idle-HIGH alongside the external 4.7k (bench robustness).
        pinMode(DS18B20_PINS[i], INPUT_PULLUP);
        // waitForConversion is off, so this returns instantly (async).
        if (ds18b20[i]->requestTemperatures()) any = true;
      }
      if (any) {
        conversion_pending = true;
        conversion_start = millis();
      }
    }

    // Re-enumerate each wired pin so a probe that dropped/re-joined is
    // rediscovered without a reboot (self-healing after unplug/replug).
    void rescanTempDevices() {
      for (int i = 0; i < NUM_MOTORS; i++) {
        if (!temp_configured[i]) continue;
        int n = ds18b20[i]->getDeviceCount();
        bool found = (n > 0) && ds18b20[i]->getAddress(temp_addresses[i], 0);
        if (found) {
          ds18b20[i]->setResolution(temp_addresses[i], DS18B20_RESOLUTION);
        }
        if (found != temp_has_device[i]) {
          Serial.print("[Sensors] Motor ");
          Serial.print(i + 1);
          Serial.println(found ? " - temp probe re-connected"
                               : " - temp probe lost");
        }
        temp_has_device[i] = found;
        if (!found) {
          temp_valid[i] = false;   // honest: device gone -> "--" now
          bad_read_streak[i] = 0;
        }
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
    SensorManager() : conversion_pending(false), conversion_start(0),
                      last_reauth(0), last_ina_check(0) {
      for (int i = 0; i < NUM_MOTORS; i++) {
        one_wire[i] = nullptr;
        ds18b20[i] = nullptr;
        temp_configured[i] = false;
        temp_has_device[i] = false;
        temp_cache[i] = NAN;
        temp_valid[i] = false;
        bad_read_streak[i] = 0;
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

      // DS18B20 — one bus per configured motor pin. No ROM matching: the
      // probe on pin i belongs to motor i+1 by construction.
      for (int i = 0; i < NUM_MOTORS; i++) {
        temp_configured[i] = (DS18B20_PINS[i] >= 0);
        if (!temp_configured[i]) continue;

        pinMode(DS18B20_PINS[i], INPUT_PULLUP);
        one_wire[i] = new OneWire(DS18B20_PINS[i]);
        ds18b20[i] = new DallasTemperature(one_wire[i]);
        ds18b20[i]->begin();
        ds18b20[i]->setWaitForConversion(false);  // never block on conversions

        int n = ds18b20[i]->getDeviceCount();
        Serial.print("[Sensors] Motor ");
        Serial.print(i + 1);
        Serial.print(" DS18B20 on GPIO ");
        Serial.print(DS18B20_PINS[i]);
        Serial.print(": found ");
        Serial.println(n);

        temp_has_device[i] =
            (n > 0) && ds18b20[i]->getAddress(temp_addresses[i], 0);
        if (temp_has_device[i]) {
          ds18b20[i]->setResolution(temp_addresses[i], DS18B20_RESOLUTION);
        } else {
          Serial.print("[Sensors] WARNING: motor ");
          Serial.print(i + 1);
          Serial.println(" - no probe found (check pull-up/wiring)");
        }
      }
      last_reauth = millis();

      // Kick off first async conversion cycle
      requestConversion();
    }

    // Call often from loop(). Drives the async DS18B20 conversion:
    // request -> wait ~750ms -> read all into cache -> request again.
    void update() {
      unsigned long now = millis();

      // Periodically re-verify INA219 presence (detects unplug mid-run).
      if (now - last_ina_check >= INA_RECHECK_MS) {
        last_ina_check = now;
        recheckIna();
      }

      // Re-enumerate OneWire probes so reconnect is picked up without reboot.
      if (now - last_reauth >= REAUTH_MS) {
        last_reauth = now;
        rescanTempDevices();
      }

      if (!conversion_pending) {
        requestConversion();
        return;
      }
      if (now - conversion_start < DS18B20_CONVERSION_MS) {
        return;
      }

      unsigned long deadline = millis() + READ_DEADLINE_MS;
      for (int i = 0; i < NUM_MOTORS; i++) {
        if (!temp_configured[i] || !temp_has_device[i]) continue;
        if ((long)(millis() - deadline) > 0) break;  // never stall the loop

        // getTempC() retries internally (CRC-verified scratchpad reads).
        float t = ds18b20[i]->getTempC(temp_addresses[i], MAX_READ_RETRIES);
        if (t > -50.0f && t < 150.0f) {   // valid reading
          temp_cache[i] = t;
          temp_valid[i] = true;
          bad_read_streak[i] = 0;
        } else {
          // One bad window. Only drop validity after several in a row
          // (hysteresis) so a single transient -127 doesn't flicker the
          // badge; a genuinely unplugged probe still goes "--" honestly.
          bad_read_streak[i]++;
          if (bad_read_streak[i] >= MAX_BAD_STREAK) {
            temp_valid[i] = false;
          }
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

      // Temperature from this motor's dedicated DS18B20 cache. Disabled
      // pins / lost probes publish NAN -> temp_valid:0 -> dashboard "--".
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