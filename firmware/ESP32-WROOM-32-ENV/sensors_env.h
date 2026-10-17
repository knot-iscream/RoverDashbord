#ifndef SENSORS_ENV_H
#define SENSORS_ENV_H

#include <Arduino.h>
#include <Wire.h>
#include <math.h>
#include <Preferences.h>
#include "config.h"

// ENV board sensors, all hand-rolled register access on purpose: the bench
// setup then needs no extra Arduino libraries beyond the existing three
// (WiFiManager, PubSubClient, ArduinoJson).
//
// Validity rules are the same as the rest of the rover: a device that cannot
// vouch for itself reports ok=0 / NaN, the backend renders "--". Nothing is
// fabricated, ever.

// ------------------------------------------------------------
// One ADS1115 (16-bit, 4 channels). Single-shot conversions at 128 SPS,
// comparator off. MUX: 4..7 = AIN0..AIN3 single-ended, 0 = AIN0-AIN1 diff.
// ------------------------------------------------------------
class Ads1115 {
  private:
    uint8_t addr = 0;
    bool ok = false;

    static uint16_t cfgWord(uint8_t mux, uint8_t pga) {
      return (uint16_t)(0x8000 |                 // OS: start single-shot
                        ((uint16_t)(mux & 0x07) << 12) |
                        ((uint16_t)(pga & 0x07) << 9) |
                        (1 << 8) |               // MODE: single-shot
                        (4 << 5) |               // DR: 128 SPS (~7.8 ms)
                        0x0003);                 // comparator disabled
    }

    static float fsrLsb(uint8_t pga) {
      // PGA 0..5 -> +/-6.144, 4.096, 2.048, 1.024, 0.512, 0.256 V
      static const float fsr[6] = {6.144f, 4.096f, 2.048f, 1.024f, 0.512f, 0.256f};
      if (pga > 5) pga = 5;
      return (fsr[pga] * 2.0f) / 65536.0f;
    }

    bool writeConfig(uint16_t cfg) {
      Wire.beginTransmission(addr);
      Wire.write((uint8_t)0x01);
      Wire.write((uint8_t)(cfg >> 8));
      Wire.write((uint8_t)(cfg & 0xFF));
      return Wire.endTransmission() == 0;
    }

  public:
    // Probe the chip (a config write is answered only when it is there).
    void begin(uint8_t a, const char* label) {
      addr = a;
      ok = writeConfig(cfgWord(4, 0));   // AIN0, +/-6.144 V probe
      Serial.print("[ADS] ");
      Serial.print(label);
      Serial.print(" at 0x");
      Serial.print(a, HEX);
      Serial.println(ok ? " OK" : " NOT FOUND");
    }

    bool isOk() { return ok; }

    // Returns volts; NAN on any bus error.
    float read(uint8_t mux, uint8_t pga) {
      if (!writeConfig(cfgWord(mux, pga))) return NAN;
      delay(10);                        // 128 SPS conversion + margin
      Wire.beginTransmission(addr);
      Wire.write((uint8_t)0x00);
      if (Wire.endTransmission(false) != 0) return NAN;
      if (Wire.requestFrom((uint8_t)addr, (uint8_t)2) != 2) return NAN;
      uint8_t hi = Wire.read();
      uint8_t lo = Wire.read();
      int16_t raw = (int16_t)((hi << 8) | lo);
      return raw * fsrLsb(pga);
    }
};

// ------------------------------------------------------------
// MLX90614 IR thermometer (SMBus-ish, RAM read). Both units ship at 0x5A;
// one must be re-addressed to 0x5B via EEPROM 0xC7. Only the range check
// and the caller's hysteresis vouch for validity (PEC not verified).
// ------------------------------------------------------------
class Mlx90614 {
  private:
    uint8_t addr = 0;
    bool ok = false;

    float readRam(uint8_t ram) {
      Wire.beginTransmission(addr);
      Wire.write(ram);
      if (Wire.endTransmission(false) != 0) return NAN;
      delay(2);
      if (Wire.requestFrom((uint8_t)addr, (uint8_t)3) != 3) return NAN;
      uint8_t lo = Wire.read();
      uint8_t hi = Wire.read();
      Wire.read();                       // PEC byte, not verified
      int16_t raw = (int16_t)((hi << 8) | lo);
      float c = raw * 0.02f - 273.15f;
      if (c < -70.0f || c > 380.0f) return NAN;
      return c;
    }

  public:
    void begin(uint8_t a, const char* label) {
      addr = a;
      float amb = readRam(0x06);
      ok = !isnan(amb);
      Serial.print("[MLX] ");
      Serial.print(label);
      Serial.print(" at 0x");
      Serial.print(a, HEX);
      Serial.println(ok ? " OK" : " NOT FOUND");
    }

    bool isOk() { return ok; }
    float object()  { return readRam(0x07); }
    float ambient() { return readRam(0x06); }
};

// ------------------------------------------------------------
// 5-channel IR flame module: 5 digital direction lines (the analog
// magnitude comes from an ADS channel instead).
// ------------------------------------------------------------
class FlameArray {
  private:
    int pins[FLAME_PIN_COUNT] = FLAME_DIR_PINS;

  public:
    void begin() {
      for (int i = 0; i < FLAME_PIN_COUNT; i++) pinMode(pins[i], INPUT);
      Serial.println("[Flame] 5 direction lines ready");
    }

    // 1..5 = active direction, 0 = no flame seen.
    int direction() {
      for (int i = 0; i < FLAME_PIN_COUNT; i++) {
        if (digitalRead(pins[i]) == HIGH) return i + 1;
      }
      return 0;
    }
};

// ------------------------------------------------------------
// One telemetry frame. NaN = invalid, the publisher turns it into null.
// gas[] is the MQ response R0/Rs: 1.0 = clean-air baseline, grows with gas.
// ------------------------------------------------------------
struct EnvSample {
  bool ads_ok = false;
  bool mlx_ok = false;
  bool flame_ok = false;   // module alive (analog channel readable, or a
                           // digital line has been seen HIGH since boot)
  bool baseline_ok = false;
  float gas[GAS_CHANNEL_COUNT] = {NAN, NAN, NAN, NAN, NAN};
  float o2_mv = NAN;
  float o2_pct = NAN;
  float flame_lvl = NAN;
  int flame_dir = 0;
  float heat_l = NAN, heat_r = NAN;
  float heat_l_amb = NAN, heat_r_amb = NAN;
};

// MQ response from voltages only: RL cancels out of R0/Rs, so the module's
// load resistor never needs to be known. v0 = clean-air volts (baseline).
static float gasResponse(float v, float v0, float vc) {
  if (isnan(v) || isnan(v0)) return NAN;
  if (v0 <= 0.02f || v <= 0.02f) return NAN;
  if (v >= vc - 0.05f) return NAN;            // rail = unusable reading
  float r = ((vc - v0) / v0) * (v / (vc - v));
  if (r < 0.0f) r = 0.0f;
  if (r > 20.0f) r = 20.0f;                   // sanity cap
  return r;
}

#endif
