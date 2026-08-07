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

      connectWiFi();
      mqtt_client.setServer(broker, port);
      mqtt_client.setKeepAlive(30);
      mqtt_client.setCallback(mqttCallback);
    }

    void connectWiFi() {
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
    }

    void loop() {
      if (!mqtt_client.connected()) {
        connectMQTT();
      }
      mqtt_client.loop();
    }

    void subscribe(const char* topic) {
      if (mqtt_client.connected()) {
        mqtt_client.subscribe(topic);
        Serial.print("[MQTT] Subscribed to ");
        Serial.println(topic);
      }
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
      if (!mqtt_client.connected()) return;

      StaticJsonDocument<128> doc;
      doc["state"] = state;
      doc["warmup_pct"] = warmup_pct;
      doc["speed_pct"] = speed_pct;
      doc["dir"] = dir;                       // +1 fwd, -1 rev, 0 idle
      doc["step_remaining_s"] = step_remaining_s;
      doc["cycle"] = cycle;

      char buffer[128];
      size_t n = serializeJson(doc, buffer);
      mqtt_client.publish(MQTT_CALIB_STATUS_TOPIC, buffer, n);
    }

    void publishMotorStatus(bool calibrating, int* speeds) {
      if (!mqtt_client.connected()) return;

      StaticJsonDocument<160> doc;
      doc["state"] = calibrating ? "calibrating" : "running";
      JsonArray arr = doc.createNestedArray("speed");
      for (int i = 0; i < 4; i++) arr.add(speeds[i]);

      char buffer[160];
      size_t n = serializeJson(doc, buffer);
      mqtt_client.publish(MQTT_MOTOR_STATUS_TOPIC, buffer, n);
    }

    void publishMotorData(int motor_id, bool vibration,
                          float voltage, float current,
                          float temperature, int motor_speed = 0) {
      unsigned long now = millis();
      if (now - last_publish < 250) return;
      last_publish = now;

      if (!mqtt_client.connected()) return;

      StaticJsonDocument<192> doc;
      doc["motor"] = motor_id;
      doc["vibration"] = vibration ? 1 : 0;
      doc["voltage"] = voltage;
      doc["current"] = current;
      doc["temp"] = temperature;
      doc["speed"] = motor_speed;   // signed duty (-255..255)

      char topic[32];
      snprintf(topic, sizeof(topic), "%s/%d", topic_prefix, motor_id);

      char buffer[192];
      size_t n = serializeJson(doc, buffer);

      if (mqtt_client.publish(topic, buffer, n)) {
        Serial.print("[MQTT] Published to ");
        Serial.print(topic);
        Serial.print(": ");
        Serial.println(buffer);
      }
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
