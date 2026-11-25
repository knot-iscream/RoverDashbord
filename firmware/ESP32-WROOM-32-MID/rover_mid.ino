#include "config.h"
#include "wifi_portal.h"   // phone setup (must come after config.h for defaults)
#include "mqtt_comms.h"
#include "motor_driver.h"
#include "sensors_mid.h"

// ESP-2 MID: 2 blind middle motors, plus the localization subsystem:
//   MPU6050 swing arm  -> rover/imu          (4 Hz, yaw drifts on purpose)
//   MG996R swing       -> rover/lidar/servo  (step-and-settle, hard limits)
// No angle encoder is fitted: the servo publishes the commanded angle and
// settles on time. The second MPU6050 at 0x69 is wired but not read yet.
// Motor telemetry shape is unchanged: ina_ok/temp_valid/vib_valid are 0 with
// null readings ("--" upstream), speed is real, backend remaps 1-2 -> 5-6.

MQTTManager mqtt;
MotorDriver driver;
Imu imu;
LidarServo servo;

unsigned long last_sample = 0;
int current_motor = 0;
unsigned long last_status_pub = 0;
unsigned long last_imu_pub = 0;
unsigned long last_servo_pub = 0;
bool last_servo_settled = true;
bool last_servo_sweeping = false;
int last_servo_step = 0;
float last_servo_target = 0;

// Water pump: low-side IRLZ44N on PUMP_PIN. pump_on is the live state
// published in telemetry; pump_off_ms is the firmware deadline, so the
// pump stops even if the browser, backend or broker disappears.
bool pump_on = false;
unsigned long pump_off_ms = 0;

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

void onMotorCommand(const char* json) {
  // The pump rides the same rover/mid/command topic as the motors, so
  // peek at the action and keep the motor parser untouched.
  StaticJsonDocument<128> doc;
  if (!deserializeJson(doc, json) && doc.containsKey("action")) {
    const char* action = doc["action"] | "";
    if (strcmp(action, "pump") == 0) {
      int on = doc["on"] | 1;   // {"action":"pump"} alone means "throw water"
      if (!on) {
        pumpStop();
      } else {
        unsigned long secs = doc["seconds"] | 0;
        pumpStart(secs * 1000UL);
      }
      return;
    }
  }
  driver.applyCommand(json);
}

// {"action":"sweep","min":-30,"max":30,"step":5,"settle_ms":500,"dwell_ms":600}
// {"action":"stop"}                             park at 0
// {"action":"goto","angle":15}                  manual move (clamped)
void onLidarCommand(const char* json) {
  StaticJsonDocument<192> doc;
  if (deserializeJson(doc, json)) {
    Serial.println("[Lidar] Bad command JSON");
    return;
  }
  const char* action = doc["action"] | "";
  if (strcmp(action, "sweep") == 0) {
    servo.startSweep(doc["min"] | SERVO_SWEEP_MIN_DEG,
                     doc["max"] | SERVO_SWEEP_MAX_DEG,
                     doc["step"] | SERVO_STEP_DEG,
                     doc["settle_ms"] | SERVO_SETTLE_MS,
                     doc["dwell_ms"] | SERVO_DWELL_MS);
  } else if (strcmp(action, "stop") == 0) {
    servo.stopSweep();
  } else if (strcmp(action, "goto") == 0) {
    servo.gotoDeg(doc["angle"] | 0.0f);
  } else {
    Serial.print("[Lidar] Unknown action: ");
    Serial.println(action);
  }
}

void setup() {
  Serial.begin(115200);
  delay(500);
  Serial.println("\n===== Rover MID - ESP32 (2 blind motors) =====");

  // Pump gate low before anything else, so a reset mid-burst cannot
  // leave water running.
  pinMode(PUMP_PIN, OUTPUT);
  digitalWrite(PUMP_PIN, LOW);
  pump_on = false;
  pump_off_ms = 0;

  driver.begin();
  imu.begin();
  servo.begin();

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
  mqtt.setLidarCommandHandler(onLidarCommand);

  Serial.println("[Setup] MID + IMU + lidar servo ready");
}

void loop() {
  driver.update();    // advance the 12 s torque ramps
  mqtt.loop();
  imu.update();
  servo.update();

  unsigned long now = millis();

  // Pump deadline: signed difference, so the 49-day millis wrap is safe.
  if (pump_on && (long)(now - pump_off_ms) >= 0) {
    pumpStop();
  }

  if (now - last_sample >= SAMPLE_INTERVAL_MS) {
    last_sample = now;

    // Blind slot: no sensors wired. Zeros + NaN serialize as
    // vibration:0, *_valid:0, readings null — backend renders "--".
    int mid_id = current_motor + 1;  // 1..2 on the wire, 5..6 in backend
    mqtt.publishMotorData(
      mid_id, false, false,
      NAN, NAN, NAN,
      false, false, driver.getSpeed(current_motor),
      NAN, NAN, NAN, NAN, pump_on
    );

    current_motor = (current_motor + 1) % 2;
  }

  if (now - last_status_pub >= 2000) {
    last_status_pub = now;
    int speeds[2] = { driver.getSpeed(0), driver.getSpeed(1) };
    mqtt.publishMotorStatus(false, speeds, pump_on);
  }

  // ---- Localization: IMU at 4 Hz ----
  if (now - last_imu_pub >= IMU_PUBLISH_MS) {
    last_imu_pub = now;
    mqtt.publishImu(imu.isOk(), imu.getAx(), imu.getAy(), imu.getAz(),
                    imu.getGz(), imu.getYaw(), imu.getPitch(), imu.getRoll());
  }

  // ---- Lidar servo: publish on change + a 1 Hz heartbeat ----
  bool servo_changed = (servo.isSettled() != last_servo_settled) ||
                       (servo.isSweeping() != last_servo_sweeping) ||
                       (servo.getStep() != last_servo_step) ||
                       (fabsf(servo.getTarget() - last_servo_target) > 0.05f);
  if (servo_changed || (now - last_servo_pub >= 1000)) {
    last_servo_pub = now;
    last_servo_settled = servo.isSettled();
    last_servo_sweeping = servo.isSweeping();
    last_servo_step = servo.getStep();
    last_servo_target = servo.getTarget();
    mqtt.publishLidarServo(true, servo.getTarget(),
                           servo.isSettled(), servo.isSweeping(),
                           servo.getStep(), servo.getSteps());
  }
}
