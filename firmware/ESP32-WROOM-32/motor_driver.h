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
    int speed[4];       // last commanded speed (-255..255)
    int8_t dir[4];      // last applied direction: -1, 0, +1

    void setChannel(int idx, int motor_speed) {
      MotorChannel& ch = channels[idx];
      if (motor_speed > 255) motor_speed = 255;
      if (motor_speed < -255) motor_speed = -255;
      speed[idx] = motor_speed;

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
    }

  public:
    void begin() {
      int pins[4][2] = {
        {MOTOR1_IN_A, MOTOR1_IN_B},  // motor 1 (FL)
        {MOTOR2_IN_A, MOTOR2_IN_B},  // motor 2 (FR)
        {MOTOR3_IN_A, MOTOR3_IN_B},  // motor 3 (RL)
        {MOTOR4_IN_A, MOTOR4_IN_B},  // motor 4 (RR)
      };

      for (int i = 0; i < 4; i++) {
        channels[i].in_a = pins[i][0];
        channels[i].in_b = pins[i][1];

        // Both halves get a PWM channel so either can carry the duty.
        ledcAttach(channels[i].in_a, MOTOR_PWM_FREQ, MOTOR_PWM_RES);
        ledcAttach(channels[i].in_b, MOTOR_PWM_FREQ, MOTOR_PWM_RES);

        speed[i] = 0;
        dir[i] = 0;
      }
      stopAll();
      Serial.println("[MotorDriver] 4ch 6A board initialized (2 inputs per channel)");
    }

    // speed: -255 (full reverse) .. 0 (stop) .. +255 (full forward)
    void setMotor(int idx, int motor_speed) {
      if (idx < 0 || idx > 3) return;
      setChannel(idx, motor_speed);
    }

    void runAll(int motor_speed) {
      for (int i = 0; i < 4; i++) setChannel(i, motor_speed);
    }

    void stopAll() {
      for (int i = 0; i < 4; i++) setChannel(i, 0);
    }

    int getSpeed(int idx) {
      if (idx < 0 || idx > 3) return 0;
      return speed[idx];
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
        Serial.printf("[Motor] All @ %d\n", s);
      } else if (m >= 1 && m <= 4) {
        setMotor(m - 1, s);
        Serial.printf("[Motor] %d @ %d\n", m, s);
      } else {
        Serial.println("[Motor] Invalid motor index");
      }
    }
};

#endif
