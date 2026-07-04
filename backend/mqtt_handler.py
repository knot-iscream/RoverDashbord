import json
import threading
import paho.mqtt.client as mqtt

class MQTTClient:
    def __init__(self, broker="localhost", port=1883, topic="rover/motor/#", on_message_callback=None):
        self.broker = broker
        self.port = port
        self.topic = topic
        self.callback = on_message_callback
        self.client = mqtt.Client()
        self.client.on_connect = self._on_connect
        self.client.on_message = self._on_message
        self.running = False

    def _on_connect(self, client, userdata, flags, rc):
        print(f"[MQTT] Connected to broker at {self.broker}:{self.port} (rc={rc})")
        if rc == 0:
            self.client.subscribe(self.topic)
            print(f"[MQTT] Subscribed to {self.topic}")
        else:
            print(f"[MQTT] Connection failed with rc={rc}")

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

    def start(self):
        self.running = True
        try:
            self.client.connect(self.broker, self.port, keepalive=60)
            thread = threading.Thread(target=self.client.loop_forever, daemon=True)
            thread.start()
            print(f"[MQTT] Client started, connecting to {self.broker}:{self.port}")
        except Exception as e:
            print(f"[MQTT] Failed to connect: {e}")

    def stop(self):
        self.running = False
        self.client.disconnect()
        print("[MQTT] Client stopped")
