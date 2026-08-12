import json
import asyncio
import os
import time
import uuid
from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse
from mqtt_handler import MQTTClient
from data_handler import DataHandler
from calibration_manager import CalibrationManager
from history_store import HistoryStore

app = FastAPI(title="Rover Digital Twin Backend")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

data_handler = DataHandler()
history_store = HistoryStore()
connected_clients = set()
connected_client_ids = {}  # websocket -> client_id
motor_status = {}

# Device (ESP32) presence — driven by the firmware's ~2s heartbeat on
# rover/motor/status (and any other rover message). A device that stops
# publishing for DEVICE_OFFLINE_TIMEOUT seconds is considered offline.
DEVICE_OFFLINE_TIMEOUT = 8.0
device_last_seen = None
device_online = False

# MQTT client instance (set during startup)
mqtt_client_instance = None

# Running asyncio loop (set during startup) — used to schedule WS
# broadcasts from the MQTT callback thread without cross-loop sends.
main_loop = None

# Calibration manager
cal_manager = CalibrationManager()

# ===== User presence broadcast ─────────────────────────────────

async def broadcast_user_presence():
    """Broadcast current user count to all clients."""
    count = len(connected_clients)
    await broadcast({
        "type": "user_presence",
        "users_online": count,
        "timestamp": time.time(),
    })


# ===== WebSocket endpoint for dashboard browsers =====

@app.websocket("/ws")
async def websocket_endpoint(websocket: WebSocket):
    await websocket.accept()
    client_id = str(uuid.uuid4())[:8]
    connected_clients.add(websocket)
    connected_client_ids[websocket] = client_id
    
    user_count = len(connected_clients)
    print(f"[WS] Client {client_id} connected. Total: {user_count}")
    
    try:
        # Notify ALL clients that a new user has joined
        await broadcast({
            "type": "user_joined",
            "client_id": client_id,
            "users_online": user_count,
            "timestamp": time.time(),
        })
        
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

        # Send current device presence
        await websocket.send_json(get_device_status_payload())

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
        disconnected_id = connected_client_ids.pop(websocket, client_id)
        remaining = len(connected_clients)
        print(f"[WS] Client {disconnected_id} disconnected. Total: {remaining}")
        
        # Notify remaining clients that user left
        asyncio.create_task(broadcast({
            "type": "user_left",
            "client_id": disconnected_id,
            "users_online": remaining,
            "timestamp": time.time(),
        }))


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


# ===== Device presence =====

def get_device_status_payload():
    return {
        "type": "device_status",
        "online": device_online,
        "last_seen": (
            time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(device_last_seen))
            if device_last_seen else None
        ),
    }


def update_device_online():
    """Recompute online state from last_seen; returns True if it changed."""
    global device_online
    was = device_online
    if device_last_seen is None:
        device_online = False
    else:
        device_online = (time.time() - device_last_seen) < DEVICE_OFFLINE_TIMEOUT
    return device_online != was


async def monitor_device_status():
    """Background loop: broadcast device_status only when the state flips."""
    while True:
        try:
            if update_device_online():
                await broadcast(get_device_status_payload())
        except Exception as e:
            print(f"[Device] Monitor error: {e}")
        await asyncio.sleep(2.0)


def schedule(coro):
    """Schedule a broadcast coroutine onto uvicorn's loop from another thread."""
    if main_loop is not None:
        asyncio.run_coroutine_threadsafe(coro, main_loop)


# ===== MQTT message handler =====

def on_mqtt_message(data):
    global device_last_seen
    topic = data.pop("_topic", "")

    # Any rover message proves the ESP32 is alive — but NOT command topics:
    # the backend itself publishes to rover/*/command while also subscribing to
    # rover/motor/#, so its own echoes must not count as a device heartbeat.
    if not topic.endswith("/command"):
        device_last_seen = time.time()

    # ── Calibration status from ESP32 ──
    if topic.startswith("rover/calibration/status"):
        cal_manager.handle_esp32_status(data)
        schedule(broadcast_calibration_status())
        return

    # ── Motor driver status from ESP32 ──
    if topic.startswith("rover/motor/status"):
        motor_status.update(data)
        schedule(broadcast({"type": "motor_status", **data}))
        return

    # ── Motor data ──
    motor_id = data.get("motor", data.get("id", 0))

    # Persist samples to day-sharded history ONLY when the rover is doing
    # something real (a motor is moving or calibration is running). Idle
    # publishes (speed:0 with unplugged sensors) are junk — skip them so
    # history/Excel/segments never fill with fabricated idle telemetry.
    running = int(data.get("speed", 0) or 0) != 0
    calibrating = cal_manager.state in ("warmup", "sweep")
    if running or calibrating:
        history_store.add_sample(data)

    # Count calibration samples while the sweep is running
    if cal_manager.state in ("warmup", "sweep"):
        cal_manager.count_sample()

    motor_data = data_handler.process_motor_data(data)

    # Pass through sensor validity flags and keep raw nulls (invalid INA219 /
    # missing temp probe) so pages render "--" instead of garbage numbers.
    # Missing flags default to FALSE: a packet that doesn't vouch for its
    # sensors is never trusted (old firmware / unplugged modules show "--").
    ina_ok = bool(data.get("ina_ok", False))
    vibration_valid = bool(data.get(
        "vibration_valid", data.get("vib_valid", False)))
    temp_valid = bool(data.get("temp_valid", False))

    payload = {
        "type": "motor_update",
        "motor": motor_id,
        "vibration": data.get("vibration", 0),
        "vibration_valid": vibration_valid,
        "ina_ok": ina_ok,
        "temp_valid": temp_valid,
        "voltage": data.get("voltage", None) if ina_ok else None,
        "current": data.get("current", None) if ina_ok else None,
        "temp": data.get("temp", data.get("temperature", None))
               if temp_valid else None,
        "speed": data.get("speed", 0),
        "health": motor_data["health"],
        "anomalies": motor_data["anomalies"],
    }
    schedule(broadcast(payload))


# ===== REST endpoints =====

@app.get("/api/health")
async def api_health():
    return {"status": "ok", "clients": len(connected_clients)}


@app.get("/api/device/status")
async def api_device_status():
    update_device_online()
    payload = get_device_status_payload()
    payload.pop("type", None)
    return payload


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
        # Broadcast to all connected clients so they see motor state change
        asyncio.create_task(broadcast({
            "type": "motor_control",
            "motor": int(motor),
            "speed": speed,
        }))
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
    filepath = history_store.export_excel()
    if filepath is None:
        return {"error": "No recorded data to export"}, 400
    return FileResponse(
        path=filepath,
        filename=os.path.basename(filepath),
        media_type="application/vnd.openxmlformats-officedocument."
                   "spreadsheetml.sheet",
    )


# ── History timeline queries ──

@app.get("/api/history/days")
async def api_history_days():
    return {"days": history_store.days()}


@app.get("/api/history/segments")
async def api_history_segments(day: str):
    return {"day": day, "segments": history_store.segments(day)}


# ── Calibration records (universal to all clients) ──

@app.get("/api/records/days")
async def api_records_days():
    """Return list of days with available calibration/recording history."""
    return {"days": history_store.days()}


@app.get("/api/records/{day}")
async def api_records_by_day(day: str):
    """Return all motor samples recorded on a given day (for universal replay)."""
    samples = history_store._read_day(day)
    if not samples:
        return {"day": day, "samples": [], "count": 0}
    return {
        "day": day,
        "samples": samples,
        "count": len(samples),
        "motors_recorded": list(set(s.get("motor", 0) for s in samples if s.get("motor")))
    }


# ===== Startup =====

@app.on_event("startup")
async def startup():
    global mqtt_client_instance, main_loop
    print("[Server] Starting Rover Digital Twin Backend...")

    main_loop = asyncio.get_running_loop()

    mqtt_client_instance = MQTTClient(
        broker="localhost",
        port=1883,
        topics=["rover/motor/#", "rover/calibration/status"],
        on_message_callback=on_mqtt_message,
    )
    # Share MQTT client with calibration manager for outgoing commands
    cal_manager.mqtt_client = mqtt_client_instance
    mqtt_client_instance.start()

    # Start device presence monitor
    asyncio.create_task(monitor_device_status())


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000)
