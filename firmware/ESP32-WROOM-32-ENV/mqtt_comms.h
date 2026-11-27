#ifndef MQTT_COMMS_ENV_H
#define MQTT_COMMS_ENV_H

#include <WiFi.h>
#include <PubSubClient.h>
#include <ArduinoJson.h>
#include "config.h"
#include "sensors_env.h"

// Trimmed comms for the ENV board: telemetry out, heartbeat out, commands
// in (baseline capture). No motors, no calibration FSM, no pump.
//
// Static-callback pattern is the same as the corners/MID sketches: one
// manager instance per sketch, pointer file-scope so PubSubClient can reach
// the handler.

class MQTTManager;
static MQTTManager* _mqtt_instance = nullptr;

class MQTTManager {
  private:
    WiFiClient wifi_client;
    PubSubClient mqtt_client;
    const char* ssid;
    const char* password;
    const char* broker;
    int port;
    const char* topic_prefix;
    void (*_cmd_handler)(const char* json) = nullptr;
    char _serial_line[512];
    size_t _serial_len = 0;

    // NaN must become null, never 0: the UI shows "--" for null, and a fake
    // "0" would be worse than no reading at all.
    static void putNum(JsonObject doc, const char* key, float v) {
      if (isnan(v)) {
        doc[key] = (const char*)nullptr;
      } else {
        doc[key] = v;
      }
    }

    static void mqttCallback(char* topic, byte* payload, unsigned int length) {
      if (!_mqtt_instance) return;
      char buf[192];
      if (length >= sizeof(buf)) return;
      memcpy(buf, payload, length);
      buf[length] = '\0';
      if (strcmp(topic, MQTT_ENV_CMD_TOPIC) == 0 &&
          _mqtt_instance->_cmd_handler) {
        _mqtt_instance->_cmd_handler(buf);
      }
    }

  public:
    MQTTManager() : mqtt_client(wifi_client) {
      _mqtt_instance = this;
    }

    void begin(const char* wifi_ssid, const char* wifi_pass,
               const char* mqtt_broker, int mqtt_port,
               const char* topic_pref) {
      ssid = wifi_ssid;
      password = wifi_pass;
      broker = mqtt_broker;
      port = mqtt_port;
      topic_prefix = topic_pref;
#if SERIAL_BRIDGE
      Serial.println("[Bridge] serial mode, WiFi and MQTT skipped");
#else
      connectWiFi();
      mqtt_client.setServer(broker, port);
      mqtt_client.setKeepAlive(30);
      mqtt_client.setCallback(mqttCallback);
#endif
    }

    void connectWiFi() {
      // The portal leaves station WiFi up already; do not flap it.
      if (WiFi.status() == WL_CONNECTED) {
        Serial.print("[WiFi] Already connected. IP: ");
        Serial.println(WiFi.localIP());
        return;
      }
      Serial.print("[WiFi] Connecting to ");
      Serial.println(ssid);
      WiFi.begin(ssid, password);
      int attempts = 0;
      while (WiFi.status() != WL_CONNECTED && attempts < 40) {
        delay(500);
        Serial.print(".");
        attempts++;
      }
      if (WiFi.status() == WL_CONNECTED) {
        Serial.println();
        Serial.print("[WiFi] Connected. IP: ");
        Serial.println(WiFi.localIP());
      } else {
        Serial.println("\n[WiFi] Failed to connect!");
      }
    }

    bool connectMQTT() {
#if SERIAL_BRIDGE
      return true;
#else
      if (!mqtt_client.connected()) {
        String client_id = "rover-env-" + String(random(0xFFFF), HEX);
        Serial.print("[MQTT] Connecting as ");
        Serial.print(client_id);
        Serial.println("...");
        if (mqtt_client.connect(client_id.c_str())) {
          Serial.println("[MQTT] Connected");
          subscribe(MQTT_ENV_CMD_TOPIC);
          return true;
        }
        Serial.print("[MQTT] Failed, rc=");
        Serial.println(mqtt_client.state());
        return false;
      }
      return true;
#endif
    }

    void loop() {
#if SERIAL_BRIDGE
      serialUpdate();
      return;
#else
      if (!mqtt_client.connected()) connectMQTT();
      mqtt_client.loop();
#endif
    }

    void subscribe(const char* topic) {
#if SERIAL_BRIDGE
      (void)topic;
#else
      if (mqtt_client.connected()) {
        mqtt_client.subscribe(topic);
        Serial.print("[MQTT] Subscribed to ");
        Serial.println(topic);
      }
#endif
    }

    bool transportReady() {
#if SERIAL_BRIDGE
      return true;
#else
      return mqtt_client.connected();
#endif
    }

    void publishRaw(const char* topic, const char* buffer) {
#if SERIAL_BRIDGE
      Serial.printf("{\"topic\":\"%s\",\"payload\":%s}\n", topic, buffer);
#else
      mqtt_client.publish(topic, buffer);
#endif
    }

    void serialHello(const char* node) {
#if SERIAL_BRIDGE
      Serial.printf(
          "{\"topic\":\"rover/node/hello\",\"payload\":{\"node\":\"%s\",\"proto\":1}}\n",
          node);
#else
      (void)node;
#endif
    }

    void serialUpdate() {
#if SERIAL_BRIDGE
      while (Serial.available()) {
        char c = (char)Serial.read();
        if (c == '\n') {
          _serial_line[_serial_len] = '\0';
          if (_serial_len > 0) handleSerialLine(_serial_line);
          _serial_len = 0;
        } else if (c != '\r') {
          if (_serial_len < sizeof(_serial_line) - 1) {
            _serial_line[_serial_len++] = c;
          } else {
            _serial_len = 0;
          }
        }
      }
#endif
    }

    void handleSerialLine(const char* line) {
#if SERIAL_BRIDGE
      if (line[0] != '{') return;
      StaticJsonDocument<512> doc;
      if (deserializeJson(doc, line)) return;
      const char* topic = doc["topic"] | "";
      JsonObject payload = doc["payload"];
      char buf[384];
      serializeJson(payload, buf, sizeof(buf));
      if (strcmp(topic, MQTT_ENV_CMD_TOPIC) == 0 && _cmd_handler) {
        _cmd_handler(buf);
      }
#else
      (void)line;
#endif
    }

    void setCommandHandler(void (*handler)(const char* json)) {
      _cmd_handler = handler;
    }

    void publishEnv(const EnvSample& s) {
      if (!transportReady()) return;
      StaticJsonDocument<640> doc;
      doc["ads_ok"] = s.ads_ok ? 1 : 0;
      doc["mlx_ok"] = s.mlx_ok ? 1 : 0;
      doc["flame_ok"] = s.flame_ok ? 1 : 0;
      doc["baseline_ok"] = s.baseline_ok ? 1 : 0;
      // No galvanic cell on this board. The backend estimates O2 from the
      // gas/flame/heat feeds and only does so while this flag is 0, so a
      // future real cell can take over by publishing o2_ok:1 + o2_mv.
      doc["o2_ok"] = 0;
      JsonObject o = doc.as<JsonObject>();
      static const char* keys[GAS_CHANNEL_COUNT] = {"co", "lpg", "ch4", "h2", "air"};
      for (int i = 0; i < GAS_CHANNEL_COUNT; i++) putNum(o, keys[i], s.gas[i]);
      putNum(o, "flame_lvl", s.flame_lvl);
      doc["flame_dir"] = s.flame_dir;
      doc["flame_mask"] = s.flame_mask;
      putNum(o, "heat_l", s.heat_l);
      putNum(o, "heat_r", s.heat_r);
      putNum(o, "heat_l_amb", s.heat_l_amb);
      putNum(o, "heat_r_amb", s.heat_r_amb);
      char buffer[640];
      serializeJson(doc, buffer);
      publishRaw(MQTT_ENV_TOPIC, buffer);
    }

    // Heartbeat: also the rover presence signal, same as the other boards.
    void publishEnvStatus(unsigned long burn_in_s, bool baseline_ok) {
      if (!transportReady()) return;
      StaticJsonDocument<192> doc;
      doc["state"] = "running";
      doc["source"] = "env";
      doc["burn_in_s"] = burn_in_s;
      doc["baseline_ok"] = baseline_ok ? 1 : 0;
      char buffer[192];
      serializeJson(doc, buffer);
      publishRaw(MQTT_ENV_STATUS_TOPIC, buffer);
    }
};

#endif
