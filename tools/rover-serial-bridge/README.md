# Rover serial bridge

Bridges the ESP32 boards from USB serial to the Pi's local MQTT broker.

```
ESP32 --USB serial JSON--> serial_bridge.py --local MQTT--> Pi broker --> PC app
```

The ESPs do not join Wi-Fi in serial mode. The lidar stays on its own USB port
and is never opened by this service.

## Firmware side

`firmware/ESP32-WROOM-32-MID/config.h` has:

```c
#define SERIAL_BRIDGE 1
```

With that set, the sketch skips Wi-Fi and MQTT. It prints one JSON object per
line:

```json
{"topic":"rover/mid/status","payload":{...}}
```

and reads the same envelope for commands. At boot it prints:

```json
{"topic":"rover/node/hello","payload":{"node":"mid","proto":1}}
```

The 600 ms command deadman and the 12 s ramp are unchanged.

## Pi side

```sh
mkdir -p ~/rover-serial-bridge
# copy serial_bridge.py and the unit here
python3 serial_bridge.py
```

The Pi already has `pyserial` (lidar hold) and `paho-mqtt` (camera/agent).

Service:

```sh
sudo cp rover-serial-bridge.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now rover-serial-bridge
journalctl -u rover-serial-bridge -f
```

## Configuration

| Variable | Default | Use |
|---|---|---|
| `BRIDGE_PORTS` | `/dev/ttyUSB1` | comma-separated ESP ports, never the lidar |
| `BRIDGE_DEFAULT_NODE` | `mid` | node before the hello arrives |
| `BRIDGE_BAUD` | `115200` | serial baud |
| `MQTT_BROKER` | `127.0.0.1` | local broker |
| `MQTT_PORT` | `1883` | local broker port |

For all three boards, list the three ESP ports, for example
`/dev/ttyUSB1,/dev/ttyUSB2,/dev/ttyUSB3`. The hello line assigns each port to
`mid`, `corner` or `env`, so the CP210x `0001` serial numbers do not matter.
The shipped unit already lists those three ports and leaves
`BRIDGE_DEFAULT_NODE` empty, so a command is not routed until the board has
introduced itself. The lidar on `/dev/ttyUSB0` is never in the list.

## Topics

The bridge publishes every telemetry topic the ESP prints, unchanged. It
forwards commands on:

- `rover/motor/command` and `rover/calibration/command` to `corner`
- `rover/mid/command`, `rover/lidar/command`, `rover/lidar/tilt_deg`,
  `rover/lidar/tilt_zero` to `mid`
- `rover/env/command` to `env`

It publishes a retained `rover/serial/status` with the port to node map and
the last receive age.

## Safety

- If the USB cable is pulled, the bridge stops writing commands. The firmware
  deadman stops the motors 600 ms later.
- If the bridge dies, the same deadman applies.
- The bridge never opens the lidar port.
- The ESP's Wi-Fi fallback is compiled out in serial mode.
