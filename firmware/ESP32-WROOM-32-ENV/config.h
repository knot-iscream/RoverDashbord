#ifndef CONFIG_H
#define CONFIG_H

// ESP-3 "ENV" — gas suite (5x MQ), 5-channel flame sensor and the
// MLX90614 IR pair. Own MQTT namespace rover/env/*. No motors on
// this board, so no motor_driver.h and no calibration FSM.
//
// The galvanic O2 cell is no longer fitted (not purchasable here).
// O2 is now estimated in the Rust backend from the gas ratios, the
// flame channels and the IR pair; this board publishes o2_ok:0 so
// the backend knows no real cell is behind the number.
//
// Two ADS1115 boards carry the 6 analog inputs (the BOM's own accounting):
//   Board A (0x48): A0 CO, A1 LPG, A2 CH4, A3 H2      (MQ cells, single-ended)
//   Board B (0x49): A2 = MQ-135 air quality, A3 = flame analog magnitude
//                   A0/A1 are unused now that the O2 cell is gone.
// Flame DIRECTIONS are 5 digital lines (D1..D5 on the module), active LOW:
// the comparator pulls a line down when that sensor sees fire. Power the
// flame module from 3.3 V so its analog output (A1..A5) stays ADS-safe; use
// A3 for the level and no divider. The MQ cells still need a 2:1 divider.

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
#define ADS_MIX_ADDR 0x49   // air quality, flame level (A0/A1 now unused)

// ADC configurations (see ads1115.h for the bit layout)
#define ADS_PGA_GAS 1       // +/-4.096 V full scale for the MQ dividers
#define ADS_PGA_4V  1       // air quality + flame level share the gas scale

// MQ cells are powered from 5 V; the ADS sees divider-scaled volts.
// GAS_VC = module supply, GAS_DIVIDER = hardware divider ratio (V_actual =
// V_adc * GAS_DIVIDER). Both live here so the bench can correct them once.
#define GAS_VC 5.0f
#define GAS_DIVIDER 2.0f

// 5-way flame module (RBD-0423): D1..D5 are active LOW, A3 feeds the level
// straight into ADS1115 B with no divider because the module runs at 3.3 V.
#define FLAME_ACTIVE_LOW 1
#define FLAME_DIVIDER 1.0f

// MQ-135 "air quality" is a relative indicator, never labelled CO2 ppm.
#define GAS_CHANNEL_COUNT 5  // CO, LPG, CH4, H2, air quality

// MLX90614 pair (both ship at 0x5A; one must be re-addressed to 0x5B via
// EEPROM 0xC7). Shroud each TO-39 with a ~20 mm tube to cut the 90 deg FOV
// to ~20-25 deg, that is what makes the bearing math work.
#define MLX_LEFT_ADDR  0x5A
#define MLX_RIGHT_ADDR 0x5B

// Flame module: 5 digital direction lines. Input-only pins are perfect
// here (no pull-ups needed, the module drives them).
#define FLAME_PIN_COUNT 5
#define FLAME_DIR_PINS {34, 35, 36, 39, 32}

// The water pump moved to the MID board (GPIO 25 there). This board keeps
// only the gas, flame and IR sensors.

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
