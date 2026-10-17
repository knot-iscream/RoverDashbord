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
// MPU6050 IMU (GY-521) — localization subsystem, NOT motor data.
// Own topic rover/imu. Yaw is gyro-integrated and drifts by design (no
// magnetometer); lidar SLAM corrects it in the Pi phase. Keep the IMU flat,
// stand it off the board and away from the servo leads, or yaw gets noisy.
// ============================================================
#define MPU6050_ADDR 0x68
#define IMU_SAMPLE_MS 20        // internal filter step (50 Hz)
#define IMU_PUBLISH_MS 250      // rover/imu publish rate (4 Hz)
#define IMU_COMP_ALPHA 0.98f    // complementary filter: gyro weight vs accel

// ============================================================
// Lidar swing servo (MG996R) + AS5600 angle feedback
// Servo V+ comes from the 12V->6V arm rail (1000 uF cap at the servo end),
// never from the ESP. Common GND. Step-and-settle sweep only: the Pi grabs
// frames while "settled" is 1, so MQTT latency cannot smear the cloud.
// ============================================================
#define SERVO_PIN 27
#define SERVO_MIN_DEG (-35.0f)        // HARD limit (clamped in code too)
#define SERVO_MAX_DEG 35.0f           // HARD limit (clamped in code too)
#define SERVO_SWEEP_MIN_DEG (-30.0f)  // default sweep range
#define SERVO_SWEEP_MAX_DEG 30.0f
#define SERVO_STEP_DEG 5.0f
#define SERVO_SETTLE_MS 500           // pause after a move before settled
#define SERVO_DWELL_MS 600            // hold per step (Pi captures here)
#define SERVO_SETTLE_TOL_DEG 1.5f     // AS5600 must be this close to target
#define SERVO_CENTER_US 1500          // pulse at mount zero (calibrate on bench)
#define SERVO_US_PER_DEG 11.11f       // (2500-500)/180 for an MG996R
#define SERVO_PWM_FREQ 50
#define SERVO_PWM_RES 16
#define AS5600_ADDR 0x36              // fixed address, no ADDR pins

// ============================================================
// MQTT topics (own namespace — never rover/motor/*)
// ============================================================
#define MQTT_CALIB_CMD_TOPIC    "rover/mid/command"
#define MQTT_CALIB_STATUS_TOPIC "rover/mid/status"
#define MQTT_MOTOR_CMD_TOPIC    "rover/mid/command"
#define MQTT_MOTOR_STATUS_TOPIC "rover/mid/status"
// Localization + lidar swing live in their own namespaces so the dashboard
// can tell them apart from motor data.
#define MQTT_IMU_TOPIC          "rover/imu"
#define MQTT_LIDAR_SERVO_TOPIC  "rover/lidar/servo"
#define MQTT_LIDAR_CMD_TOPIC    "rover/lidar/command"

// ============================================================
// Timing
// ============================================================
#define SAMPLE_INTERVAL_MS 250

#endif
