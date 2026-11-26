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
// Motor drivers — one 4-channel 6 A board (RBD-3658, "DC_MOTOR_4")
// Two logic inputs per channel and NO enable pin: one input takes
// the PWM duty, the other stays low. Direction is which input
// pulses, so the old ENA wires simply come off the harness.
// NOTE: GPIO 6-11 are tied to the internal SPI flash on
// ESP32-WROOM and CANNOT be used as general purpose I/O.
// GPIO 1/3 are UART0. GPIO 34-39 are input-only.
// ============================================================

// Input pairs per channel, motor order FL, FR, RL, RR (backend 1-4).
// The four pins per motor keep the old L298N direction wiring.
#define MOTOR1_IN_A 15   // FL  (was L298N1 IN1)
#define MOTOR1_IN_B 14   // FL  (was L298N1 IN2)
#define MOTOR2_IN_A 19   // FR  (was L298N1 IN3)
#define MOTOR2_IN_B 23   // FR  (was L298N1 IN4)
#define MOTOR3_IN_A 33   // RL  (was L298N2 IN1)
#define MOTOR3_IN_B 27   // RL  (was L298N2 IN2)
#define MOTOR4_IN_A 17   // RR  (was L298N2 IN3)
#define MOTOR4_IN_B 16   // RR  (was L298N2 IN4)

// Both inputs of every channel are LEDC channels, so either side
// can carry the duty; motor_driver.h picks the side per command.
#define MOTOR_PWM_FREQ   1000   // Hz
#define MOTOR_PWM_RES    8      // bits (0-255 duty)
#define DEFAULT_MOTOR_SPEED 0   // duty on boot / idle (0-255)
// 0 = coast on zero command (both inputs LOW), the old real
// behaviour. 1 = brake (both inputs HIGH). Confirm on the bench.
#define MOTOR_STOP_BRAKE 0

// Torque ramp: a new speed target is reached over 12 s with a quadratic
// ease-in (slow at first, larger steps near the end). STOP is still
// immediate. Calibration bypasses the ramp, it changes speed far too
// quickly for a 12 s ramp to be useful.
#define TORQUE_RAMP_MS 12000UL

// Command deadman: any command that carries arm_ms must be refreshed within
// that window or every motor on the board stops. Manual and autonomous
// commands share the same 12 s ramp, so a target change can never shock the
// frame.
#define CMD_ARM_DEFAULT_MS 600UL
#define CMD_ARM_MIN_MS     200UL
#define CMD_ARM_MAX_MS     5000UL

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

// INA226 current sensors — four breakout modules on the main I2C bus.
// Their A0/A1 straps come from the wiring schematic (U2, U7, U6, U9):
// motor 1 = 0x41, motor 2 = 0x44, motor 3 = 0x45, motor 4 = 0x40.
// The TCA9548A mux stays in front of the four ADXL345 on channels 0-3
// only. It is never selected for an INA read.
// I2C bus: SDA = GPIO 21, SCL = GPIO 22
#define INA226_ADDR_1 0x41  // Motor 1 (FL), U2
#define INA226_ADDR_2 0x44  // Motor 2 (FR), U7
#define INA226_ADDR_3 0x45  // Motor 3 (RL), U6
#define INA226_ADDR_4 0x40  // Motor 4 (RR), U9

// INA226 calibration — stock R100 0.1 ohm shunt on the breakout.
// The chip accepts +/-81.92 mV across the shunt, so 0.1 ohm tops out
// at 0.819 A. Bench build runs 0.8 A; a 37GB motor pulls more than
// that under load, so the reading clips instead of lying. Swapping
// to 0.01 ohm would raise the range to 8 A but the modules are not
// ours to modify. setMaxCurrentShunt rejects max * shunt > 0.0819.
#define INA226_SHUNT_OHM 0.1f
#define INA226_MAX_A     0.8f

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