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

// Same soft start as the corner board: 12 s quadratic ease-in to any
// new speed target, with STOP immediate. The hold mount is the reason.
#define TORQUE_RAMP_MS 12000UL

// ============================================================
// Water pump (RBD-1872, AD20P-1230C): 12 V, ~350 mA, 3.6 W.
// Low-side IRLZ44N on GPIO 25. The gate wire now lands here instead of
// on ESP-3, so this board owns both the command and the reported state.
// GPIO 25 is free on MID: motors use 13/15/14/18/19/23, servo 27.
// ============================================================
#define PUMP_PIN 25
#define PUMP_BURST_MS 10000UL   // default "throw water" burst (about 0.7 L)
#define PUMP_MAX_MS   60000UL   // hard cap on any single command

// ============================================================
// MPU6050 IMU (GY-521) — localization subsystem, NOT motor data.
// Own topic rover/imu. Yaw is gyro-integrated and drifts by design (no
// magnetometer); lidar SLAM corrects it in the Pi phase. Keep the IMU flat,
// stand it off the board and away from the servo leads, or yaw gets noisy.
// ============================================================
#define MPU6050_ADDR 0x68
// Second MPU6050 on the tilt bracket. AD0 to 3.3 V gives 0x69.
#define MPU_BRACKET_ADDR 0x69
#define I2C_SDA 21              // shared body + bracket MPU bus
#define I2C_SCL 22
#define IMU_SAMPLE_MS 20        // internal filter step (50 Hz)
#define IMU_PUBLISH_MS 250      // rover/imu publish rate (4 Hz)
#define IMU_COMP_ALPHA 0.98f    // body complementary filter: gyro weight
#define BRACKET_IMU_COMP_ALPHA 0.96f
#define IMU_ZERO_SAMPLES 50     // bracket zero samples at boot / tilt_zero
#define MPU_GYRO_SENS 131.0f    // LSB per deg/s at +/-250 deg/s
// Flip if the bracket MPU is mounted facing the other way.
#define TILT_SIGN -1.0f

// ============================================================
// Lidar swing servo (MG996R). No angle encoder is fitted: the servo settles
// on time and publishes the commanded angle. The bracket MPU at 0x69 measures
// the real tilt for diagnostics and the tilt_zero flow.
// Servo V+ comes from the 12V->6V arm rail (1000 uF cap at the servo end),
// never from the ESP. Common GND. Step-and-settle sweep only: the Pi grabs
// frames while "settled" is 1, so MQTT latency cannot smear the cloud.
// ============================================================
#define SERVO_PIN 27
#define SERVO_MIN_DEG (-35.0f)        // HARD limit (clamped in code too)
#define SERVO_MAX_DEG 35.0f           // HARD limit (clamped in code too)
#define SERVO_SWEEP_MIN_DEG (-30.0f)  // default sweep range
#define SERVO_SWEEP_MAX_DEG 30.0f
#define SERVO_STEP_DEG 0.5f           // proven bench profile
#define SERVO_SETTLE_MS 200
#define SERVO_DWELL_MS 200
#define SERVO_CENTER_US 1311          // physically flat on this mount
#define SERVO_US_PER_DEG 11.11f       // (2500-500)/180 for an MG996R
#define SERVO_PWM_FREQ 50
#define SERVO_PWM_RES 16
// Commanded moves ramp in small steps instead of slewing at full speed.
#define SERVO_RAMP_STEP_DEG 2.0f
#define SERVO_RAMP_MS 60

// No angle encoder fitted. These two flags stay in the payload so the
// backend and the dashboard contract do not change; they are always 0.
#define SERVO_ENCODER_FITTED 0

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
#define MQTT_TILT_DEG_TOPIC     "rover/lidar/tilt_deg"
#define MQTT_TILT_MEASURED_TOPIC "rover/lidar/tilt_measured"
#define MQTT_TILT_ZERO_TOPIC    "rover/lidar/tilt_zero"

// ============================================================
// Timing
// ============================================================
#define SAMPLE_INTERVAL_MS 250

#endif
