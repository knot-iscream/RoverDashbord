#ifndef CONFIG_H
#define CONFIG_H

// WiFi credentials
#define WIFI_SSID "your_wifi_ssid"
#define WIFI_PASSWORD "your_wifi_password"

// MQTT broker settings
#define MQTT_BROKER "192.168.1.100"
#define MQTT_PORT 1883
#define MQTT_TOPIC_PREFIX "rover/motor"

// ============================================================
// Motor drivers — 2x L298N H-Bridge (2 channels each = 4 motors)
// NOTE: GPIO 6-11 are tied to the internal SPI flash on
// ESP32-WROOM and CANNOT be used as general purpose I/O.
// GPIO 1/3 are UART0. GPIO 34-39 are input-only.
// ============================================================

// L298N #1 — Motors 1 (FL) & 2 (FR)
#define L298N1_ENA 13   // PWM speed — Motor 1
#define L298N1_IN1 15   // Motor 1 direction
#define L298N1_IN2 14   // Motor 1 direction
#define L298N1_ENB 18   // PWM speed — Motor 2
#define L298N1_IN3 19   // Motor 2 direction
#define L298N1_IN4 23   // Motor 2 direction

// L298N #2 — Motors 3 (RL) & 4 (RR)
#define L298N2_ENA 32   // PWM speed — Motor 3
#define L298N2_IN1 33   // Motor 3 direction
#define L298N2_IN2 27   // Motor 3 direction
#define L298N2_ENB 16   // PWM speed — Motor 4
#define L298N2_IN3 17   // Motor 4 direction
#define L298N2_IN4 25   // Motor 4 direction

// PWM configuration for the L298N enable pins
#define MOTOR_PWM_FREQ   1000   // Hz
#define MOTOR_PWM_RES    8      // bits (0-255 duty)
#define CALIB_MOTOR_SPEED 200   // duty during calibration (0-255)
#define DEFAULT_MOTOR_SPEED 0   // duty on boot / idle (0-255)

// ============================================================
// Sensors
// ============================================================

// DS18B20 temperature — all 4 probes share ONE OneWire bus
// Requires a ~4.7k pull-up resistor on the data line to 3.3V.
#define DS18B20_DATA_PIN 4
#define DS18B20_RESOLUTION 12

// SW-420 vibration (NC = normally closed). Input-only GPIOs.
#define SW420_PIN_1 34  // Motor 1 vibration
#define SW420_PIN_2 35  // Motor 2 vibration
#define SW420_PIN_3 36  // Motor 3 vibration
#define SW420_PIN_4 39  // Motor 4 vibration

// INA219 I2C addresses (each needs unique ADDR pin config)
// I2C bus: SDA = GPIO 21, SCL = GPIO 22
#define INA219_ADDR_1 0x41
#define INA219_ADDR_2 0x44
#define INA219_ADDR_3 0x45
#define INA219_ADDR_4 0x40

// ============================================================
// MQTT topics
// ============================================================
#define MQTT_CALIB_CMD_TOPIC    "rover/calibration/command"
#define MQTT_CALIB_STATUS_TOPIC "rover/calibration/status"
#define MQTT_MOTOR_CMD_TOPIC    "rover/motor/command"
#define MQTT_MOTOR_STATUS_TOPIC "rover/motor/status"

// ============================================================
// Timing
// ============================================================
#define CALIB_WARMUP_MS   900000   // 15 minutes
#define SAMPLE_INTERVAL_MS 250

#endif