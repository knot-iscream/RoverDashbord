#ifndef MOTOR_DRIVER_H
#define MOTOR_DRIVER_H

#include <Arduino.h>
#include <ArduinoJson.h>
#include "config.h"

// One channel of the 4-channel 6 A board (RBD-3658, "DC_MOTOR_4").
// Two logic inputs, no enable: each input drives one half of the
// H-bridge, so the PWM duty goes on whichever half is driving.
//   IN_A = PWM, IN_B = LOW  -> forward at that duty
//   IN_A = LOW, IN_B = PWM  -> reverse at that duty
//   both LOW -> coast, both HIGH -> brake
struct MotorChannel {
  int in_a;
  int in_b;
};

class MotorDriver {
  private:
    MotorChannel channels[4];
    int target[4];            // last commanded speed (-255..255)
    float applied[4];         // current point on the ramp
    float ramp_from[4];       // applied value when the target changed
    unsigned long ramp_start[4];
    int written[4];           // last duty actually sent to the bridge
    int8_t dir[4];            // last applied direction: -1, 0, +1
    bool ramp_enabled;

    // Write one duty to the bridge. The caller supplies the ramped value;
    // this function never touches the target or the ramp state.
    void writePwm(int idx, int motor_speed) {
      if (motor_speed > 255) motor_speed = 255;
      if (motor_speed < -255) motor_speed = -255;
      if (motor_speed == written[idx]) return;
      MotorChannel& ch = channels[idx];

      int8_t want = (motor_speed > 0) - (motor_speed < 0);

      // Dead time on a direction flip: both inputs LOW so the bridge
      // is fully off before the other half starts. Reversing a motor
      // mid-spin dumps its back-EMF into the outputs, and this board
      // has no enable pin to gate that.
      if (want != dir[idx]) {
        ledcWrite(ch.in_a, 0);
        ledcWrite(ch.in_b, 0);
        delayMicroseconds(500);
        dir[idx] = want;
      }

      int pwm = abs(motor_speed);
      if (want > 0) {
        ledcWrite(ch.in_a, pwm);
        ledcWrite(ch.in_b, 0);
      } else if (want < 0) {
        ledcWrite(ch.in_a, 0);
        ledcWrite(ch.in_b, pwm);
      } else if (MOTOR_STOP_BRAKE) {
        ledcWrite(ch.in_a, 255);
        ledcWrite(ch.in_b, 255);
      } else {
        ledcWrite(ch.in_a, 0);
        ledcWrite(ch.in_b, 0);
      }
      written[idx] = motor_speed;
    }

    // Apply a new target now (used by STOP and by calibration).
    void applyNow(int idx, int motor_speed) {
      target[idx] = motor_speed;
      applied[idx] = motor_speed;
      ramp_from[idx] = motor_speed;
      ramp_start[idx] = millis();
      writePwm(idx, motor_speed);
    }

  public:
    void begin() {
      int pins[4][2] = {
        {MOTOR1_IN_A, MOTOR1_IN_B},  // motor 1 (FL)
        {MOTOR2_IN_A, MOTOR2_IN_B},  // motor 2 (FR)
        {MOTOR3_IN_A, MOTOR3_IN_B},  // motor 3 (RL)
        {MOTOR4_IN_A, MOTOR4_IN_B},  // motor 4 (RR)
      };

      ramp_enabled = true;
      for (int i = 0; i < 4; i++) {
        channels[i].in_a = pins[i][0];
        channels[i].in_b = pins[i][1];

        // Both halves get a PWM channel so either can carry the duty.
        ledcAttach(channels[i].in_a, MOTOR_PWM_FREQ, MOTOR_PWM_RES);
        ledcAttach(channels[i].in_b, MOTOR_PWM_FREQ, MOTOR_PWM_RES);

        target[i] = 0;
        applied[i] = 0.0f;
        ramp_from[i] = 0.0f;
        ramp_start[i] = 0;
        written[i] = 0;
        dir[i] = 0;
      }
      stopAll();
      Serial.println("[MotorDriver] 4ch 6A board initialized (2 inputs per channel)");
      Serial.printf("[MotorDriver] torque ramp %lu ms (quadratic ease-in)\n",
                    (unsigned long)TORQUE_RAMP_MS);
    }

    // Call once per loop. Advances all four ramps and writes the bridge.
    void update() {
      if (!ramp_enabled) return;
      unsigned long now = millis();
      for (int i = 0; i < 4; i++) {
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

    // Calibration changes speed every half step, far faster than the
    // ramp. It disables the ramp for the duration and restores it after.
    void setRampEnabled(bool on) {
      ramp_enabled = on;
    }

    // speed: -255 (full reverse) .. 0 (stop) .. +255 (full forward)
    void setMotor(int idx, int motor_speed) {
      if (idx < 0 || idx > 3) return;
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
      for (int i = 0; i < 4; i++) setMotor(i, motor_speed);
    }

    void stopAll() {
      for (int i = 0; i < 4; i++) applyNow(i, 0);
    }

    // The value the motor is actually seeing, not just the last command.
    int getSpeed(int idx) {
      if (idx < 0 || idx > 3) return 0;
      return (int)applied[idx];
    }

    // Parse a JSON command:
    //   {"motor":1,"speed":200}    -> single motor (motor 1-4)
    //   {"motor":0,"speed":200}    -> all motors
    //   {"speed":200}              -> all motors
    // speed: -255 (reverse) .. +255 (forward)
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
      } else if (m >= 1 && m <= 4) {
        setMotor(m - 1, s);
        Serial.printf("[Motor] %d -> %d\n", m, s);
      } else {
        Serial.println("[Motor] Invalid motor index");
      }
    }
};

#endif
