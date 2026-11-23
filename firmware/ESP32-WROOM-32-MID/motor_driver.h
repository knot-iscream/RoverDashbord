#ifndef MOTOR_DRIVER_H
#define MOTOR_DRIVER_H

#include <Arduino.h>
#include <ArduinoJson.h>
#include "config.h"

// MID build: 2 middle motors on 1x L298N (blind, no sensors).
struct MotorChannel {
  int ena;
  int in1;
  int in2;
};

class MotorDriver {
  private:
    MotorChannel channels[2];
    int target[2];            // last commanded speed (-255..255)
    float applied[2];         // current point on the ramp
    float ramp_from[2];       // applied value when the target changed
    unsigned long ramp_start[2];
    int written[2];           // last duty actually sent to the bridge
    bool ramp_enabled;

    // Write one duty to the bridge. The caller supplies the ramped value;
    // this function never touches the target or the ramp state.
    void writePwm(int idx, int motor_speed) {
      if (motor_speed > 255) motor_speed = 255;
      if (motor_speed < -255) motor_speed = -255;
      if (motor_speed == written[idx]) return;
      MotorChannel& ch = channels[idx];

      int dir1 = motor_speed > 0 ? HIGH : LOW;
      int dir2 = motor_speed > 0 ? LOW : HIGH;
      int pwm  = abs(motor_speed);

      if (motor_speed == 0) {
        dir1 = LOW;
        dir2 = LOW;
      }

      int cur1 = digitalRead(ch.in1);
      int cur2 = digitalRead(ch.in2);

      if (cur1 != dir1 || cur2 != dir2) {
        ledcWrite(ch.ena, 0);
        delayMicroseconds(5);
      }

      digitalWrite(ch.in1, dir1);
      digitalWrite(ch.in2, dir2);
      ledcWrite(ch.ena, pwm);
      written[idx] = motor_speed;
    }

    // Apply a new target now (used by STOP).
    void applyNow(int idx, int motor_speed) {
      target[idx] = motor_speed;
      applied[idx] = motor_speed;
      ramp_from[idx] = motor_speed;
      ramp_start[idx] = millis();
      writePwm(idx, motor_speed);
    }

  public:
    void begin() {
      int pins[2][3] = {
        {MID_ENA, MID_IN1, MID_IN2},  // mid motor 1 (-> backend motor 5)
        {MID_ENB, MID_IN3, MID_IN4},  // mid motor 2 (-> backend motor 6)
      };

      ramp_enabled = true;
      for (int i = 0; i < 2; i++) {
        channels[i].ena = pins[i][0];
        channels[i].in1 = pins[i][1];
        channels[i].in2 = pins[i][2];

        pinMode(channels[i].ena, OUTPUT);
        pinMode(channels[i].in1, OUTPUT);
        pinMode(channels[i].in2, OUTPUT);

        ledcAttach(channels[i].ena, MOTOR_PWM_FREQ, MOTOR_PWM_RES);

        target[i] = 0;
        applied[i] = 0.0f;
        ramp_from[i] = 0.0f;
        ramp_start[i] = 0;
        written[i] = 0;
      }
      stopAll();
      Serial.println("[MotorDriver] MID 1x L298N initialized (2 channels)");
      Serial.printf("[MotorDriver] torque ramp %lu ms (quadratic ease-in)\n",
                    (unsigned long)TORQUE_RAMP_MS);
    }

    // Call once per loop. Advances both ramps and writes the bridge.
    void update() {
      if (!ramp_enabled) return;
      unsigned long now = millis();
      for (int i = 0; i < 2; i++) {
        if ((int)applied[i] == target[i] && ramp_from[i] == target[i]) continue;
        unsigned long elapsed = now - ramp_start[i];
        float p = (float)elapsed / (float)TORQUE_RAMP_MS;
        if (p >= 1.0f) {
          applied[i] = target[i];
          ramp_from[i] = target[i];
        } else {
          // Quadratic ease-in: slow off the mark, larger steps near the end.
          applied[i] = ramp_from[i] + (target[i] - ramp_from[i]) * p * p;
        }
        writePwm(i, (int)applied[i]);
      }
    }

    void setRampEnabled(bool on) {
      ramp_enabled = on;
    }

    void setMotor(int idx, int motor_speed) {
      if (idx < 0 || idx > 1) return;
      if (motor_speed > 255) motor_speed = 255;
      if (motor_speed < -255) motor_speed = -255;
      if (!ramp_enabled || motor_speed == 0) {
        applyNow(idx, motor_speed);
        return;
      }
      target[idx] = motor_speed;
      ramp_from[idx] = applied[idx];
      ramp_start[idx] = millis();
    }

    void runAll(int motor_speed) {
      for (int i = 0; i < 2; i++) setMotor(i, motor_speed);
    }

    void stopAll() {
      for (int i = 0; i < 2; i++) applyNow(i, 0);
    }

    // The value the motor is actually seeing, not just the last command.
    int getSpeed(int idx) {
      if (idx < 0 || idx > 1) return 0;
      return (int)applied[idx];
    }

    // Parse a JSON command:
    //   {"motor":1,"speed":200}    -> mid motor 1 (backend motor 5)
    //   {"motor":2,"speed":200}    -> mid motor 2 (backend motor 6)
    //   {"motor":0,"speed":200}    -> both
    //   {"speed":200}              -> both
    void applyCommand(const char* json) {
      StaticJsonDocument<96> doc;
      DeserializationError err = deserializeJson(doc, json);
      if (err) {
        Serial.print("[Motor] Bad command: ");
        Serial.println(err.c_str());
        return;
      }

      int m = doc["motor"] | 0;          // 0 = all
      int s = doc["speed"] | 0;

      if (m == 0) {
        runAll(s);
        Serial.printf("[Motor] All -> %d\n", s);
      } else if (m >= 1 && m <= 2) {
        setMotor(m - 1, s);
        Serial.printf("[Motor] %d -> %d\n", m, s);
      } else {
        Serial.println("[Motor] Invalid motor index");
      }
    }
};

#endif
