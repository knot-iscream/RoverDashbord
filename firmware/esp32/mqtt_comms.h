#ifndef MQTT_COMMS_H
#define MQTT_COMMS_H

#include <WiFi.h>
#include <PubSubClient.h>
#include <ArduinoJson.h>

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

  public:
    MQTTManager() : mqtt_client(wifi_client) {}

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

    void publishMotorData(int motor_id, bool vibration,
                          float voltage, float current, float temperature) {
      unsigned long now = millis();
      if (now - last_publish < 250) return;
      last_publish = now;

      if (!mqtt_client.connected()) return;

      StaticJsonDocument<128> doc;
      doc["motor"] = motor_id;
      doc["vibration"] = vibration ? 1 : 0;
      doc["voltage"] = voltage;
      doc["current"] = current;
      doc["temp"] = temperature;

      char topic[32];
      snprintf(topic, sizeof(topic), "%s/%d", topic_prefix, motor_id);

      char buffer[128];
      size_t n = serializeJson(doc, buffer);

      if (mqtt_client.publish(topic, buffer, n)) {
        Serial.print("[MQTT] Published to ");
        Serial.print(topic);
        Serial.print(": ");
        Serial.println(buffer);
      }
    }
};

#endif
