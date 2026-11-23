# Wiring and connections, table version

| How to read this file | What to do |
|---|---|
| Every useful fact is a table row | Nothing is hidden in a paragraph |
| Tick the first column when a row is done | Change ☐ to ☑ in your own copy |
| "Net" means the label in EasyEDA | Same label joins two pins without a wire |
| Value and type are always written together | `10 kΩ resistor`, `0.1 µF ceramic` |
| "Ref" is the designator on the EasyEDA sheet | U2 is the INA226 for motor 1, for example |
| Both legs of every part are listed | A cell that says "open" means no connection |
| This build is a trainer board and a DIY harness | Nothing is soldered to a custom PCB |
| 12 V adapter first, LiPo for the floor test | The adapter cannot feed all six motors |

## Parts to have on the bench

| ☐ | Part | Value and type | Quantity | Goes where |
|---|---|---|---|---|
| ☐ | Ceramic capacitor | 0.1 µF, 50 V | 6 | across the two terminals of each motor (C1 to C6) |
| ☐ | Electrolytic capacitor | 470 µF, 25 V | 2 | C7 at the corner driver input, C8 at the L298N VS input |
| ☐ | Electrolytic capacitor | 1000 µF, 16 V | 1 | C9 across the servo 6 V rail |
| ☐ | Resistor | 10 kΩ | 10 | two for each of the five gas dividers |
| ☐ | Resistor | 10 kΩ | 1 | pump gate pull-down |
| ☐ | Resistor | 100 Ω | 1 | pump gate series resistor |
| ☐ | Resistor | 4.7 kΩ | 4 | one pull-up for each DS18B20 data line |
| ☐ | MOSFET | IRLZ44N, logic level, TO-220 | 1 | low-side switch for the water pump |
| ☐ | Diode | 1N5408, 3 A, 1000 V | 1 | flyback across the water pump |
| ☐ | Electrolytic capacitor | 100 µF, 25 V | 1 | across the pump terminals |
| ☐ | Blade fuse and holder | 2 A | 1 | pump branch only |
| ☐ | DC power socket or pigtail | 5.5 mm outside, 2.1 mm inside, centre positive | 1 | pump jack to the harness |
| ☐ | Level shifter | 74HCT245, optional | 1 | only if the corner driver needs 5 V logic |
| ☐ | Wire | 12 AWG, red and black | 2 m | pack, fuse, switch, driver input |
| ☐ | Wire | 18 AWG, red and black | 6 m | motors, pump, module power |
| ☐ | Jumper wire | 22 AWG, assorted colours | 1 set | I2C, sensors, gate signals |

## Reference key for the EasyEDA sheet

| Ref | Part | Where it sits |
|---|---|---|
| U1 | RBD-3658 4-channel 6 A driver | ESP-1, corner motors |
| U2 | INA226 current sensor | motor 1, front left |
| U7 | INA226 current sensor | motor 2, front right |
| U6 | INA226 current sensor | motor 3, rear left |
| U9 | INA226 current sensor | motor 4, rear right |
| U3 | ESP32-WROOM-32 | ESP-1, corner board |
| U10 | ESP32-WROOM-32 | ESP-2, middle board |
| U16 | ESP32-WROOM-32 | ESP-3, environment board |
| U4 | L298N dual H-bridge | ESP-2, middle motors |
| M8 | TCA9548A 8-channel I2C mux | ESP-1, I2C hub |
| P1 to P4 | ADXL345 vibration sensor | motors 1 to 4, mux channels 0 to 3 |
| U17 | ADS1115, address 0x48 | ESP-3, four gas channels |
| U18 | ADS1115, address 0x49 | ESP-3, air quality and flame level |
| U19 | MLX90614, address 0x5A | ESP-3, left IR |
| U20 | MLX90614, address 0x5B | ESP-3, right IR |
| U14 | MPU6050, address 0x68 | ESP-2, rover orientation |
| U15 | MPU6050, address 0x69 | ESP-2, tilt bracket, not read yet |
| U26 | 5-channel flame module | ESP-3 |
| M1 to M6 | drive motors | four corners, two middle |
| C1 to C6 | 0.1 µF motor capacitors | one per motor |
| C7, C8 | 470 µF electrolytic | driver input and L298N input |
| C9 | 1000 µF electrolytic | servo rail |

## The three boards at a glance

| Board | Name in code | Ref | Talks to | Power in |
|---|---|---|---|---|
| ESP-1 | corner | U3 | U1 driver, four INA226, M8 mux, four ADXL345, four DS18B20 | USB 5 V from the Pi or PC |
| ESP-2 | MID | U10 | U4 L298N, two MPU6050, MG996R servo | USB 5 V from the Pi or PC |
| ESP-3 | ENV | U16 | two ADS1115, two MLX90614, five MQ cells, flame module, pump MOSFET | USB 5 V from the Pi or PC |

## Power tree, 12 V side

| ☐ | From | Part | Value and type | To | Voltage there |
|---|---|---|---|---|---|
| ☐ | pack positive, XT-60 | fuse | 20 A blade | master switch | 9.0 to 12.6 V |
| ☐ | master switch | wire | 12 AWG red | U1 terminal 3-12V | pack voltage |
| ☐ | master switch | wire | 12 AWG red | U4 terminal VS | pack voltage |
| ☐ | master switch | wire | 18 AWG red | XL4016 input positive | pack voltage |
| ☐ | master switch | wire | 18 AWG red | 12 V to 6 V buck input positive | pack voltage |
| ☐ | master switch | wire | 18 AWG red | 2 A pump fuse | pack voltage |
| ☐ | 2 A pump fuse | wire | 18 AWG red | pump positive, red wire | pack voltage |
| ☐ | pack negative, XT-60 | wire | 12 AWG black | common ground point | 0 V |
| ☐ | U1 GND | wire | 18 AWG black | common ground point | 0 V |
| ☐ | U4 GND | wire | 18 AWG black | common ground point | 0 V |
| ☐ | XL4016 input negative | wire | 18 AWG black | common ground point | 0 V |
| ☐ | 6 V buck input negative | wire | 18 AWG black | common ground point | 0 V |
| ☐ | pump MOSFET source, pin 3 | wire | 18 AWG black | common ground point | 0 V |

## Power tree, 5 V side

| ☐ | From | Part | Value and type | To | Voltage there |
|---|---|---|---|---|---|
| ☐ | XL4016 output positive | regulator | set to 5.1 V, 8 A | Pi 5 5 V input, LiDAR, USB hub | 5.1 V |
| ☐ | USB hub | USB cable | data and 5 V | U3, U10, U16 USB ports | 5 V |
| ☐ | XL4016 output negative | wire | 18 AWG black | common ground point | 0 V |
| ☐ | XL4016 5.1 V output | wire | 18 AWG red | U1 5 V pin, only if the check below reads 0 V | 5.1 V |

## Power tree, 6 V servo side

| ☐ | From | Part | Value and type | To | Voltage there |
|---|---|---|---|---|---|
| ☐ | 6 V buck output positive | regulator | set to 6.0 V, 10 A | MG996R servo V+ | 6.0 V |
| ☐ | C9 positive leg | capacitor | 1000 µF, 16 V | servo V+ | 6.0 V |
| ☐ | C9 negative leg | capacitor | 1000 µF, 16 V | servo GND | 0 V |
| ☐ | 6 V buck output negative | wire | 18 AWG black | common ground point | 0 V |
| ☐ | servo GND, brown or black wire | wire | 18 AWG black | common ground point | 0 V |

## Power tree, 3.3 V sensor rails

| ☐ | Rail | Source | Load | Voltage |
|---|---|---|---|---|
| ☐ | ESP-1 sensors | U3 3V3 pin | M8 mux, four INA226, four ADXL345, four 4.7 kΩ pull-ups | 3.3 V |
| ☐ | ESP-2 sensors | U10 3V3 pin | U14, U15 | 3.3 V |
| ☐ | ESP-3 sensors | U16 3V3 pin | U17, U18, U19, U20, U26 flame module | 3.3 V |
| ☐ | every 3V3 load ground | wire 22 AWG black | the same board's GND pin | 0 V |

## Current budget, so nothing is overloaded

| ☐ | Load | Normal current | Stall or peak | Fed from |
|---|---|---|---|---|
| ☐ | one corner motor, 37GB | 0.6 to 1.0 A | about 2.5 A | 12 V rail through U1 |
| ☐ | four corner motors | about 3 A | about 10 A | 12 V rail through U1 |
| ☐ | one middle motor | 0.4 to 0.8 A | about 2 A | 12 V rail through U4 |
| ☐ | two middle motors | about 1.2 A | about 4 A | 12 V rail through U4 |
| ☐ | water pump | 0.35 A | 0.5 A at start | 12 V rail, 2 A fuse |
| ☐ | Pi 5, LiDAR, three ESP32 | about 1.5 A at 5 V | 2.5 A at 5 V | XL4016 |
| ☐ | MG996R servo | 0.3 A idle | 2.5 A stall | 6 V buck |
| ☐ | bench 12 V 5 A adapter | one corner motor plus pump, or the two middle motors alone | never four corner motors | 12 V rail |
| ☐ | 3S LiPo 4500 mAh | floor test and full rover | 20 A fuse covers all six stalled | 12 V rail |

## ESP-1, driver and motor connections

| ☐ | Motor | ESP board label | GPIO in code | U1 header pin | U1 output |
|---|---|---|---|---|---|
| ☐ | 1 front left | D15 | 15 | IN1 | OUT1 and OUT2 |
| ☐ | 1 front left | D14 | 14 | IN2 | OUT1 and OUT2 |
| ☐ | 2 front right | D19 | 19 | IN3 | OUT3 and OUT4 |
| ☐ | 2 front right | D23 | 23 | IN4 | OUT3 and OUT4 |
| ☐ | 3 rear left | D33 | 33 | IN5 | OUT5 and OUT6 |
| ☐ | 3 rear left | D27 | 27 | IN6 | OUT5 and OUT6 |
| ☐ | 4 rear right | TX2 | 17 | IN7 | OUT7 and OUT8 |
| ☐ | 4 rear right | RX2 | 16 | IN8 | OUT7 and OUT8 |
| ☐ | all four channels | GND | GND | header GND | reference for the logic inputs |

| ☐ | Motor | From | Through | To | Then |
|---|---|---|---|---|---|
| ☐ | 1 front left | U1 OUT1 | U2 VIN+ to VIN− | M1 terminal B | M1 terminal A to U1 OUT2 |
| ☐ | 2 front right | U1 OUT3 | U7 VIN+ to VIN− | M2 terminal B | M2 terminal A to U1 OUT4 |
| ☐ | 3 rear left | U1 OUT5 | U6 VIN+ to VIN− | M3 terminal B | M3 terminal A to U1 OUT6 |
| ☐ | 4 rear right | U1 OUT7 | U9 VIN+ to VIN− | M4 terminal B | M4 terminal A to U1 OUT8 |

| ☐ | INA226 pin | Connects to | Note |
|---|---|---|---|
| ☐ | VIN+ | U1 output for that motor | current enters here |
| ☐ | VIN− | motor terminal B | current leaves here |
| ☐ | VBS | 12 V rail after the master switch | bus voltage stays readable while PWM switches |
| ☐ | Alert | open | firmware does not use it |
| ☐ | VCC | 3V3 rail | see the module table below |
| ☐ | GND | GND rail | one ground for every module |
| ☐ | SDA, SCL | GPIO 21 and GPIO 22 | main bus, one net for all four modules |

| ☐ | Part | Ref | Value and type | First leg | Second leg |
|---|---|---|---|---|---|
| ☐ | Motor capacitor | C1 | 0.1 µF ceramic | M1 terminal A | M1 terminal B |
| ☐ | Motor capacitor | C2 | 0.1 µF ceramic | M2 terminal A | M2 terminal B |
| ☐ | Motor capacitor | C3 | 0.1 µF ceramic | M3 terminal A | M3 terminal B |
| ☐ | Motor capacitor | C4 | 0.1 µF ceramic | M4 terminal A | M4 terminal B |
| ☐ | Motor capacitor | C5 | 0.1 µF ceramic | M5 terminal A | M5 terminal B |
| ☐ | Motor capacitor | C6 | 0.1 µF ceramic | M6 terminal A | M6 terminal B |
| ☐ | Driver capacitor | C7 | 470 µF, 25 V | U1 3-12V positive | U1 3-12V negative |
| ☐ | L298N capacitor | C8 | 470 µF, 25 V | U4 VS positive | U4 GND |
| ☐ | Servo capacitor | C9 | 1000 µF, 16 V | servo V+ | servo GND |

| ☐ | Note | Detail |
|---|---|---|
| ☐ | C7 and C8 stripe | the striped leg is negative, to ground |
| ☐ | 16 V parts | safe on the 12 V rail, use 25 V if you have them |
| ☐ | INA226 reads negative in reverse | the firmware publishes signed current, that is expected |
| ☐ | stock shunt is 0.1 Ω, marked R100 | current clips at 0.8 A, voltage stays valid |
| ☐ | 2 A over-current alarm is inactive | 0.8 A is below the 2 A rule, do not expect it to fire |

## ESP-1, shared I2C bus and mux

| ☐ | Net | U3 pin | Goes to | Note |
|---|---|---|---|---|
| ☐ | SDA | GPIO 21 | M8 SDA and U2, U7, U6, U9 SDA | main bus, shared by mux and all four INA |
| ☐ | SCL | GPIO 22 | M8 SCL and U2, U7, U6, U9 SCL | main bus, shared by mux and all four INA |
| ☐ | 3V3 | 3V3 pin | M8 VCC | mux logic supply |
| ☐ | GND | GND pin | M8 GND | common ground |

| ☐ | M8 pin | Set to | Result |
|---|---|---|---|
| ☐ | A0 | GND | address bit 0 = 0 |
| ☐ | A1 | GND | address bit 1 = 0 |
| ☐ | A2 | GND | address bit 2 = 0 |
| ☐ | RESET | 3V3 | mux enabled, address 0x70 |

| ☐ | Mux channel | SDn goes to | SCn goes to | Module | Motor |
|---|---|---|---|---|---|
| ☐ | 0 | P1 SDA | P1 SCL | ADXL345 number 1 | 1 front left |
| ☐ | 1 | P2 SDA | P2 SCL | ADXL345 number 2 | 2 front right |
| ☐ | 2 | P3 SDA | P3 SCL | ADXL345 number 3 | 3 rear left |
| ☐ | 3 | P4 SDA | P4 SCL | ADXL345 number 4 | 4 rear right |

The four INA226 are not on the mux. They share the main bus at different
addresses, one per motor:

| ☐ | Motor | Ref | I2C address | A0 strap | A1 strap |
|---|---|---|---|---|---|
| ☐ | 1 front left | U2 | 0x41 | 3V3 | GND |
| ☐ | 2 front right | U7 | 0x44 | GND | 3V3 |
| ☐ | 3 rear left | U6 | 0x45 | 3V3 | 3V3 |
| ☐ | 4 rear right | U9 | 0x40 | GND | GND |

| ☐ | Module | Pin | Set to | Result |
|---|---|---|---|---|
| ☐ | INA226 U2, U6, U7, U9 | A0, A1 | as in the table above | sets the four different addresses |
| ☐ | INA226 U2, U6, U7, U9 | VCC | 3V3 rail | powered, shared |
| ☐ | INA226 U2, U6, U7, U9 | GND | GND rail | shared |
| ☐ | ADXL345 P1 to P4 | CS | 3V3 | I2C mode |
| ☐ | ADXL345 P1 to P4 | SDO | GND | address 0x53 |
| ☐ | ADXL345 P1 to P4 | VCC | 3V3 rail | powered, shared |
| ☐ | ADXL345 P1 to P4 | GND | GND rail | shared |

| ☐ | Warning | Why |
|---|---|---|
| ☐ | one mux channel active at a time | the firmware selects before every ADXL345 read |
| ☐ | all four INA226 have different addresses | the mux only carries the ADXL345, not the INA |
| ☐ | do not tie VBS to the motor side of the shunt | the PWM off time reads 0 V and the frame goes invalid |
| ☐ | keep the I2C wires short | long stubs at 400 kHz pick up motor noise |

## ESP-1, temperature probes

| ☐ | Probe | Data net | U3 pin | Pull-up resistor | Pull-up legs |
|---|---|---|---|---|---|
| ☐ | Temperature 1 | DS_M1 | GPIO 5 | 4.7 kΩ | DS_M1 to 3V3 |
| ☐ | Temperature 2 | DS_M2 | GPIO 26 | 4.7 kΩ | DS_M2 to 3V3 |
| ☐ | Temperature 3 | DS_M3 | GPIO 2 | 4.7 kΩ | DS_M3 to 3V3 |
| ☐ | Temperature 4 | DS_M4 | GPIO 4 | 4.7 kΩ | DS_M4 to 3V3 |
| ☐ | every probe VDD | 3V3 | 3.3 V only | not 5 V | the GPIOs are not 5 V tolerant |
| ☐ | every probe GND | GND | common ground | | |

## ESP-2, middle board

| ☐ | Motor | U10 pin | GPIO | U4 pin | U4 output |
|---|---|---|---|---|---|
| ☐ | middle 1, app motor 5 | D13 | 13 | ENA | OUT1 and OUT2 |
| ☐ | middle 1, app motor 5 | D15 | 15 | IN1 | OUT1 and OUT2 |
| ☐ | middle 1, app motor 5 | D14 | 14 | IN2 | OUT1 and OUT2 |
| ☐ | middle 2, app motor 6 | D18 | 18 | ENB | OUT3 and OUT4 |
| ☐ | middle 2, app motor 6 | D19 | 19 | IN3 | OUT3 and OUT4 |
| ☐ | middle 2, app motor 6 | D23 | 23 | IN4 | OUT3 and OUT4 |
| ☐ | U4 OUT1 | wire 18 AWG | M5 terminal A | | |
| ☐ | U4 OUT2 | wire 18 AWG | M5 terminal B | | |
| ☐ | U4 OUT3 | wire 18 AWG | M6 terminal A | | |
| ☐ | U4 OUT4 | wire 18 AWG | M6 terminal B | | |

| ☐ | U4 pin | Connects to | Note |
|---|---|---|---|
| ☐ | VS | 12 V rail after the master switch | motor power |
| ☐ | GND | common ground point | same point as U10 GND |
| ☐ | 5V jumper | stays fitted | the onboard regulator makes the L298N logic supply |
| ☐ | 5V terminal | open | never feed this into the ESP32 |
| ☐ | ENA, IN1, IN2, ENB, IN3, IN4 | U10 GPIOs above | 3.3 V logic works on the TTL inputs |

| ☐ | Module | SDA | SCL | Extra pin | Set extra pin to | Result |
|---|---|---|---|---|---|---|
| ☐ | MPU6050 U14 | GPIO 21 | GPIO 22 | AD0 | GND | 0x68, rover orientation |
| ☐ | MPU6050 U15 | GPIO 21 | GPIO 22 | AD0 | 3V3 | 0x69, tilt bracket, not read yet |
| ☐ | MPU6050 U14, U15 | not used | not used | VCC | 3V3 rail | powered from U10 |
| ☐ | MPU6050 U14, U15 | not used | not used | GND | common ground | shared |

| ☐ | Servo wire | Connects to | Note |
|---|---|---|---|
| ☐ | signal, orange or yellow | U10 GPIO 27 | 50 Hz PWM, no angle feedback |
| ☐ | V+, red | 6 V buck output positive | never the ESP32 5 V pin |
| ☐ | GND, brown or black | common ground point | same point as the 6 V buck negative |
| ☐ | C9 | across V+ and GND at the servo | 1000 µF, striped leg to GND |
| ☐ | drawing note | the EasyEDA sheet shows the servo on 5 V | use the 6 V buck, the MG996R stalls near 2.5 A |

## ESP-3, environment board

| ☐ | Net | U16 pin | Goes to | Note |
|---|---|---|---|---|
| ☐ | SDA | GPIO 21 | U17 SDA, U18 SDA, U19 SDA, U20 SDA | one bus, four devices, four addresses |
| ☐ | SCL | GPIO 22 | U17 SCL, U18 SCL, U19 SCL, U20 SCL | |
| ☐ | 3V3 | 3V3 pin | U17 VCC, U18 VCC, U19 VCC, U20 VCC, U26 VCC | sensors and flame module |
| ☐ | GND | GND pin | every module GND | common ground |

| ☐ | Module | Extra pin | Set to | Result |
|---|---|---|---|---|
| ☐ | ADS1115 U17 | ADDR | GND | address 0x48 |
| ☐ | ADS1115 U18 | ADDR | 3V3 | address 0x49 |
| ☐ | MLX90614 U19 | no address pin | fixed | address 0x5A, left |
| ☐ | MLX90614 U20 | no address pin | EEPROM rewritten once | address 0x5B, right |

| ☐ | Gas sensor | Output pin | Divider top | Divider bottom | ADS input |
|---|---|---|---|---|---|
| ☐ | MQ-7 carbon monoxide | AO | 10 kΩ from AO to node | 10 kΩ from node to GND | U17 A0 |
| ☐ | MQ-6 LPG | AO | 10 kΩ from AO to node | 10 kΩ from node to GND | U17 A1 |
| ☐ | MQ-4 methane | AO | 10 kΩ from AO to node | 10 kΩ from node to GND | U17 A2 |
| ☐ | MQ-8 hydrogen | AO | 10 kΩ from AO to node | 10 kΩ from node to GND | U17 A3 |
| ☐ | MQ-135 air quality | AO | 10 kΩ from AO to node | 10 kΩ from node to GND | U18 A2 |

| ☐ | Sensor | Supply | Why |
|---|---|---|---|
| ☐ | MQ-7 | 5 V | heater needs 5 V for the datasheet curve |
| ☐ | MQ-6 | 5 V | heater needs 5 V for the datasheet curve |
| ☐ | MQ-4 | 5 V | heater needs 5 V for the datasheet curve |
| ☐ | MQ-8 | 5 V | heater needs 5 V for the datasheet curve |
| ☐ | MQ-135 | 5 V | heater needs 5 V for the datasheet curve |
| ☐ | flame module U26 | 3.3 V | at 3.3 V the analog output needs no divider |
| ☐ | every MQ ground | common ground | the divider only works with one ground |

| ☐ | Flame pin | U16 pin | State | Trigger |
|---|---|---|---|---|
| ☐ | D1 | GPIO 34 | wired | LOW means fire |
| ☐ | D2 | GPIO 35 | wired | LOW means fire |
| ☐ | D3 | GPIO 36, marked VP | wired | LOW means fire |
| ☐ | D4 | GPIO 39 | planned, not wired | LOW means fire |
| ☐ | D5 | GPIO 32 | wired | LOW means fire |
| ☐ | A1, A2, A4, A5 | open | not used | the firmware reads one level, not five |
| ☐ | A3 | U18 A3 | planned, not wired | no divider, module runs at 3.3 V |
| ☐ | open D4 line | 10 kΩ to 3V3 | fit if the pin floats | stops a fake fire reading |

| ☐ | Flame module pin | Goes to | Note |
|---|---|---|---|
| ☐ | VCC | 3V3 | never 5 V, the analog output must stay ADS safe |
| ☐ | GND | GND | common ground |
| ☐ | D1, D2, D3, D5 | GPIO 34, 35, 36, 32 | active LOW |
| ☐ | D4 | open, or GPIO 39 later | pull up if left open |
| ☐ | A3 | open for now, U18 A3 later | flame_ok stays 0 until this is wired |

## ESP-3, water pump (2 A branch)

| ☐ | From | Part | Value and type | To | Note |
|---|---|---|---|---|---|
| ☐ | 12 V rail after the master switch | fuse | 2 A blade | pump positive, red | pump branch only |
| ☐ | pump negative, black | wire | 18 AWG black | IRLZ44N pin 2, drain | the switch is on the low side |
| ☐ | IRLZ44N pin 3, source | wire | 18 AWG black | common ground point | same ground as U16 |
| ☐ | U16 GPIO 25 | resistor | 100 Ω | IRLZ44N pin 1, gate | limits the gate pulse |
| ☐ | IRLZ44N pin 1, gate | resistor | 10 kΩ | common ground point | pump stays off at boot |
| ☐ | 1N5408 anode, no stripe | wire | 18 AWG | pump negative, black | catches the inductive kick |
| ☐ | 1N5408 cathode, stripe | wire | 18 AWG | pump positive, red | to the fused 12 V side |
| ☐ | 100 µF positive leg | capacitor | 100 µF, 25 V | pump positive, red | at the pump |
| ☐ | 100 µF negative leg | capacitor | 100 µF, 25 V | common ground point | at the pump |
| ☐ | pump jack centre pin | wire | 18 AWG red | fused 12 V | centre positive |
| ☐ | pump jack sleeve | wire | 18 AWG black | MOSFET drain | switched negative |

| ☐ | Pump fact | Value | Check before use |
|---|---|---|---|
| ☐ | model | RBD-1872, AD20P-1230C | read the label |
| ☐ | rated current | 0.35 A, 3.6 W | if the label says 4 A, stop and resize the fuse |
| ☐ | flow | 240 L/h | about 0.7 L in a 10 s burst |
| ☐ | head | 3 m | keep the outlet above the water level |
| ☐ | jack | 5.5 mm outside, 2.1 mm inside | centre positive |
| ☐ | dry run | a few seconds only | prime it in a bucket first |
| ☐ | GPIO 25 state at boot | LOW | pump off until a command arrives |
| ☐ | firmware deadline | 10 s default, 60 s maximum | the pump stops even if the broker dies |

## Handover and bench order

| ☐ | Step | Action | Expected result |
|---|---|---|---|
| ☐ | 1 | Wire motor 1 and the pump branch only | nothing else powered yet |
| ☐ | 2 | Flash U3 and U16, open both serial logs | U3 lists INA at 0x41, 0x44, 0x45 and 0x40, U16 prints pump pin low |
| ☐ | 3 | Power U3 on USB, 12 V off | four INA226 answer on the main bus, four ADXL345 answer on mux channels 0 to 3 |
| ☐ | 4 | Apply 12 V, motors idle | U1 5 V pin reads about 5 V, or 0 V means feed it 5 V, C7 stays cool |
| ☐ | 5 | Command motor 1 at 25, 50, 75 percent | speed eases in over 12 s, wheel follows, current sign follows direction |
| ☐ | 5a | Press STOP while the ramp is still climbing | PWM drops to zero at once, no 12 s coast |
| ☐ | 6 | Pump in a bucket, press THROW WATER | 10 s run, about 0.7 L, MOSFET cool |
| ☐ | 7 | Press STOP WATER during a burst | pump stops at once |
| ☐ | 8 | Kill the broker during a burst | pump still stops at the 10 s deadline |
| ☐ | 9 | Wire motors 2 to 4, repeat one at a time | no shared-ground noise, no reboots |
| ☐ | 10 | Wire the middle pair and test both together | only on the 12 V 5 A adapter with no corner motor |
| ☐ | 11 | Move to the LiPo, all six motors | 20 A fuse fitted, 3S buzzer on the balance lead |
| ☐ | 12 | Floor test | watch pack voltage, stop at 10.2 V |

## Before power is applied

| ☐ | Check | Expected answer |
|---|---|---|
| ☐ | pack positive to fuse | same voltage as the pack |
| ☐ | fuse to switch | open switch reads no voltage |
| ☐ | switch to U1 and U4 | about 12 V |
| ☐ | U1 5 V pin with 12 V applied | about 5 V means the board makes logic power, 0 V means feed 5 V here |
| ☐ | every ground joined | near 0 Ω to pack negative |
| ☐ | motor lead through INA226 in series | never VIN+ to VIN− across the motor |
| ☐ | INA226 VBS on the 12 V rail | not on the PWM motor side |
| ☐ | INA226 shunt marking | R100 on all four modules |
| ☐ | INA226 A0 and A1 straps | 0x41, 0x44, 0x45, 0x40 for motors 1 to 4 |
| ☐ | mux channel map | 0 to 3 ADXL only, no INA on the mux |
| ☐ | C7 and C8 polarity | striped leg to ground |
| ☐ | C9 polarity | striped leg to ground, on the 6 V rail |
| ☐ | motor capacitors | six total, no polarity, across the terminals |
| ☐ | gas dividers | two 10 kΩ per channel, five channels |
| ☐ | highest ADS input | 2.5 V, never 5 V |
| ☐ | highest ESP pin | 3.3 V, except the 5 V input pin |
| ☐ | pump gate pull-down | 10 kΩ from gate to ground |
| ☐ | pump diode direction | stripe to +12 V, no stripe to the drain |
| ☐ | pump fuse | 2 A, in the red wire only |
| ☐ | pump jack polarity | centre positive |
| ☐ | pump in water | never run dry for more than a few seconds |

## When something does not work

| ☐ | What you see | Likely cause | First check |
|---|---|---|---|
| ☐ | board never appears online | wrong broker address | serial log prints the address |
| ☐ | all INA226 report not found | main SDA/SCL swapped, or no 3.3 V on the modules | serial log shows each address |
| ☐ | one INA226 missing | wrong A0/A1 straps, or power on that module | meter the straps, then swap the module |
| ☐ | current always 0 A | calibration rejected, or VBS open | serial log, then VBS to 12 V |
| ☐ | current stuck at 0.8 A | stock 0.1 Ω shunt at full scale | expected on the bench, voltage still valid |
| ☐ | voltage reads 0 V | VBS not connected to 12 V | meter VBS to ground |
| ☐ | motor silent, current 0 A | no driver power or no common ground | meter at the power input, then ground |
| ☐ | motor buzzes, no rotation | duty too low or loose input | try speed 200, meter the input |
| ☐ | full speed at every duty | board is not two-input PWM mode | stop, check the A B C D pads |
| ☐ | one wheel fights the others | leads reversed on that wheel | swap the two motor leads |
| ☐ | gas reads all show `--` | ADS damaged by 5 V, or no baseline | meter the divider output |
| ☐ | sensors drop out when motors run | brush noise | add the 0.1 µF motor capacitors |
| ☐ | ESP reboots when a motor starts | supply sag | 470 µF at the driver input |
| ☐ | pump silent | fuse open, gate low, or no ground | meter 12 V at the pump, then gate volts |
| ☐ | pump runs but no water | air lock or wrong hose direction | prime it, check the arrow on the head |
| ☐ | pump will not stop | MOSFET shorted | pull the 2 A fuse, replace the MOSFET |
| ☐ | MOSFET hot | gate not fully on, or pump current too high | meter gate at 3.3 V, read the pump label |
| ☐ | pump restarts the ESP32 | shared ground noise | 100 µF at the pump, star ground |
| ☐ | flame_ok stays 0 | A3 not wired | expected until A3 goes to U18 A3 |

## Not connected on purpose

| ☐ | Item | Why it is missing | If needed later |
|---|---|---|---|
| ☐ | battery voltage wire to GPIO 32 | buzzer protects the pack already | 100 kΩ and 22 kΩ divider, 2.27 V at full charge |
| ☐ | 3S LiPo and 20 A fuse in the rover | bench uses a 12 V 5 A adapter | fit before the first full pack run |
| ☐ | LiPo low voltage buzzer | not bought yet | balance lead, 3.5 V per cell |
| ☐ | galvanic O2 cell | not purchasable here | publish o2_ok 1 and o2_mv |
| ☐ | AS5600 angle encoder | not fitted, the MG996R reports commanded angle only | mount a magnet on the shaft, firmware support was removed |
| ☐ | tilt measurement from the second MPU | U15 is wired at 0x69 but the firmware does not read it yet | add the tilt code and publish `rover/lidar/tilt_deg` |
| ☐ | extra ADXL345 on the middle motors | the MID board has no vibration sensors today | use address 0x1D for the second one |
| ☐ | flame D4 and A3 | planned, left open for the first bench | wire D4 to GPIO 39 and A3 to U18 A3 |
| ☐ | INA226 on the mux | the INAs keep four distinct addresses on the main bus | unused mux channels 4 to 7 could carry more ADXL345 later |

## The numbers came from these files

| ☐ | File | What it decides |
|---|---|---|
| ☐ | firmware/ESP32-WROOM-32/config.h | motor pins, INA226 addresses and shunt, ADXL mux channels, probe pins, torque ramp |
| ☐ | firmware/ESP32-WROOM-32-MID/config.h | middle motor pins, both IMU addresses, servo pin |
| ☐ | firmware/ESP32-WROOM-32-ENV/config.h | ADS addresses, divider ratio, flame pins, pump pin and burst |
| ☐ | src/routes.rs | `/api/pump` request shape |
| ☐ | README.md | parts list and topic payloads |
| ☐ | BOM/CHANGES.md | what changed in the parts list |
