import json
import threading
import paho.mqtt.client as mqtt


class MQTTClient:
    def __init__(self, broker="localhost", port=1883,
                 topics=None, on_message_callback=None):
        self.broker = broker
        self.port = port
        self.topics = topics or ["rover/motor/#", "rover/calibration/status"]
        self.callback = on_message_callback
        self.client = mqtt.Client()
        self.client.on_connect = self._on_connect
        self.client.on_message = self._on_message
        self.client.on_disconnect = self._on_disconnect
        self.running = False
        self.connected = False  # Track actual connection status

    def _on_connect(self, client, userdata, flags, rc):
        print(f"[MQTT] Connected to broker at {self.broker}:{self.port} "
              f"(rc={rc})")
        if rc == 0:
            self.connected = True
            for topic in self.topics:
                self.client.subscribe(topic)
                print(f"[MQTT] Subscribed to {topic}")
        else:
            self.connected = False
            print(f"[MQTT] Connection failed with rc={rc}")

    def _on_disconnect(self, client, userdata, rc):
        self.connected = False
        if rc != 0:
            print(f"[MQTT] Unexpected disconnection (rc={rc})")
        else:
            print("[MQTT] Disconnected cleanly")

    def _on_message(self, client, userdata, msg):
        try:
            payload = json.loads(msg.payload.decode())
            payload["_topic"] = msg.topic
            if self.callback:
                self.callback(payload)
        except json.JSONDecodeError as e:
            print(f"[MQTT] Invalid JSON on {msg.topic}: {e}")
        except Exception as e:
            print(f"[MQTT] Error processing message: {e}")

    def publish(self, topic, payload):
        if isinstance(payload, dict):
            payload = json.dumps(payload)
        result = self.client.publish(topic, payload)
        if result.rc == mqtt.MQTT_ERR_SUCCESS:
            print(f"[MQTT] Published to {topic}: {payload}")
        else:
            print(f"[MQTT] Publish failed to {topic}: rc={result.rc}")

    def start(self):
        self.running = True
        try:
            self.client.connect(self.broker, self.port, keepalive=60)
            thread = threading.Thread(target=self.client.loop_forever,
                                      daemon=True)
            thread.start()
            print(f"[MQTT] Client started on {self.broker}:{self.port}")
        except Exception as e:
            print(f"[MQTT] Failed to connect: {e}")

    def stop(self):
        self.running = False
        self.client.disconnect()
        print("[MQTT] Client stopped")
