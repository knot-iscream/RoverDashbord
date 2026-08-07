import json
import asyncio
import os
from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse
from mqtt_handler import MQTTClient
from data_handler import DataHandler
from calibration_manager import CalibrationManager

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
motor_status = {}

# MQTT client instance (set during startup)
mqtt_client_instance = None

# Calibration manager
cal_manager = CalibrationManager()


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
            await websocket.send_json({
                "type": "snapshot",
                "data": snapshot,
            })

        # Send current calibration status
        cal_status = cal_manager.get_status()
        await websocket.send_json({
            "type": "calibration_status",
            **cal_status,
        })

        while True:
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


# ===== Broadcast helpers =====

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


async def broadcast_calibration_status():
    status = cal_manager.get_status()
    await broadcast({"type": "calibration_status", **status})


# ===== MQTT message handler =====

def on_mqtt_message(data):
    topic = data.pop("_topic", "")

    # ── Calibration status from ESP32 ──
    if topic.startswith("rover/calibration/status"):
        cal_manager.handle_esp32_status(data)
        asyncio.run(broadcast_calibration_status())
        return

    # ── Motor driver status from ESP32 ──
    if topic.startswith("rover/motor/status"):
        motor_status.update(data)
        asyncio.run(broadcast({"type": "motor_status", **data}))
        return

    # ── Motor data ──
    motor_id = data.get("motor", data.get("id", 0))
    motor_data = data_handler.process_motor_data(data)

    # Route to calibration buffer if calib flag is set
    if data.get("calib") == 1 or data.get("calib") == "1":
        cal_manager.record_sample(data)

    payload = {
        "type": "motor_update",
        "motor": motor_id,
        "vibration": data.get("vibration", 0),
        "voltage": data.get("voltage", 0.0),
        "current": data.get("current", 0.0),
        "temp": data.get("temp", data.get("temperature", 0.0)),
        "health": motor_data["health"],
        "anomalies": motor_data["anomalies"],
    }
    asyncio.run(broadcast(payload))


# ===== REST endpoints =====

@app.get("/api/health")
async def api_health():
    return {"status": "ok", "clients": len(connected_clients)}


@app.get("/api/motors")
async def api_motors():
    return {"motors": data_handler.get_latest()}


# ── Motor control (L298N driver via ESP32) ──

@app.post("/api/motor/control")
async def api_motor_control(cmd: dict):
    motor = cmd.get("motor", 0)
    speed = cmd.get("speed", 0)

    # Clamp to [-255, 255]
    speed = max(-255, min(255, int(speed)))

    if mqtt_client_instance:
        mqtt_client_instance.publish("rover/motor/command", {
            "motor": int(motor),
            "speed": speed,
        })
        return {"status": "ok", "motor": motor, "speed": speed, "sent": True}

    return {"status": "error", "detail": "MQTT not connected", "sent": False},


@app.get("/api/motor/status")
async def api_motor_status():
    return motor_status


@app.get("/api/calibration")
async def api_calibration():
    return {"baselines": data_handler.baselines}


@app.post("/api/calibration/baseline/{motor_id}")
async def api_set_baseline(motor_id: int, baseline: dict):
    data_handler.set_baseline(motor_id, baseline)
    return {"status": "ok", "motor": motor_id}


# ── Calibration session endpoints ──

@app.post("/api/calibration/start")
async def api_calibration_start():
    result = cal_manager.start()
    asyncio.create_task(broadcast_calibration_status())
    return result


@app.post("/api/calibration/stop")
async def api_calibration_stop():
    result = cal_manager.stop()
    asyncio.create_task(broadcast_calibration_status())
    return result


@app.get("/api/calibration/status")
async def api_calibration_status():
    return cal_manager.get_status()


@app.get("/api/calibration/export")
async def api_calibration_export():
    filepath = cal_manager.export_excel()
    if filepath is None:
        return {"error": "No calibration data to export"}, 400
    return FileResponse(
        path=filepath,
        filename=os.path.basename(filepath),
        media_type="application/vnd.openxmlformats-officedocument."
                   "spreadsheetml.sheet",
    )


# ===== Startup =====

@app.on_event("startup")
async def startup():
    global mqtt_client_instance
    print("[Server] Starting Rover Digital Twin Backend...")

    mqtt_client_instance = MQTTClient(
        broker="localhost",
        port=1883,
        topics=["rover/motor/#", "rover/calibration/status"],
        on_message_callback=on_mqtt_message,
    )
    # Share MQTT client with calibration manager for outgoing commands
    cal_manager.mqtt_client = mqtt_client_instance
    mqtt_client_instance.start()


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000)
