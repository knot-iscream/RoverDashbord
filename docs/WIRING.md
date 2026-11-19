# Rover wiring, human version

This is the file to read while your hands are on the rover. It is written as
tables and short notes. If you are in a hurry, jump to section 9, the Saturday
checklist.

Every pin number here comes from the three `config.h` files. If a pin changes
in firmware, this file has to change too. The pictures that used to live in
`docs/wiring/` were removed because they were harder to use than a table.

## Read this before you touch a wire

- Turn the power off before moving any wire. The pack can deliver far more
  current than a motor needs.
- Add one wire, check it, then add the next. A mistake found after ten wires
  takes ten times longer to find.
- Grounds first. Every board GND must meet the pack negative at one point.
  Without that, signals float and motors behave randomly.
- Red to plus, black to minus. A reversed supply destroys a driver instantly.
- Keep the wheels off the ground for the first spin of any motor.
- An ESP32 pin never sees more than 3.3 V. Anything 5 V must be divided or
  shifted first. This is the single most common way to kill a board here.

When a table says "check with a meter", a cheap multimeter on DC volts is
enough. You do not need an oscilloscope for anything in this document.

---

## 1. The whole rover in one minute

| Part | What it does | What feeds it |
|---|---|---|
| 3S LiPo 4500 mAh | main battery, 11.1 V nominal | balance charger, off the rover |
| 6 A driver, 4 channels | drives corner motors 1 to 4 | pack through fuse and switch |
| L298N | drives middle motors 5 and 6 | same pack rail |
| XL4016 buck | makes 5.1 V for Pi, LiDAR and the three ESP32s | same pack rail |
| 12 V to 6 V buck | makes 6 V for the arm servo | same pack rail |
| ESP-1 corner | reads corner sensors, sends motor telemetry | USB 5 V from the PC or Pi |
| ESP-2 MID | middle motors, IMU, lidar servo | USB 5 V |
| ESP-3 ENV | gas, flame and heat sensors | USB 5 V |
| Laptop | runs the app and the relay, or reads the Pi's relay | its own power |

The three ESP32 boards are separate computers. A wire on one does not reach
another board just because both use GPIO 21 and 22.

---

## 2. Power connections

Do this section first. Nothing else works until it is right.

| From | To | Check with a meter |
|---|---|---|
| pack + (XT-60) | 20 A fuse | same voltage as the pack |
| fuse out | master switch | switch clicks, no voltage while open |
| switch out | 6 A driver `3-12V` + | about 12 V when closed |
| switch out | L298N `VS` + | about 12 V |
| switch out | XL4016 `IN+` | about 12 V |
| switch out | 12 V to 6 V buck `IN+` | about 12 V |
| every board GND | pack - | all joined at one point, near 0 ohm |

**Why the fuse is 20 A.** Four motors stalling can pull about 10 A. The Pi
rail adds about 2.3 A and the arm about 5.4 A. Together that is roughly 18 A.
A 15 A fuse would blow during normal hard use. Use 12 AWG wire from the pack
to the switch, and 18 AWG for each motor lead.

**A switch is not a fuse.** The 40 A switch can be closed all day with a
shorted wire behind it. The fuse is the only thing that opens on a short.

**The 12.6 V problem.** A full 3S pack is 12.6 V, while the driver label says
3-12 V. That label is a nominal rating, not a hard limit; the chips inside
are rated far higher. Soak one channel at full charge for five minutes and
feel the MOSFETs and the big capacitors before trusting a full pack. Do not
run while charging.

**Charging.** A LiPo needs a balance charger, never the old SLA charger. Take
the pack out of the rover to charge it. A B6-AC class charger takes about an
hour at 1C. The small B3 takes most of a night. Use a LiPo bag.

**Low-voltage protection.** The dashboard warns at 10.2 V, but only near
full throttle, and it reads motor voltage after the driver, not the pack. It
is not a battery protector. A 3S buzzer on the balance lead, set to 3.5 V per
cell, is the real protection. Never discharge below 3.0 V per cell.

**Saturday is different.** You will use a 12 V 5 A wall adapter, not the
LiPo. The 5 A limit protects a miswire, but it also means all six motors
stalling together will make the adapter fold back. That is fine on a smooth
floor.

---

## 3. ESP-1 corner board to the 6 A driver

| Motor | ESP pin A | ESP pin B | Driver header | Goes to |
|---|---|---|---|---|
| 1, front left | 15 | 14 | IN1, IN2 | motor 1 |
| 2, front right | 19 | 23 | IN3, IN4 | motor 2 |
| 3, rear left | 33 | 27 | IN5, IN6 | motor 3 |
| 4, rear right | 17 | 16 | IN7, IN8 | motor 4 |
| all | GND | | header GND | ESP ground |

The eight inputs and one ground are on one 9-pin header. The little 5-pin
JST labelled `IN1 IN2 IN5 IN6 GND` is a duplicate tap for two channels. Use
the 9-pin header.

**There is no enable pin.** This is the big change from the L298N. One input
gets the PWM duty, the other stays low, and direction is decided by which
input pulses.

| Input A | Input B | What the motor does |
|---|---|---|
| HIGH | LOW | forward, full speed |
| LOW | HIGH | reverse, full speed |
| PWM | LOW | forward at that duty |
| LOW | PWM | reverse at that duty |
| LOW | LOW | coast |
| HIGH | HIGH | brake |

The firmware holds both inputs low on a stop and waits half a millisecond
before reversing. That is deliberate. Do not "fix" it.

**3.3 V logic check.** The driver makes its own logic supply from the motor
rail, so it has no 5 V pin. Its inputs may expect 5 V. Before trusting it,
drive one input high from the ESP and measure the pin: if the motor responds
and the pin reads about 3.3 V, fine. If the channel ignores it, add a
74HCT245 level shifter between the ESP and the header. This is the most
likely bench surprise.

**The `A B C D` pads are unconfirmed.** They are small solder pads on the
board. They almost certainly select a mode or are test points, but nobody has
buzzed them out yet. Leave them as shipped. If a channel runs full speed at
every duty, stop and check them before doing anything else.

**Never** connect the ESP 5 V or VIN pin to the driver, and never power the
ESP from the driver. Only the GND and the eight inputs go between them.

---

## 4. Voltage dividers, the part that kills boards

### 4.1 The gas sensor divider, which exists today

The MQ gas modules are powered from 5 V. Their analog output can therefore
rise above 3.3 V. The ADS1115 chip reads that output and must never see more
than about 3.4 V. The fix is a 2 to 1 divider on every MQ signal: two equal
resistors, for example 10 kΩ from module output to the ADS pin and 10 kΩ
from that pin to ground.

| Check | Expected |
|---|---|
| module output with meter | up to about 5 V |
| voltage at the ADS pin | half of that, so at most about 2.5 V |
| `GAS_DIVIDER` in ENV `config.h` | `2.0`, meaning real voltage = measured x 2 |

If you connect a 5 V MQ output straight to the ADS input, the chip dies and
the dashboard shows `--` for every gas. There is no software fix.

The exact resistor values do not need to be perfect. The software reports
`R0/Rs`, a ratio against the clean-air baseline; the load resistor cancels
out. Keep the two resistors in each divider the same value so the ratio
stays honest.

### 4.2 The pack voltage divider, which is NOT fitted

We discussed adding a divider from the battery to a free ESP pin so the
dashboard could show pack voltage directly. We did not fit it. The buzzer
protects the pack, and the dashboard already shows a voltage through the
INA226.

| If you add it later | Value |
|---|---|
| free pin | GPIO 32 (ADC1, safe while WiFi is on) |
| divider | 100 kΩ from pack + to pin, 22 kΩ from pin to ground |
| voltage at the pin at full charge | about 2.27 V, safely under 3.3 V |
| extra part | 100 nF from pin to ground, to quiet noise |
| must do | tie divider ground to the same ground as the pack |

Do not connect 12 V directly to any ESP pin. Even a brief touch destroys it.

---

## 5. INA226 current monitors

Each corner motor has one INA226. It is in series with one motor lead,
between the driver output and the motor terminal.

| Motor | INA226 address |
|---|---|
| 1 | 0x41 |
| 2 | 0x44 |
| 3 | 0x45 |
| 4 | 0x40 |

**It must be in series.** Do not put VIN+ and VIN- across the motor in
parallel. That puts pack voltage across the 10 mΩ shunt and burns it open.
We have seen this on the bench: the chip still answered, about 7 V was
coupled through, and current stayed at 0 A forever.

**The shunt** is 10 mΩ, 1 W, replacing the stock 0.1 Ω part. Thermally it
could take 10 A, but the firmware caps at 4 A for resolution. One motor
stalls around 2.5 A, so that is enough headroom. Reverse drive reads negative
current, which is normal.

**Voltage is after the PWM.** The INA226 reads the averaged motor terminal
voltage, not the pack. At half throttle it reads about half the pack. That is
why the dashboard only flags low voltage near full duty (`|speed| >= 200`)
and compares against 10.2 V. A low number at low speed is not a fault.

---

## 6. The shared data bus and the multiplexer

SDA is GPIO 21 and SCL is GPIO 22 on each board. Devices on one board share
these two wires, plus power and ground.

| Device | Address | Which board | Note |
|---|---|---|---|
| INA226 x4 | 0x40, 0x41, 0x44, 0x45 | ESP-1 | unique addresses, no mux |
| TCA9548A mux | 0x70 | ESP-1 | A0, A1, A2 left open |
| ADXL345 x4 | 0x53 each | ESP-1, behind the mux | SDO to GND on all four |
| ADS1115 A | 0x48 | ESP-3 | four MQ cells |
| ADS1115 B | 0x49 | ESP-3 | air quality and flame level |
| MLX90614 pair | 0x5A, 0x5B | ESP-3 | one re-addressed in EEPROM |
| MPU6050 | 0x68 | ESP-2 | separate board |
| AS5600 | 0x36 | ESP-2 | fixed address |

**Why only one mux.** All four ADXL345 sensors answer at the same address,
0x53. The mux is a switchboard that lets the ESP talk to one at a time.
Everything else already has its own address. The mux has eight channels and
only four are used, so channels 4 to 7 are free for more sensors later.

**The two spare muxes stay in the box.** If you ever fit one, change its
address pads first. Two muxes both at 0x70 answer at once and the symptom
looks like random sensor dropouts, which is miserable to debug.

**Keep the bus short.** All modules here run at 100 kHz on 3.3 V. Long
untwisted wires pick up noise and the motors make it worse. If a sensor
reports `--` while others work, shorten its wires before replacing the part.

---

## 7. Temperature probes

| Motor | DS18B20 data pin |
|---|---|
| 1 | GPIO 5 |
| 2 | GPIO 26 |
| 3 | GPIO 2 |
| 4 | GPIO 4 |

Each probe has its own pin and its own 4.7 kΩ resistor from DATA to 3.3 V.
Do not share one resistor between probes, and do not pull up to 5 V.

VCC goes to 3.3 V, not 5 V. Tape the probe to the motor casing with thermal
paste, otherwise it reads air temperature and the health score lies. A probe
that cannot be read reports `temp_valid:0` and the page shows `--`.

---

## 8. The other two boards

### ESP-2 MID, still on the L298N

| Motor | Backend number | L298N pins |
|---|---|---|
| middle 1 | 5 | ENA 13, IN1 15, IN2 14 |
| middle 2 | 6 | ENB 18, IN3 19, IN4 23 |

| Device | Pin or address | Note |
|---|---|---|
| MPU6050 | 0x68, SDA 21, SCL 22 | publishes at 4 Hz |
| AS5600 | 0x36, SDA 21, SCL 22 | angle only when `cal_ok` |
| lidar servo | GPIO 27 | 50 Hz, hard limits -35 to +35 degrees |

The servo takes power from the 6 V rail, never from the ESP pin. Put a
1000 µF capacitor at the servo end. These two motors have no sensors, so
their temperature and current show `--` by design. Do not chase that as a
fault.

### ESP-3 ENV, no O2 cell

| Device | Wiring |
|---|---|
| MQ-7, MQ-6, MQ-4, MQ-8 | ADS1115 0x48, A0 to A3, 2 to 1 divider each |
| MQ-135 air quality | ADS1115 0x49, A2, same divider |
| flame level | ADS1115 0x49, A3 |
| flame direction | GPIO 34, 35, 36, 39, 32 |
| MLX90614 pair | 0x5A and 0x5B, 20 mm shroud tube each |

ADS1115 B A0 and A1 are now free. The galvanic O2 cell is not fitted because
it cannot be bought here. The board publishes `o2_ok:0` and the backend
derives an estimate from gas, flame and heat. The dashboard says "O2
estimate" and puts a `~` in front of the number. That is an indicator, not a
measurement.

The MQ cells need 24 to 48 hours powered before the clean-air baseline means
anything. Gas readings are relative ratios, not ppm. Fit the shroud tubes on
both MLX sensors or the heat readings will not agree.

---

## 9. Saturday checklist, in the order to do it

| Done | Step | How you know it worked |
|---|---|---|
| [ ] | Pi on, wait for `iscream-hotspot` | the hotspot appears on the laptop |
| [ ] | laptop joins the hotspot, password `789632145` | connected, ignore "no internet" |
| [ ] | open the app folder, set broker to `10.42.0.1`, run `roverdash.exe` | app window opens |
| [ ] | power the driver from the 12 V 5 A adapter | about 12 V at the driver input |
| [ ] | ESP-1 on USB, flash the corner sketch | serial shows the driver line and broker `10.42.0.1` |
| [ ] | join `Rover-Setup` from your phone, choose the hotspot, relay `10.42.0.1` | dashboard shows the board online |
| [ ] | wire motor 1 only: IN1, IN2, GND, motor through INA226 | nothing else connected yet |
| [ ] | command speed 60, then -60 | wheel turns forward, then back |
| [ ] | repeat for motors 2, 3, 4 | all four go forward on +80 |
| [ ] | middle pair on the L298N | +80 matches the corners, swap leads if not |
| [ ] | all six on the floor | straight line, no fighting between wheels |

Use the Control page (`/control`) in the app for the motion checks, or send a
command with `mosquitto_pub`:

```
mosquitto_pub -h 10.42.0.1 -t rover/motor/command -m '{"motor":1,"speed":60}'
mosquitto_pub -h 10.42.0.1 -t rover/motor/command -m '{"motor":1,"speed":0}'
```

`motor:0` means all motors. Negative speed is reverse.

Do not run the calibration sweep on Saturday. It is a long heat test, not a
wiring test.

---

## 10. What you see, what it means

| Symptom | Likely cause | First check |
|---|---|---|
| dashboard never shows the board online | wrong relay address, or portal not saved | serial log prints the broker IP |
| nothing moves, current reads 0 A | no driver power, or no common ground | meter at `3-12V`, then continuity on GND |
| motor buzzes but does not turn at 60 | duty too low, or a loose input | try speed 200, then meter the two input pins |
| full speed at every duty | the board is not in two-input PWM mode | stop, check the `A B C D` pads |
| only one direction works | one input wire is loose | meter both pins while commanding forward |
| motor turns, current stays 0 | INA226 across the motor instead of in series | follow both motor leads, check the shunt |
| a wheel fights the others | motor leads reversed on that wheel | swap the two output leads, not the logic pins |
| ESP reboots when a motor starts | brownout on the USB supply | separate supply, 470 µF at the driver input |
| driver hot within a minute | shorted output or wrong channel map | power off, find the short before trying again |
| all gas readings show `--` | ADS input damaged by 5 V, or no baseline | meter the divider output, then check burn-in |

---

## 11. Not wired on purpose

| Item | Why it is missing | If you need it later |
|---|---|---|
| pack voltage divider | buzzer covers protection, no free time to fit it | GPIO 32, 100 kΩ / 22 kΩ, see section 4.2 |
| LiPo and 20 A fuse | Saturday uses the 12 V 5 A adapter | fit both before the first full-pack run |
| three S low-voltage buzzer | not bought yet | balance lead, 3.5 V per cell |
| galvanic O2 cell | not purchasable here | publish `o2_ok:1` and `o2_mv`, the backend stands down |
| extra ADXL345 on the middles | no sensors on the MID board today | use address 0x1D for the second one, no mux needed |
| spare muxes | only four devices share an address | set their address pads before fitting a second |
| pack charging in the rover | unsafe | charge off the rover with a balance charger |
| PCB schematic with the new driver | not redrawn, still shows two L298N | `PCB/Motors/Motors.kicad_sch` is stale |
| `BOM/Electrical Components.xlsx` | left as the formatted report document | changes are recorded in `BOM/CHANGES.md` |

A missing item from this table is a decision, not a mistake. Anything not in
this table and not working is a fault worth investigating.

---

## 12. Where the numbers came from

- `firmware/ESP32-WROOM-32/config.h`: corner pins, INA226 addresses, mux
  channels, DS18B20 pins, PWM settings.
- `firmware/ESP32-WROOM-32-MID/config.h`: middle motor pins, IMU, servo,
  AS5600.
- `firmware/ESP32-WROOM-32-ENV/config.h`: ADS addresses, gas divider, flame
  pins, MLX addresses.
- `README.md`: power tree, parts list, topic payloads.
- `BOM/CHANGES.md`: what changed in the parts list.

If you change a pin in a config file, change the matching row here. The app
does not discover wiring by itself.
