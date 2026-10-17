#ifndef CONFIG_H
#define CONFIG_H

// ESP-3 "ENV" — gas suite (5x MQ), galvanic O2 cell, 5-channel flame sensor
// and the MLX90614 IR pair. Own MQTT namespace rover/env/*. No motors on
// this board, so no motor_driver.h and no calibration FSM.
//
// Two ADS1115 boards carry the 7 analog inputs (the BOM's own accounting):
//   Board A (0x48): A0 CO, A1 LPG, A2 CH4, A3 H2      (MQ cells, single-ended)
//   Board B (0x49): A0-A1 differential = O2 cell (+/-0.256 V, 7.8 uV/LSB),
//                   A2 = MQ-135 air quality, A3 = flame analog magnitude
// Flame DIRECTIONS are 5 digital lines (module outputs), no ADC needed.
// MQ cells need a 2:1 divider per channel if the ADS runs at 3.3 V, or a
// level-shifted I2C bus if it runs at 5 V. Settle that at the bench.

// WiFi + MQTT — FALLBACK DEFAULTS ONLY (portal pre-fill, no secrets in git).
#define WIFI_SSID "Your_SSID"
#define WIFI_PASSWORD "Your_Password"
#define MQTT_BROKER "192.168.0.96"
#define MQTT_PORT 1883
#define MQTT_TOPIC_PREFIX "rover/env"

// Phone setup portal (WiFiManager by tzapu) — unique AP name per board.
#define PORTAL_AP_NAME "Rover-Setup-ENV"
#define PORTAL_AP_PASSWORD ""
#define WIFI_CONNECT_TIMEOUT_S 10
#define PORTAL_TIMEOUT_S 180

// I2C bus: ADS1115 x2 + MLX90614 x2 share it.
#define I2C_SDA 21
#define I2C_SCL 22

#define ADS_GAS_ADDR 0x48   // CO, LPG, CH4, H2
#define ADS_MIX_ADDR 0x49   // O2 differential, air quality, flame level

// ADC configurations (see ads1115.h for the bit layout)
#define ADS_PGA_GAS 1       // +/-4.096 V full scale for the MQ dividers
#define ADS_PGA_O2  5       // +/-0.256 V for the ~10-13 mV cell
#define ADS_PGA_4V  1       // air quality + flame level share the gas scale

// MQ cells are powered from 5 V; the ADS sees divider-scaled volts.
// GAS_VC = module supply, GAS_DIVIDER = hardware divider ratio (V_actual =
// V_adc * GAS_DIVIDER). Both live here so the bench can correct them once.
#define GAS_VC 5.0f
#define GAS_DIVIDER 2.0f

// MQ-135 "air quality" is a relative indicator, never labelled CO2 ppm.
#define GAS_CHANNEL_COUNT 5  // CO, LPG, CH4, H2, air quality

// Galvanic O2 cell reference: 20.9% O2 in clean air, ~10-13 mV typical.
#define O2_AIR_PCT 20.9f

// MLX90614 pair (both ship at 0x5A; one must be re-addressed to 0x5B via
// EEPROM 0xC7). Shroud each TO-39 with a ~20 mm tube to cut the 90 deg FOV
// to ~20-25 deg, that is what makes the bearing math work.
#define MLX_LEFT_ADDR  0x5A
#define MLX_RIGHT_ADDR 0x5B

// Flame module: 5 digital direction lines. Input-only pins are perfect
// here (no pull-ups needed, the module drives them).
#define FLAME_PIN_COUNT 5
#define FLAME_DIR_PINS {34, 35, 36, 39, 32}

// Timing
#define SAMPLE_INTERVAL_MS 1000   // telemetry publish (1 Hz)
#define STATUS_INTERVAL_MS 2000   // heartbeat
#define BURNIN_FLUSH_MS 300000    // persist powered-time every 5 min

// ============================================================
// MQTT topics
// ============================================================
#define MQTT_ENV_TOPIC        "rover/env/telemetry"
#define MQTT_ENV_STATUS_TOPIC "rover/env/status"
#define MQTT_ENV_CMD_TOPIC    "rover/env/command"

#endif
