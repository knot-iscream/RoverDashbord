#ifndef WIFI_PORTAL_H
#define WIFI_PORTAL_H

// Rover phone WiFi setup (no USB cable, no re-flash).
//
// Boot flow:
//   1. Try the saved home WiFi (stored in flash by WiFiManager) for
//      WIFI_CONNECT_TIMEOUT_S seconds.
//   2. If that fails -> open AP "Rover-Setup" (open network, user choice)
//      with a captive portal. Join it from a phone, pick the home WiFi from
//      the scanned list, type its password + the PC's broker IP, tap Save.
//   3. ESP32 connects to home WiFi and streams to the dashboard (no reboot
//      needed after Save; it reboots only on portal timeout, then retries).
// The portal opens ONLY at boot on failed connect — never mid-run, so a
// shaky bench network can never hijack a calibration.
//
// Requires the "WiFiManager" library by tzapu (Arduino Library Manager).
// WiFiManager persists SSID/password itself; the MQTT broker IP/port are
// persisted below in NVS ("rover" namespace) via a save-params callback.
//
// config.h provides fallback defaults (pre-fill only). No secrets in git.

#include <WiFi.h>
#include <WiFiManager.h>  // tzapu WiFiManager v2
#include <Preferences.h>

// Static-lifetime credential buffers (mqtt_comms.h keeps const char*
// pointers, so these must never be temporaries).
static char _portal_ssid[33] = {0};
static char _portal_pass[65] = {0};
static char _portal_broker[40] = {0};
static int _portal_port = 1883;

static const char* portalSSID() { return _portal_ssid; }
static const char* portalPass() { return _portal_pass; }
static const char* portalBroker() { return _portal_broker; }
static int portalPort() { return _portal_port; }

// NVS handles for the broker fields (WiFiManager does NOT persist custom
// params across reboots, so we do it here).
static WiFiManagerParameter* _prm_broker = nullptr;
static WiFiManagerParameter* _prm_port = nullptr;

static void portalSaveParams() {
  if (!_prm_broker || !_prm_port) return;
  Preferences prefs;
  if (!prefs.begin("rover", false)) {
    Serial.println("[Portal] NVS open failed, broker fields not saved");
    return;
  }
  prefs.putString("broker", _prm_broker->getValue());
  prefs.putString("port", _prm_port->getValue());
  prefs.end();
  Serial.print("[Portal] Saved broker=");
  Serial.print(_prm_broker->getValue());
  Serial.print(" port=");
  Serial.println(_prm_port->getValue());
}

static void portalLoadBrokerDefaults() {
  // Start from config.h fallback defaults, overlay saved NVS values.
  strncpy(_portal_broker, MQTT_BROKER, sizeof(_portal_broker) - 1);
  _portal_port = MQTT_PORT;
  Preferences prefs;
  if (prefs.begin("rover", true)) {
    String b = prefs.getString("broker", MQTT_BROKER);
    String p = prefs.getString("port", String(MQTT_PORT));
    b.toCharArray(_portal_broker, sizeof(_portal_broker));
    _portal_port = p.toInt();
    if (_portal_port <= 0 || _portal_port > 65535) _portal_port = MQTT_PORT;
    prefs.end();
  }
}

// Blocks during first-time setup (inside the portal). Returns true when
// station WiFi is up. Returns false ONLY if the portal timed out with no
// save — caller should ESP.restart() (next boot retries, portal reopens).
static bool portalEnsureWiFi() {
  portalLoadBrokerDefaults();

  WiFi.mode(WIFI_STA);

  WiFiManager wm;
  wm.setTitle("Rover Setup");
  wm.setConnectTimeout(WIFI_CONNECT_TIMEOUT_S);

  char portBuf[8];
  snprintf(portBuf, sizeof(portBuf), "%d", _portal_port);
  static WiFiManagerParameter prmBroker("broker", "MQTT broker IP (your PC)",
                                        _portal_broker, 40);
  static WiFiManagerParameter prmPort("port", "MQTT broker port",
                                      portBuf, 8);
  _prm_broker = &prmBroker;
  _prm_port = &prmPort;
  wm.addParameter(&prmBroker);
  wm.addParameter(&prmPort);
  wm.setSaveParamsCallback(portalSaveParams);

  // Open network per user decision (PORTAL_AP_PASSWORD empty).
  // Timeout then false so a forgotten setup cannot wedge the rover forever.
  wm.setConfigPortalTimeout(PORTAL_TIMEOUT_S);

  Serial.println("[Portal] Trying saved WiFi...");
  bool ok = wm.autoConnect(PORTAL_AP_NAME, PORTAL_AP_PASSWORD);

  _prm_broker = nullptr;
  _prm_port = nullptr;

  if (!ok) {
    Serial.println("[Portal] No WiFi configured yet (portal timed out).");
    Serial.println("[Portal] Rebooting to retry — join Rover-Setup to configure.");
    return false;
  }

  // Snapshot live connection into static buffers (stable pointers for MQTT).
  String s = WiFi.SSID();
  String wmpass = wm.getWiFiPass();
  s.toCharArray(_portal_ssid, sizeof(_portal_ssid));
  wmpass.toCharArray(_portal_pass, sizeof(_portal_pass));

  // Re-read broker fields: save-callback wrote NVS on portal Save; on a
  // plain saved-WiFi boot the NVS values (loaded above) still apply.
  portalLoadBrokerDefaults();

  Serial.print("[Portal] WiFi up: ");
  Serial.print(_portal_ssid);
  Serial.print(" IP: ");
  Serial.println(WiFi.localIP());
  Serial.print("[Portal] Broker: ");
  Serial.print(_portal_broker);
  Serial.print(":");
  Serial.println(_portal_port);
  return true;
}

#endif
