import json
import asyncio
from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from mqtt_handler import MQTTClient
from data_handler import DataHandler

app = FastAPI(title="Rover Digital Twin Backend")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

data_handler = DataHandler()
connected_clients = set()

# ===== WebSocket endpoint for dashboard browsers =====
@app.websocket("/ws")
async def websocket_endpoint(websocket: WebSocket):
    await websocket.accept()
    connected_clients.add(websocket)
    print(f"[WS] Client connected. Total: {len(connected_clients)}")
    try:
        # Send latest data snapshot on connect
        snapshot = data_handler.get_latest()
        if snapshot:
            await websocket.send_json({"type": "snapshot", "data": snapshot})
        while True:
            # Keep connection alive, receive pings
            msg = await websocket.receive_text()
            if msg == "ping":
                await websocket.send_json({"type": "pong"})
    except WebSocketDisconnect:
        pass
    except Exception as e:
        print(f"[WS] Error: {e}")
    finally:
        connected_clients.discard(websocket)
        print(f"[WS] Client disconnected. Total: {len(connected_clients)}")

# ===== Broadcast to all WebSocket clients =====
async def broadcast(payload):
    if not connected_clients:
        return
    message = json.dumps(payload)
    dead = set()
    for client in connected_clients:
        try:
            await client.send_text(message)
        except Exception:
            dead.add(client)
    connected_clients.difference_update(dead)

# ===== MQTT message handler =====
def on_mqtt_message(data):
    motor_data = data_handler.process_motor_data(data)
    motor_id = data.get("motor", data.get("id", 0))
    payload = {
        "type": "motor_update",
        "motor": motor_id,
        "vibration": data.get("vibration", 0),
        "voltage": data.get("voltage", 0.0),
        "current": data.get("current", 0.0),
        "temp": data.get("temp", data.get("temperature", 0.0)),
        "health": motor_data["health"],
        "anomalies": motor_data["anomalies"]
    }
    asyncio.run(broadcast(payload))

# ===== REST endpoints =====
@app.get("/api/health")
async def api_health():
    return {"status": "ok", "clients": len(connected_clients)}

@app.get("/api/motors")
async def api_motors():
    return {"motors": data_handler.get_latest()}

@app.get("/api/calibration")
async def api_calibration():
    return {"baselines": data_handler.baselines}

@app.post("/api/calibration/baseline/{motor_id}")
async def api_set_baseline(motor_id: int, baseline: dict):
    data_handler.set_baseline(motor_id, baseline)
    return {"status": "ok", "motor": motor_id}

# ===== Startup =====
@app.on_event("startup")
async def startup():
    print("[Server] Starting Rover Digital Twin Backend...")
    mqtt = MQTTClient(
        broker="localhost",
        port=1883,
        topic="rover/motor/#",
        on_message_callback=on_mqtt_message
    )
    mqtt.start()

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000)
