#ifndef SENSORS_H
#define SENSORS_H

#include <DHT.h>
#include <Adafruit_INA219.h>

struct MotorSensorData {
  int motor_id;
  bool vibration;
  float voltage;
  float current;
  float temperature;
};

class SensorManager {
  private:
    DHT* dht_sensors[4];
    Adafruit_INA219* ina_sensors[4];
    int vib_pins[4];
    int dht_pins[4];
    uint8_t ina_addresses[4];

  public:
    SensorManager() {}

    void begin(int dht_pins_arr[4], int vib_pins_arr[4], uint8_t ina_addrs[4]) {
      for (int i = 0; i < 4; i++) {
        dht_pins[i] = dht_pins_arr[i];
        vib_pins[i] = vib_pins_arr[i];
        ina_addresses[i] = ina_addrs[i];

        pinMode(vib_pins[i], INPUT);

        dht_sensors[i] = new DHT(dht_pins[i], DHT11);
        dht_sensors[i]->begin();

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
    }

    MotorSensorData readMotor(int motor_idx) {
      MotorSensorData data;
      data.motor_id = motor_idx + 1;

      // Vibration — digital read from SW-420 (NC = normally closed)
      // SW-420 outputs LOW when vibration detected
      data.vibration = (digitalRead(vib_pins[motor_idx]) == LOW);

      // Temperature from DHT11
      float temp = dht_sensors[motor_idx]->readTemperature();
      if (isnan(temp)) {
        temp = 25.0;  // fallback
      }
      data.temperature = temp;

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
