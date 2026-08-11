#ifndef MOTOR_DRIVER_H
#define MOTOR_DRIVER_H

#include <Arduino.h>
#include <ArduinoJson.h>
#include "config.h"

// Maps a "motor index" (0-3) to its L298N channel hardware.
struct MotorChannel {
  int ena;   // PWM enable pin
  int in1;   // direction pin
  int in2;   // direction pin
};

class MotorDriver {
  private:
    MotorChannel channels[4];
    int speed[4];               // last commanded speed (-255..255)

    void setChannel(int idx, int motor_speed) {
      MotorChannel& ch = channels[idx];
      if (motor_speed > 255) motor_speed = 255;
      if (motor_speed < -255) motor_speed = -255;
      speed[idx] = motor_speed;

      int dir1 = motor_speed > 0 ? HIGH : LOW;
      int dir2 = motor_speed > 0 ? LOW : HIGH;
      int pwm  = abs(motor_speed);

      digitalWrite(ch.in1, dir1);
      digitalWrite(ch.in2, dir2);
      ledcWrite(ch.ena, pwm);
    }

  public:
    void begin() {
      int pins[4][3] = {
        {L298N1_ENA, L298N1_IN1, L298N1_IN2},  // motor 1
        {L298N1_ENB, L298N1_IN3, L298N1_IN4},  // motor 2
        {L298N2_ENA, L298N2_IN1, L298N2_IN2},  // motor 3
        {L298N2_ENB, L298N2_IN3, L298N2_IN4},  // motor 4
      };

      for (int i = 0; i < 4; i++) {
        channels[i].ena = pins[i][0];
        channels[i].in1 = pins[i][1];
        channels[i].in2 = pins[i][2];

        pinMode(channels[i].ena, OUTPUT);
        pinMode(channels[i].in1, OUTPUT);
        pinMode(channels[i].in2, OUTPUT);

        ledcAttach(channels[i].ena, MOTOR_PWM_FREQ, MOTOR_PWM_RES);

        speed[i] = 0;
      }
      stopAll();
      Serial.println("[MotorDriver] 2x L298N initialized (4 channels)");
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