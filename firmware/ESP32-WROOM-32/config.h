#ifndef CONFIG_H
#define CONFIG_H

// WiFi + MQTT — FALLBACK DEFAULTS ONLY (used to pre-fill the phone portal
// fields on first boot). Real credentials live in ESP32 flash (NVS) after
// phone setup via the Rover-Setup portal — never put secrets here, this
// file is tracked in git. To change WiFi later, just power-cycle the rover
// with the home network down: the portal reopens by itself, no re-flash.
#define WIFI_SSID "Your_SSID"
#define WIFI_PASSWORD "Your_Password"

// MQTT broker settings (fallback default = this PC; override in portal)
#define MQTT_BROKER "192.168.0.96"
#define MQTT_PORT 1883
#define MQTT_TOPIC_PREFIX "rover/motor"

// ============================================================
// Phone setup portal (wifi_portal.h, WiFiManager by tzapu)
// ============================================================
// Open AP while the rover has no WiFi (user decision: open network).
#define PORTAL_AP_NAME "Rover-Setup"
#define PORTAL_AP_PASSWORD ""   // empty = open network
// How long to try the saved home WiFi before opening the portal.
#define WIFI_CONNECT_TIMEOUT_S 10
// How long the portal stays open with no save before reboot+retry.
// (Reboot reopens it — a forgotten setup can never wedge the rover.)
#define PORTAL_TIMEOUT_S 180

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
#define L298N2_ENB 25   // PWM speed — Motor 4
#define L298N2_IN3 17   // Motor 4 direction
#define L298N2_IN4 16   // Motor 4 direction

// PWM configuration for the L298N enable pins
#define MOTOR_PWM_FREQ   1000   // Hz
#define MOTOR_PWM_RES    8      // bits (0-255 duty)
#define DEFAULT_MOTOR_SPEED 0   // duty on boot / idle (0-255)

// Calibration sweep profile: warm up, then sweep every speed % forward+reverse
#define CALIB_WARMUP_MS    120000  // total warmup (split: 1min +255, 1min -255)
#define CALIB_SWEEP_STEP_MS 120000 // 2 min per speed % (1min FWD + 1min REV)
#define CALIB_HALF_STEP_MS  60000  // one direction half of a step
// duty(pct) = (255 * pct + 50) / 100
// NOTE: CALIB_WARMUP_MS must stay in sync with the backend's 120 s warmup math
// (backend/calibration_manager.py). Re-defining it below caused a mismatch.

// ============================================================
// Sensors
// ============================================================

// DS18B20 temperature — one dedicated OneWire bus PER MOTOR (pin = motor).
// Full build: all four probes wired, one per pin below. Each pin needs a
// 4.7k pull-up to 3.3V (external recommended; the ESP32 internal
// INPUT_PULLUP also runs for insurance). Probe VCC must be 3.3V — these
// GPIOs are NOT 5V-tolerant. -1 = slot disabled -> temp_valid:0.
#define DS18B20_PIN_1 5   // Motor 1 (FL)
#define DS18B20_PIN_2 26  // Motor 2 (FR)
#define DS18B20_PIN_3 2   // Motor 3 (RL)
#define DS18B20_PIN_4 4   // Motor 4 (RR)
#define DS18B20_RESOLUTION 12

// ADXL345 vibration (GY-291) — 4x range sensors via TCA9548A mux.
// One bus cannot hold 4x ADXL345 natively (only 2 I2C addresses:
// 0x53 ALT-LOW / 0x1D ALT-HIGH). All four breakouts stay at 0x53 with
// SDO->GND; the mux puts each on its own channel. Solder A0-A2 on the
// TCA board all open -> mux at 0x70. Shares SDA=21/SCL=22 with INA226.
#define ADXL345_ADDR 0x53
#define TCA9548A_ADDR 0x70
#define ADXL345_CH_1 0   // Motor 1 (FL) on mux channel 0
#define ADXL345_CH_2 1   // Motor 2 (FR) on mux channel 1
#define ADXL345_CH_3 2   // Motor 3 (RL) on mux channel 2
#define ADXL345_CH_4 3   // Motor 4 (RR) on mux channel 3
// |mag - 1g| above this counts as vibration (at rest mag ~= 1g gravity).
#define VIB_THRESHOLD_G 0.25f

// INA226 I2C addresses (each needs unique ADDR pin config).
// I2C bus: SDA = GPIO 21, SCL = GPIO 22
#define INA226_ADDR_1 0x41
#define INA226_ADDR_2 0x44
#define INA226_ADDR_3 0x45
#define INA226_ADDR_4 0x40

// INA226 calibration — 10 mOhm 1 W shunt (stock R100 replaced).
// P = I^2*R -> 1 W / 0.01 ohm = 10 A max; firmware caps at 4 A
// (2x the L298N 2 A channel limit) for finer current_LSB resolution.
#define INA226_SHUNT_OHM 0.01f
#define INA226_MAX_A     4.0f

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
#define SAMPLE_INTERVAL_MS 250

#endif