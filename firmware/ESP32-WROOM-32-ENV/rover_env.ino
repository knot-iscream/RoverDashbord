#include "config.h"
#include "wifi_portal.h"     // phone setup (must come after config.h defaults)
#include "sensors_env.h"
#include "mqtt_comms.h"

// ESP-3 "ENV": 5x MQ gas cells, 5-channel flame module and the MLX90614
// IR pair. No motors on this board. The galvanic O2 cell is not fitted,
// so the backend estimates O2 from these feeds instead.
//
// Publishes rover/env/telemetry (1 Hz) and rover/env/status (2 s heartbeat),
// and takes commands on rover/env/command:
//   {"action":"baseline"}                     capture the clean-air reference
//   {"action":"pump","on":1,"seconds":10}     run the water pump (default 10 s)
//   {"action":"pump","on":0}                  stop it now
// Gas cells need 24-48 h of powered burn-in BEFORE a baseline capture means
// anything; burn time accumulates in NVS and rides the heartbeat as burn_in_s.
// The pump has a firmware deadline, so a lost broker cannot leave it running.

MQTTManager mqtt;
Ads1115 adsGas;   // 0x48: CO, LPG, CH4, H2  (A0..A3 single-ended)
Ads1115 adsMix;   // 0x49: air quality (A2), flame (A3); A0/A1 unused
Mlx90614 mlxL, mlxR;
FlameArray flame;

// Clean-air baseline, NVS-persisted. NaN = not captured yet.
float g_v0[GAS_CHANNEL_COUNT] = {NAN, NAN, NAN, NAN, NAN};
bool baseline_ok = false;

// Powered-time accumulator (burn-in proxy across reboots).
unsigned long burn_base_s = 0;
unsigned long boot_ms = 0;

// Water pump: low-side IRLZ44N on PUMP_PIN. pump_on is the live state
// published in telemetry; pump_off_ms is the firmware deadline, so the
// pump stops even if the browser, backend or broker disappears.
bool pump_on = false;
unsigned long pump_off_ms = 0;

unsigned long last_sample = 0;
unsigned long last_status = 0;
unsigned long last_burn_flush = 0;

static unsigned long burnInNow() {
  return burn_base_s + (millis() - boot_ms) / 1000;
}

static void loadBaseline() {
  Preferences p;
  if (!p.begin("rover", true)) return;
  static const char* keys[GAS_CHANNEL_COUNT] = {"v0_co", "v0_lpg", "v0_ch4",
                                                "v0_h2", "v0_air"};
  for (int i = 0; i < GAS_CHANNEL_COUNT; i++) {
    g_v0[i] = p.getFloat(keys[i], NAN);
  }
  baseline_ok = p.getBool("base_ok", false);
  burn_base_s = p.getULong("burn_s", 0);
  p.end();
  Serial.printf("[Baseline] loaded: ok=%d burn_in=%lus\n",
                baseline_ok ? 1 : 0, burn_base_s);
}

static void saveBaseline() {
  Preferences p;
  if (!p.begin("rover", false)) return;
  static const char* keys[GAS_CHANNEL_COUNT] = {"v0_co", "v0_lpg", "v0_ch4",
                                                "v0_h2", "v0_air"};
  for (int i = 0; i < GAS_CHANNEL_COUNT; i++) {
    if (!isnan(g_v0[i])) p.putFloat(keys[i], g_v0[i]);
  }
  p.putBool("base_ok", baseline_ok);
  p.end();
}

static void flushBurnIn() {
  burn_base_s = burnInNow();
  Preferences p;
  if (p.begin("rover", false)) {
    p.putULong("burn_s", burn_base_s);
    p.end();
  }
}

// Reads every analog channel once and stores the clean-air reference.
// Run this in fresh air AFTER the MQ burn-in, never in the lab with fumes.
static void captureBaseline() {
  Serial.println("[Baseline] Capturing clean-air reference...");
  float v[5];
  for (int i = 0; i < 4; i++) {
    float adc = adsGas.read(4 + i, ADS_PGA_GAS);
    v[i] = isnan(adc) ? NAN : adc * GAS_DIVIDER;
    delay(20);
  }
  float air_adc = adsMix.read(6, ADS_PGA_4V);
  v[4] = isnan(air_adc) ? NAN : air_adc * GAS_DIVIDER;

  bool any = false;
  for (int i = 0; i < GAS_CHANNEL_COUNT; i++) {
    if (!isnan(v[i]) && v[i] > 0.02f) {
      g_v0[i] = v[i];
      any = true;
    }
  }
  baseline_ok = any;

  flushBurnIn();
  saveBaseline();

  Serial.printf("[Baseline] gas V: co=%.3f lpg=%.3f ch4=%.3f h2=%.3f air=%.3f\n",
                v[0], v[1], v[2], v[3], v[4]);
  if (!any) {
    Serial.println("[Baseline] Nothing usable: check the ADS wiring/divider");
  }
}

// ------------------------------------------------------------
// Water pump control
// ------------------------------------------------------------
static void pumpStop() {
  digitalWrite(PUMP_PIN, LOW);
  pump_off_ms = 0;
  if (pump_on) Serial.println("[Pump] OFF");
  pump_on = false;
}

// ms = 0 -> default burst. Everything is capped at PUMP_MAX_MS so a bad
// command cannot run the pump dry for minutes.
static void pumpStart(unsigned long ms) {
  if (ms == 0) ms = PUMP_BURST_MS;
  if (ms > PUMP_MAX_MS) ms = PUMP_MAX_MS;
  digitalWrite(PUMP_PIN, HIGH);
  pump_on = true;
  pump_off_ms = millis() + ms;
  Serial.printf("[Pump] ON for %lu ms\n", ms);
}

static void onEnvCommand(const char* json) {
  StaticJsonDocument<128> doc;
  if (deserializeJson(doc, json)) {
    Serial.println("[Env] Bad command JSON");
    return;
  }
  const char* action = doc["action"] | "";
  if (strcmp(action, "baseline") == 0) {
    captureBaseline();
  } else if (strcmp(action, "pump") == 0) {
    int on = doc["on"] | 1;   // {"action":"pump"} alone means "throw water"
    if (!on) {
      pumpStop();
    } else {
      // seconds is clamped by pumpStart; missing/0 falls back to the burst.
      unsigned long ms = (unsigned long)(doc["seconds"] | 0) * 1000UL;
      pumpStart(ms);
    }
  } else {
    Serial.print("[Env] Unknown action: ");
    Serial.println(action);
  }
}

static void sampleAndPublish() {
  EnvSample s;
  s.ads_ok = adsGas.isOk() && adsMix.isOk();
  s.mlx_ok = mlxL.isOk() && mlxR.isOk();
  s.baseline_ok = baseline_ok;
  s.pump_on = pump_on;

  float gv[5];
  for (int i = 0; i < 4; i++) {
    float adc = adsGas.read(4 + i, ADS_PGA_GAS);
    gv[i] = isnan(adc) ? NAN : adc * GAS_DIVIDER;
    s.gas[i] = gasResponse(gv[i], g_v0[i], GAS_VC);
  }
  float air_adc = adsMix.read(6, ADS_PGA_4V);
  gv[4] = isnan(air_adc) ? NAN : air_adc * GAS_DIVIDER;
  s.gas[4] = gasResponse(gv[4], g_v0[4], GAS_VC);

  s.flame_dir = flame.direction();
  float fl = adsMix.read(7, ADS_PGA_4V);
  s.flame_lvl = isnan(fl) ? NAN : fl * FLAME_DIVIDER;
  // flame_ok = the module answers. The analog output is readable whenever
  // the module is powered, so that is the liveness proof here. The old
  // "a line went HIGH" test is useless on an active-low module: HIGH is idle.
  // "Fire right now" is flame_dir > 0, which the backend turns into a verdict.
  // Never confuse alive with on-fire.
  s.flame_ok = !isnan(s.flame_lvl);

  s.heat_l = mlxL.object();
  s.heat_l_amb = mlxL.ambient();
  s.heat_r = mlxR.object();
  s.heat_r_amb = mlxR.ambient();

  mqtt.publishEnv(s);

  // Bench log: raw volts and temps so wiring problems are visible on USB.
  Serial.printf("[Env] V co=%.3f lpg=%.3f ch4=%.3f h2=%.3f air=%.3f | "
                "flame dir=%d lvl=%.3fV | mlx L=%.1f/%.1f R=%.1f/%.1f C\n",
                gv[0], gv[1], gv[2], gv[3], gv[4],
                s.flame_dir,
                isnan(s.flame_lvl) ? 0.0f : s.flame_lvl,
                isnan(s.heat_l) ? 0.0f : s.heat_l,
                isnan(s.heat_l_amb) ? 0.0f : s.heat_l_amb,
                isnan(s.heat_r) ? 0.0f : s.heat_r,
                isnan(s.heat_r_amb) ? 0.0f : s.heat_r_amb);
}

void setup() {
  Serial.begin(115200);
  delay(500);
  Serial.println("\n===== Rover ENV - ESP32 (gas / flame / MLX) =====");

  // Pump gate low before anything else, so a reset mid-burst cannot
  // leave the MOSFET on while the board reboots.
  pinMode(PUMP_PIN, OUTPUT);
  digitalWrite(PUMP_PIN, LOW);
  pump_on = false;
  pump_off_ms = 0;

  Wire.begin(I2C_SDA, I2C_SCL);
  adsGas.begin(ADS_GAS_ADDR, "gas board");
  adsMix.begin(ADS_MIX_ADDR, "mix board");
  mlxL.begin(MLX_LEFT_ADDR, "MLX left");
  mlxR.begin(MLX_RIGHT_ADDR, "MLX right");
  flame.begin();

  loadBaseline();
  boot_ms = millis();

  if (!portalEnsureWiFi()) {
    Serial.println("[Setup] No WiFi yet - rebooting to retry portal");
    delay(1000);
    ESP.restart();
  }

  mqtt.begin(portalSSID(), portalPass(), portalBroker(), portalPort(),
             MQTT_TOPIC_PREFIX);
  mqtt.connectMQTT();
  mqtt.setCommandHandler(onEnvCommand);

  Serial.println("[Setup] ENV ready");
}

void loop() {
  mqtt.loop();

  unsigned long now = millis();

  // Pump deadline: signed difference, so the 49-day millis wrap is safe.
  if (pump_on && (long)(now - pump_off_ms) >= 0) {
    pumpStop();
  }

  if (now - last_sample >= SAMPLE_INTERVAL_MS) {
    last_sample = now;
    sampleAndPublish();
  }

  if (now - last_status >= STATUS_INTERVAL_MS) {
    last_status = now;
    mqtt.publishEnvStatus(burnInNow(), baseline_ok, pump_on);
  }

  if (now - last_burn_flush >= BURNIN_FLUSH_MS) {
    last_burn_flush = now;
    flushBurnIn();
  }
}
