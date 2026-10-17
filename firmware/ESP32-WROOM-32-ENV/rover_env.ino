#include "config.h"
#include "wifi_portal.h"     // phone setup (must come after config.h defaults)
#include "sensors_env.h"
#include "mqtt_comms.h"

// ESP-3 "ENV": 5x MQ gas cells, galvanic O2 cell, 5-channel flame module and
// the MLX90614 IR pair. No motors on this board.
//
// Publishes rover/env/telemetry (1 Hz) and rover/env/status (2 s heartbeat),
// and takes one command: {"action":"baseline"} = capture the clean-air
// reference (gas volts + O2 mV) into NVS. Gas cells also need 24-48 h of
// powered burn-in BEFORE that capture means anything; burn time accumulates
// in NVS and rides the heartbeat as burn_in_s.

MQTTManager mqtt;
Ads1115 adsGas;   // 0x48: CO, LPG, CH4, H2  (A0..A3 single-ended)
Ads1115 adsMix;   // 0x49: O2 differential (A0-A1), air quality (A2), flame (A3)
Mlx90614 mlxL, mlxR;
FlameArray flame;

// Clean-air baseline, NVS-persisted. NaN = not captured yet.
float g_v0[GAS_CHANNEL_COUNT] = {NAN, NAN, NAN, NAN, NAN};
float o2_v0_mv = NAN;
bool baseline_ok = false;

// Powered-time accumulator (burn-in proxy across reboots).
unsigned long burn_base_s = 0;
unsigned long boot_ms = 0;

// Set once any flame direction line has gone HIGH (module liveness proof).
bool flame_seen_high = false;

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
  o2_v0_mv = p.getFloat("o2_mv0", NAN);
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
  if (!isnan(o2_v0_mv)) p.putFloat("o2_mv0", o2_v0_mv);
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
  float o2 = adsMix.read(0, ADS_PGA_O2);
  float o2_mv = isnan(o2) ? NAN : o2 * 1000.0f;

  bool any = false;
  for (int i = 0; i < GAS_CHANNEL_COUNT; i++) {
    if (!isnan(v[i]) && v[i] > 0.02f) {
      g_v0[i] = v[i];
      any = true;
    }
  }
  if (!isnan(o2_mv) && o2_mv > 1.0f) o2_v0_mv = o2_mv;
  baseline_ok = any;

  flushBurnIn();
  saveBaseline();

  Serial.printf("[Baseline] gas V: co=%.3f lpg=%.3f ch4=%.3f h2=%.3f air=%.3f\n",
                v[0], v[1], v[2], v[3], v[4]);
  Serial.printf("[Baseline] O2 cell %.2f mV (expect ~10-13 in air), saved=%d\n",
                o2_mv, baseline_ok ? 1 : 0);
  if (!any) {
    Serial.println("[Baseline] Nothing usable: check the ADS wiring/divider");
  }
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

  float gv[5];
  for (int i = 0; i < 4; i++) {
    float adc = adsGas.read(4 + i, ADS_PGA_GAS);
    gv[i] = isnan(adc) ? NAN : adc * GAS_DIVIDER;
    s.gas[i] = gasResponse(gv[i], g_v0[i], GAS_VC);
  }
  float air_adc = adsMix.read(6, ADS_PGA_4V);
  gv[4] = isnan(air_adc) ? NAN : air_adc * GAS_DIVIDER;
  s.gas[4] = gasResponse(gv[4], g_v0[4], GAS_VC);

  float o2 = adsMix.read(0, ADS_PGA_O2);   // differential AIN0-AIN1
  if (!isnan(o2)) {
    s.o2_mv = o2 * 1000.0f;
    if (!isnan(o2_v0_mv) && o2_v0_mv > 1.0f) {
      s.o2_pct = O2_AIR_PCT * (s.o2_mv / o2_v0_mv);
    }
  }

  s.flame_dir = flame.direction();
  if (s.flame_dir > 0) flame_seen_high = true;
  float fl = adsMix.read(7, ADS_PGA_4V);
  s.flame_lvl = isnan(fl) ? NAN : fl * GAS_DIVIDER;
  // flame_ok = the module answers: analog magnitude readable, or a digital
  // line has lit up at least once since boot (same idea as vib_valid).
  // "Fire right now" is flame_dir > 0 / flame_lvl over threshold, which the
  // backend turns into the alarm. Never confuse alive with on-fire.
  s.flame_ok = (!isnan(s.flame_lvl)) || flame_seen_high;

  s.heat_l = mlxL.object();
  s.heat_l_amb = mlxL.ambient();
  s.heat_r = mlxR.object();
  s.heat_r_amb = mlxR.ambient();

  mqtt.publishEnv(s);

  // Bench log: raw volts and temps so wiring problems are visible on USB.
  Serial.printf("[Env] V co=%.3f lpg=%.3f ch4=%.3f h2=%.3f air=%.3f | "
                "o2=%.2fmV | flame dir=%d lvl=%.3fV | mlx L=%.1f/%.1f R=%.1f/%.1f C\n",
                gv[0], gv[1], gv[2], gv[3], gv[4],
                isnan(s.o2_mv) ? 0.0f : s.o2_mv,
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
  Serial.println("\n===== Rover ENV - ESP32 (gas / O2 / flame / MLX) =====");

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
  if (now - last_sample >= SAMPLE_INTERVAL_MS) {
    last_sample = now;
    sampleAndPublish();
  }

  if (now - last_status >= STATUS_INTERVAL_MS) {
    last_status = now;
    mqtt.publishEnvStatus(burnInNow(), baseline_ok);
  }

  if (now - last_burn_flush >= BURNIN_FLUSH_MS) {
    last_burn_flush = now;
    flushBurnIn();
  }
}
