#ifndef CONFIG_H
#define CONFIG_H

// ESP-2 "MID" — 2 blind middle motors, no sensors.
// Own MQTT namespace (rover/mid/*) so it never fights ESP-1 corners
// on rover/motor/command. Backend remaps mid motor 1-2 -> 5-6.

// WiFi + MQTT — FALLBACK DEFAULTS ONLY (same portal flow as corners).
#define WIFI_SSID "Your_SSID"
#define WIFI_PASSWORD "Your_Password"

#define MQTT_BROKER "192.168.0.96"
#define MQTT_PORT 1883
#define MQTT_TOPIC_PREFIX "rover/mid"

// Phone setup portal (wifi_portal.h, WiFiManager by tzapu)
#define PORTAL_AP_NAME "Rover-Setup-MID"
#define PORTAL_AP_PASSWORD ""
#define WIFI_CONNECT_TIMEOUT_S 10
#define PORTAL_TIMEOUT_S 180

// ============================================================
// Motor driver — 1x L298N H-Bridge (2 channels = 2 middle motors)
// Same pins as corners L298N #1 so one spare board swaps either way.
// ============================================================
#define MID_ENA 13   // PWM speed — Mid motor 1 (publishes as motor 5)
#define MID_IN1 15
#define MID_IN2 14
#define MID_ENB 18   // PWM speed — Mid motor 2 (publishes as motor 6)
#define MID_IN3 19
#define MID_IN4 23

// Alias names so the shared motor_driver.h compiles untouched.
#define L298N1_ENA MID_ENA
#define L298N1_IN1 MID_IN1
#define L298N1_IN2 MID_IN2
#define L298N1_ENB MID_ENB
#define L298N1_IN3 MID_IN3
#define L298N1_IN4 MID_IN4
// Unwired channels settle LOW (no floating inputs).
#define L298N2_ENA -1
#define L298N2_IN1 -1
#define L298N2_IN2 -1
#define L298N2_ENB -1
#define L298N2_IN3 -1
#define L298N2_IN4 -1

#define MOTOR_PWM_FREQ   1000
#define MOTOR_PWM_RES    8
#define DEFAULT_MOTOR_SPEED 0

// ============================================================
// MQTT topics (own namespace — never rover/motor/*)
// ============================================================
#define MQTT_CALIB_CMD_TOPIC    "rover/mid/command"
#define MQTT_CALIB_STATUS_TOPIC "rover/mid/status"
#define MQTT_MOTOR_CMD_TOPIC    "rover/mid/command"
#define MQTT_MOTOR_STATUS_TOPIC "rover/mid/status"

// ============================================================
// Timing
// ============================================================
#define SAMPLE_INTERVAL_MS 250

#endif
