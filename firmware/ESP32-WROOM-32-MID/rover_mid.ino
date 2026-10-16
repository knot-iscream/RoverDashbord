#include "config.h"
#include "wifi_portal.h"   // phone setup (must come after config.h for defaults)
#include "mqtt_comms.h"
#include "motor_driver.h"

// ESP-2 MID: 2 blind middle motors, no sensors.
// Telemetry shape matches corners so the backend needs no special case:
// ina_ok/temp_valid/vib_valid are 0 with null readings ("--" upstream),
// speed is real. Backend remaps mid 1-2 -> motors 5-6.

MQTTManager mqtt;
MotorDriver driver;

unsigned long last_sample = 0;
int current_motor = 0;
unsigned long last_status_pub = 0;

void onMotorCommand(const char* json) {
  driver.applyCommand(json);
}

void setup() {
  Serial.begin(115200);
  delay(500);
  Serial.println("\n===== Rover MID - ESP32 (2 blind motors) =====");

  driver.begin();

  if (!portalEnsureWiFi()) {
    Serial.println("[Setup] No WiFi yet - rebooting to retry portal");
    delay(1000);
    ESP.restart();
  }

  mqtt.begin(portalSSID(), portalPass(), portalBroker(), portalPort(),
             MQTT_TOPIC_PREFIX);
  mqtt.connectMQTT();
  mqtt.subscribe(MQTT_MOTOR_CMD_TOPIC);
  mqtt.setMotorCommandHandler(onMotorCommand);

  Serial.println("[Setup] MID ready");
}

void loop() {
  mqtt.loop();

  unsigned long now = millis();
  if (now - last_sample >= SAMPLE_INTERVAL_MS) {
    last_sample = now;

    // Blind slot: no sensors wired. Zeros + NaN serialize as
    // vibration:0, *_valid:0, readings null — backend renders "--".
    int mid_id = current_motor + 1;  // 1..2 on the wire, 5..6 in backend
    mqtt.publishMotorData(
      mid_id, false, false,
      NAN, NAN, NAN,
      false, false, driver.getSpeed(current_motor)
    );

    current_motor = (current_motor + 1) % 2;
  }

  if (now - last_status_pub >= 2000) {
    last_status_pub = now;
    int speeds[2] = { driver.getSpeed(0), driver.getSpeed(1) };
    mqtt.publishMotorStatus(false, speeds);
  }
}
