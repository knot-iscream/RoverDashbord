#ifndef CONFIG_H
#define CONFIG_H

// WiFi credentials
#define WIFI_SSID "your_wifi_ssid"
#define WIFI_PASSWORD "your_wifi_password"

// MQTT broker settings
#define MQTT_BROKER "192.168.1.100"
#define MQTT_PORT 1883
#define MQTT_TOPIC_PREFIX "rover/motor"

// Sensor pins
#define DH11_PIN_1 4   // Motor 1 (FL)
#define DH11_PIN_2 5   // Motor 2 (FR)
#define DH11_PIN_3 6   // Motor 3 (RL)
#define DH11_PIN_4 7   // Motor 4 (RR)

#define SW420_PIN_1 14  // Motor 1 vibration
#define SW420_PIN_2 27  // Motor 2 vibration
#define SW420_PIN_3 26  // Motor 3 vibration
#define SW420_PIN_4 25  // Motor 4 vibration

// INA219 I2C addresses (each needs unique ADDR pin config)
#define INA219_ADDR_1 0x41
#define INA219_ADDR_2 0x44
#define INA219_ADDR_3 0x45
#define INA219_ADDR_4 0x40

// Sample interval (milliseconds)
#define SAMPLE_INTERVAL_MS 250
#define MQTT_PUBLISH_INTERVAL_MS 1000

#endif
