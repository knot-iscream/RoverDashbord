#ifndef MQTT_COMMS_H
#define MQTT_COMMS_H

#include <WiFi.h>
#include <PubSubClient.h>
#include <ArduinoJson.h>

// Forward declaration for static callback routing
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
    unsigned long last_publish = 0;
    void (*_cmd_handler)(const char* action) = nullptr;
    void (*_motor_cmd_handler)(const char* json) = nullptr;
    char _serial_line[512];
    size_t _serial_len = 0;

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
      // Phone-portal flow leaves station WiFi already up before begin() is
      // called — do not flap a working connection (would drop MQTT).
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
        String client_id = "rover-esp32-" + String(random(0xFFFF), HEX);
        Serial.print("[MQTT] Connecting as ");
        Serial.print(client_id);
        Serial.println("...");

        if (mqtt_client.connect(client_id.c_str())) {
          Serial.println("[MQTT] Connected");
          // Re-subscribe on reconnect
          subscribe(MQTT_CALIB_CMD_TOPIC);
          subscribe(MQTT_MOTOR_CMD_TOPIC);
          return true;
        } else {
          Serial.print("[MQTT] Failed, rc=");
          Serial.println(mqtt_client.state());
          return false;
        }
      }
      return true;
#endif
    }

    void loop() {
#if SERIAL_BRIDGE
      serialUpdate();
      return;
#else
      if (!mqtt_client.connected()) {
        connectMQTT();
      }
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
      if (strcmp(topic, MQTT_CALIB_CMD_TOPIC) == 0 && _cmd_handler) {
        const char* action = payload["action"] | "";
        if (action[0]) _cmd_handler(action);
      } else if (strcmp(topic, MQTT_MOTOR_CMD_TOPIC) == 0 && _motor_cmd_handler) {
        _motor_cmd_handler(buf);
      }
#else
      (void)line;
#endif
    }

    void setCommandHandler(void (*handler)(const char* action)) {
      _cmd_handler = handler;
    }

    void setMotorCommandHandler(void (*handler)(const char* json)) {
      _motor_cmd_handler = handler;
    }

    void publishCalibrationStatus(int warmup_pct, const char* state,
                                  int speed_pct, int dir,
                                  int step_remaining_s, int cycle) {
      if (!transportReady()) return;

      StaticJsonDocument<128> doc;
      doc["state"] = state;
      doc["warmup_pct"] = warmup_pct;
      doc["speed_pct"] = speed_pct;
      doc["dir"] = dir;                       // +1 fwd, -1 rev, 0 idle
      doc["step_remaining_s"] = step_remaining_s;
      doc["cycle"] = cycle;

      char buffer[128];
      serializeJson(doc, buffer);
      publishRaw(MQTT_CALIB_STATUS_TOPIC, buffer);
    }

    void publishMotorStatus(bool calibrating, int* speeds) {
      if (!transportReady()) return;

      StaticJsonDocument<160> doc;
      doc["state"] = calibrating ? "calibrating" : "running";
      JsonArray arr = doc.createNestedArray("speed");
      for (int i = 0; i < 4; i++) arr.add(speeds[i]);

      char buffer[160];
      serializeJson(doc, buffer);
      publishRaw(MQTT_MOTOR_STATUS_TOPIC, buffer);
    }

    void publishMotorData(int motor_id, bool vibration, bool vibration_valid,
                          float voltage, float current,
                          float temperature, bool ina_ok, bool temp_valid,
                          int motor_speed = 0, float vib_mag_g = NAN,
                          float ax_g = NAN, float ay_g = NAN,
                          float az_g = NAN) {
      unsigned long now = millis();
      if (now - last_publish < 250) return;
      last_publish = now;

      if (!transportReady()) return;

      StaticJsonDocument<384> doc;
      doc["motor"] = motor_id;
      doc["vibration"] = vibration ? 1 : 0;
      doc["vib_valid"] = vibration_valid ? 1 : 0;
      doc["ina_ok"] = ina_ok ? 1 : 0;
      doc["temp_valid"] = temp_valid ? 1 : 0;
      doc["voltage"] = voltage;
      doc["current"] = current;
      doc["temp"] = temperature;
      doc["speed"] = motor_speed;   // signed duty (-255..255)
      // Range extension: only sent when the ADXL345 produced a reading.
      // Old backends ignore unknown fields, so this stays compatible.
      if (vib_mag_g == vib_mag_g) doc["vib_mag"] = vib_mag_g;
      if (ax_g == ax_g) doc["ax"] = ax_g;
      if (ay_g == ay_g) doc["ay"] = ay_g;
      if (az_g == az_g) doc["az"] = az_g;

      char topic[32];
      snprintf(topic, sizeof(topic), "%s/%d", topic_prefix, motor_id);

      char buffer[384];
      serializeJson(doc, buffer);

      publishRaw(topic, buffer);
    }

  private:
    static void mqttCallback(char* topic, byte* payload, unsigned int length) {
      if (!_mqtt_instance) return;

      char buf[length + 1];
      memcpy(buf, payload, length);
      buf[length] = '\0';

      if (strcmp(topic, MQTT_CALIB_CMD_TOPIC) == 0 && _mqtt_instance->_cmd_handler) {
        StaticJsonDocument<64> doc;
        DeserializationError err = deserializeJson(doc, buf);
        if (!err) {
          const char* action = doc["action"];
          if (action) {
            _mqtt_instance->_cmd_handler(action);
          }
        }
      } else if (strcmp(topic, MQTT_MOTOR_CMD_TOPIC) == 0
                 && _mqtt_instance->_motor_cmd_handler) {
        _mqtt_instance->_motor_cmd_handler(buf);
      }
    }
};

#endif
