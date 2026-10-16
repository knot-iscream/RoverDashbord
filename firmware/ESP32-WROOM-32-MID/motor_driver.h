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
    int speed[2];

    void setChannel(int idx, int motor_speed) {
      MotorChannel& ch = channels[idx];
      if (motor_speed > 255) motor_speed = 255;
      if (motor_speed < -255) motor_speed = -255;
      speed[idx] = motor_speed;

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
    }

  public:
    void begin() {
      int pins[2][3] = {
        {MID_ENA, MID_IN1, MID_IN2},  // mid motor 1 (-> backend motor 5)
        {MID_ENB, MID_IN3, MID_IN4},  // mid motor 2 (-> backend motor 6)
      };

      for (int i = 0; i < 2; i++) {
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
      Serial.println("[MotorDriver] MID 1x L298N initialized (2 channels)");
    }

    void setMotor(int idx, int motor_speed) {
      if (idx < 0 || idx > 1) return;
      setChannel(idx, motor_speed);
    }

    void runAll(int motor_speed) {
      for (int i = 0; i < 2; i++) setChannel(i, motor_speed);
    }

    void stopAll() {
      for (int i = 0; i < 2; i++) setChannel(i, 0);
    }

    int getSpeed(int idx) {
      if (idx < 0 || idx > 1) return 0;
      return speed[idx];
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

      int m = doc["motor"] | 0;
      int s = doc["speed"] | 0;

      if (m == 0) {
        runAll(s);
        Serial.printf("[Motor] Both @ %d\n", s);
      } else if (m >= 1 && m <= 2) {
        setMotor(m - 1, s);
        Serial.printf("[Motor] Mid %d @ %d\n", m, s);
      } else {
        Serial.println("[Motor] Invalid motor index");
      }
    }
};

#endif
