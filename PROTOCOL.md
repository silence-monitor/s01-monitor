# Silence / SEAT MÓ CAN protocol

Classic CAN, 11-bit identifiers, 250 kbit/s. Multi-byte integers are little-endian unless a field is marked BCD. Bit 0 is the least significant bit of a byte.

Two modules own fixed identifier windows:

| Module | Identifiers |
| --- | --- |
| BMS | `0x181`–`0x27F` |
| ECU | `0x280`–`0x37F` |

Frames outside those windows are listed only when a recording showed them. Every other field is defined here. Where a recording disagrees with that definition, this document wins. The disagreements are collected at the end.


## Broadcast frames

### `0x181` BMS summary

| Bytes | Type | Meaning |
| --- | --- | --- |
| 0 | — | Counter. |
| 1 | uint8 | State of charge, percent. |
| 2 | — | Lowest temperature, °C. |
| 3 | — | Highest temperature, °C. |
| 4–5 | uint16 | Pack voltage, unit 0.1 V. Divide by 10. |
| 6–7 | — | Current, unit 0.1 A. Not the current used below; see `0x189`. |

### `0x183` identity

Sent as a remote frame to request the record. The data frame is:

| Bytes | Type | Meaning |
| --- | --- | --- |
| 0–3 | uint32 | Battery pack ID (BPID). `0xFFFFFFFF` means unprogrammed. |
| 4 | uint8 | Manufacture year, two digits. |
| 5 | uint8 | Manufacture month. |
| 6 | uint8 | Manufacture day. |

The same record is parameter index `0x21`. See [Parameter access](#parameter-access).

### `0x184` BMS status

Eight flag bytes. Names are the on-wire flags.

| Byte | Bit | Flag |
| --- | --- | --- |
| 0 | 0 | ON |
| 0 | 1 | DSCH |
| 0 | 2 | CHGi |
| 0 | 3 | CHGe |
| 0 | 4 | CHGD |
| 0 | 5 | WarnMode |
| 0 | 6 | LEDr |
| 0 | 7 | Alarm |
| 1 | 0 | CC |
| 1 | 1 | CV |
| 1 | 2 | Bal |
| 1 | 3 | VKL |
| 1 | 4 | OFD |
| 1 | 5 | INH |
| 1 | 6 | UVPD |
| 1 | 7 | ILDSCH |
| 2 | 0 | WOV |
| 2 | 1 | WUV |
| 2 | 2 | WOCD |
| 2 | 3 | WOCR |
| 2 | 4 | WOCC |
| 2 | 5 | WOT |
| 2 | 6 | WUT |
| 2 | 7 | WLS |
| 3 | 0 | LOV |
| 3 | 1 | LUV |
| 3 | 2 | LOCD |
| 3 | 3 | LOCR |
| 3 | 4 | LOCC |
| 3 | 5 | LOT |
| 3 | 6 | LUT |
| 3 | 7 | SCD |
| 4 | 0 | Key_In |
| 4 | 1 | Chg_ext |
| 4 | 2 | Chg_ob |
| 4 | 3 | PiezoTap |
| 4 | 4 | Accel |
| 4 | 5 | RTCC |
| 5 | 0 | K_plus |
| 5 | 1 | PMOS_E |
| 5 | 2 | PMOS_I |
| 5 | 3 | Kch_ob |
| 5 | 4 | Heater |
| 5 | 5 | Fan_A |
| 5 | 6 | Fan_B |
| 6 | 0 | Unfiltered data |
| 6 | 1 | GOD Mode |
| 6 | 2 | Fast Acq |
| 6 | 3 | RAW data |
| 6 | 4 | Debug |
| 6 | 5 | Developer |
| 7 | 2 | Kmain error |
| 7 | 3 | HiGs Impact |
| 7 | 4 | Thermal Event |

### `0x185`–`0x188` cell voltages

Each value is a uint16 in millivolts. Cells are packed in order, two bytes each, from byte 0.

| ID | Cells | Bytes |
| --- | --- | --- |
| `0x185` | 1–4 | 0–1, 2–3, 4–5, 6–7 |
| `0x186` | 5–8 | 0–1, 2–3, 4–5, 6–7 |
| `0x187` | 9–12 | 0–1, 2–3, 4–5, 6–7 |
| `0x188` | 13, 14 | 0–1, 2–3 |

`0x188` is 6 bytes in some recordings. Bytes 4–5 were `00 00`.

### `0x189` current and probe temperatures

| Bytes | Type | Meaning |
| --- | --- | --- |
| 0–1 | int16 | Pack current, unit 10 mA. Divide by 100 for amperes. Negative is charge or regeneration. The own capture shows the opposite: negative is discharge (see Own capture below). |
| 2–3 | int16 | Temperature A, unit 0.01 °C. |
| 4–5 | int16 | Temperature B, unit 0.01 °C. |
| 6–7 | int16 | Temperature C, unit 0.01 °C. |

### `0x18E` balancing

| Byte | Bit | Flag |
| --- | --- | --- |
| 0 | 0–7 | Cells 1–8 balancing |
| 1 | 0–5 | Cells 9–14 balancing |
| 2 | 0 | Enabled |
| 2 | 1 | Allowed |
| 2 | 2 | Inhibited |
| 2 | 3 | AUTO |
| 2 | 4 | Imbalance |
| 2 | 5 | Viddle |
| 2 | 6 | High Temp |
| 2 | 7 | Over Temp |

Byte 3 is `Voltage_delta_max`. Byte 6 is `milis_balance`.

### `0x191` extra BMS flags

| Byte | Bit | Flag |
| --- | --- | --- |
| 0 | 2 | Kopen |
| 0 | 3 | HiGs Impact |

### `0x200` clock

BCD digits, printed with `%02X` so a byte of `0x14` is fourteen.

| Byte | Meaning |
| --- | --- |
| 0 | Status. Bit 1 set means the clock is valid. A recording shows `0x03` here, which has that bit set. |
| 1 | Hour |
| 2 | Minute |
| 3 | Second |
| 4 | Day |
| 5 | Month |
| 6 | Year, two digits |

### `0x401` throttle and drive switches

| Byte | Bit | Flag |
| --- | --- | --- |
| 4 | 0 | FS1 |
| 4 | 1 | Forward |
| 4 | 2 | Reverse |
| 4 | 3 | DM1 |
| 4 | 4 | DM2 |
| 4 | 5 | DM3 |

Byte 0 is throttle input voltage, a 16-bit value.

## ECU input and output flags

### `0x28A`

| Byte | Bit | Flag |
| --- | --- | --- |
| 0 | 0 | God Mode |
| 3 | 0 | Sending on debug mode |
| 3 | 1 | BMS timeout |
| 3 | 2 | Sevcon timeout |
| 4 | 0 | PCB undervoltage |
| 4 | 1 | BTSoff |
| 4 | 2 | BTSon |
| 4 | 3 | Normal PCB voltage |
| 4 | 4 | PCB overvoltage |

The same identifier is a command. See [Debug and god mode](#debug-and-god-mode).

### `0x293`

| Byte | Bit | Flag |
| --- | --- | --- |
| 6 | 0 | Mode switch is not pressed |
| 6 | 1 | Ready-to-drive is 0 |
| 6 | 2 | Throttle is not released |
| 6 | 3 | Alarm is not fired |
| 6 | 4 | Controller run time is over the minimum delay |
| 6 | 5 | Not in charging mode |
| 6 | 6 | Not in charged mode |
| 6 | 7 | Not in warnings mode |
| 7 | 0 | PMOS I is active |
| 7 | 1 | No MCU timeout |
| 7 | 2 | Battery sensor detects a battery |
| 7 | 3 | Left brake is pressed |

### `0x294`

| Byte | Bit | Flag |
| --- | --- | --- |
| 0 | 0 | Brake L |
| 0 | 1 | Normal beam |
| 0 | 2 | Warnings |
| 0 | 3 | Brake R |
| 0 | 4 | Blink L |
| 0 | 5 | High beam |
| 0 | 6 | Blink R |
| 0 | 7 | Reverse |
| 1 | 0 | Side stand |
| 1 | 1 | Mode |
| 1 | 2 | Horn |
| 1 | 3 | Seat actuator |
| 1 | 4 | Battery block |
| 1 | 5 | Main stand |
| 1 | 6 | Helmet 1 |
| 1 | 7 | Helmet 2 |
| 2 | 0 | Blink L (output) |
| 2 | 1 | Blink R (output) |
| 2 | 2 | Highbeam (output) |
| 2 | 3 | Position (output) |
| 2 | 4 | Normalbeam (output) |
| 2 | 5 | Brake L (output) |
| 2 | 6 | Brake R (output) |
| 2 | 7 | Horn (output) |
| 3 | 0 | Seat actuator (output) |
| 3 | 1 | Fans (output) |
| 3 | 2 | Battery block (output) |
| 3 | 3 | **White R** (output; an earlier listing has this the other way round) |
| 3 | 4 | **White L** (output; an earlier listing has this the other way round) |
| 3 | 5 | Buzzer (output) |
| 3 | 6 | Dash (output) |
| 3 | 7 | Steering Lock (output) |

## Parameter access

A parameter frame is 8 bytes. Byte 0 is the parameter index (`0x00`–`0xFE`). Bytes 1–7 hold the value. The value is placed at `iniByte` and runs through `endByte` from the parameter catalogue. Integers are little-endian. `FLOAT` values are little-endian IEEE-754. `scaling` in the catalogue is the power of ten applied when the integer is shown in engineering units. `HEX` and `BIN` catalogue values are numeric bases, not a different on-wire encoding. ASCII values can span several frames; each frame carries the next slice and the index increases by one per frame.

### Battery

| ID | Direction | Payload |
| --- | --- | --- |
| `0x234` | request | Byte 0 = index. |
| `0x235` | reply | Byte 0 = index. Bytes 1–7 = value. |
| `0x233` | write | Byte 0 = index. Bytes 1–7 = value. |

A full read walks every index that has a parameter, skipping empty ones. Each reply on `0x235` is followed by a request for the next index.

A full write sends index `0x20` first when that index has a value, then the remaining indexes except `0x21`. After the last data index, a commit frame is `0x233` with byte 0 = `0xFF`. The reply on `0x235` with byte 0 = `0xFF` is success when bytes 1–7 are zero. Any other byte is a failed write.

Index `0x21` is not part of that bulk write. It is the BPID record:

| Byte | Meaning |
| --- | --- |
| 0 | `0x21` |
| 1–4 | BPID, uint32 |
| 5 | Year, two digits |
| 6 | Month |
| 7 | Day |

The date written with a BPID is the current day, month, and year modulo 100. The exchange reads `0x21` on `0x234` first. If the stored BPID is anything other than `0xFFFFFFFF`, the write is not sent unless an explicit override is in effect. The write itself is one `0x233` frame.

### ECU

| ID | Direction | Payload |
| --- | --- | --- |
| `0x31B` | request | Byte 0 = index, or the next slice index for a multi-frame ASCII value. |
| `0x31C` | reply | Byte 0 echoes that index. A recording shows the VIN in this frame. |
| `0x31A` | write | Byte 0 = index. Bytes 1–7 = value. |

There is no `0xFF` commit frame on the ECU path.

## Clock set

`0x201`, 7 bytes. Byte 0 is the command `0x04`. The rest is BCD, produced by taking the decimal text of each field and parsing it as hex (`14` becomes `0x14`).

| Byte | Field |
| --- | --- |
| 0 | `0x04` |
| 1 | Hour |
| 2 | Minute |
| 3 | Second |
| 4 | Day |
| 5 | Month |
| 6 | Year, two digits |

The monitor sends this frame only from the Battery clock card on the Parameters screen, after the person confirms; the ESP32's firmware passes it only in this exact form, with every field in its range. Sent once from a script on 9 Nov 2026 (`t201704222803091126`), the battery accepted it: its clock, on `0x200`, changed to that time and date within a second, and the event log recorded a `0x3007` time change with the same time and date (see [CAN_NOTES.md](CAN_NOTES.md)).

## Event log

Request `0x1ED`. The first request has an empty payload. Records come back on `0x1EE`, three frames per record.

| Frame | Contents |
| --- | --- |
| 1 | Bytes 0–1 are the record index, `(byte0 >> 4) + (byte1 << 4)`. Bytes 2–7 are the date and time, each byte a plain decimal number, in the order year, month, day, hour, minute, second, printed as `YY/MM/DD HH:MM:SS` (an earlier version of this line said `DD/MM/YY`). |
| 2 | Bytes 2–3 are the event code. Bytes 4–7 are further hex. |
| 3 | Bytes 2–5 are further hex. |

A `0x1ED` request with four bytes is two 16-bit numbers, low byte first: the index to start at, and how many records to send, counting down from that index. The first request has an empty payload and brings the newest ten. Asking for every older record in one request brings them all at once, a burst of about 900 frames a second that made the Sevcon controller fault (see [CAN_NOTES.md](CAN_NOTES.md)). The monitor asks for ten records at a time instead, with a pause of one second between requests. The event code is looked up in this table:

| Code | Meaning |
| --- | --- |
| `0x1010` | Off |
| `0x1021` | Periodic wake, AFE refresh reading |
| `0x1022` | Key on |
| `0x1023` | On from the internal charger |
| `0x1024` | On from the external charger |
| `0x10A0` | System start, reason for reset |
| `0x1100` | Outputs forced disable |
| `0x1110` | Keyless enabled |
| `0x1120` | Keyless disabled |
| `0x1121` | Keyless disabled, timeout |
| `0x1200` | Shutdown |
| `0x1A00` | Status message sent |
| `0x1A01` | Message sent |
| `0x1C00` | Interlock change |
| `0x2201` | Overvoltage |
| `0x2202` | Undervoltage |
| `0x2203` | Overcurrent, discharge |
| `0x2204` | Overcurrent, regenerative |
| `0x2205` | Overcurrent, charge |
| `0x2206` | Overtemperature |
| `0x2207` | Undertemperature |
| `0x2208` | Short-circuit detection |
| `0x2301` | Shutdown, undervoltage (12 V rail, battery, and related rails) |
| `0x2302` | Shutdown, 5 V rail |
| `0x3004` | Time change over CAN, 4 bytes |
| `0x3007` | Time change over CAN, 7 bytes |
| `0x3010` | Time change from GPS |
| `0x3011` | GPS offset |
| `0x3101` | MCU bootloader update |
| `0x3102` | MCU bootloader updated |
| `0x3F00` | Downlink |
| `0x4020` | Parameter flags |
| `0x4021` | Parameter BPID |
| `0x5A01` | Stack overflow |

## Battery reset

`0x250`, data bytes `FA 05`. No reply is defined. This does not write the parameter catalogue and it does not clear the event log. `0x10A0` in the event log is the BMS recording a restart and the reason for it.

## Debug and god mode

Both are an 8-byte frame on `0x28A`.

| Command | Byte 0 | Byte 3 | Other bytes |
| --- | --- | --- | --- |
| Debug on | `0x00` | `0x01` | `0x00` |
| Debug off | `0x00` | `0x00` | `0x00` |
| God mode on | `0x01` | `0x01` | `0x00` |
| God mode off | `0x00` | `0x01` | `0x00` |

Debug on is sent when the bus wakes (the first frame after more than one second of silence) and debug off when disconnecting. It is not sent again in between. The output control buttons use the same frame: bits 0 to 7 of byte 1 (Blink L, Blink R, Highbeam, Position, Normalbeam, Brake L, Brake R, Horn) and of byte 2 (Seat actuator, Fans, Battery block, White L, White R, Buzzer, Dash, Steering Lock), with bytes 0 and 3 set on each, and their state is read back on `0x294` bytes 2 and 3. From version 0.8.0 the monitor sends debug mode, and nothing else on `0x28A`: the two frames above, never the god-mode bit or an output bit.

## Further identifiers

Recording A, below, corrects several of these rows. Where the two disagree, use [Recording A](#recording-a). Byte numbers in some listings are 1-based.

These identifiers were recorded on a Silence S01 / SEAT MÓ bus. They are not part of the layouts above. Scaling is as written in the capture. Blank cells in the source sheet are omitted.

| ID | Source in the capture | Layout |
| --- | --- | --- |
| `0x182` | ECU | Byte 1 state. Byte 4 warnings, MSB/LSB as labeled. DLC 7. |
| `0x190` | BMS | Byte 1 SOC in 1/1000. Bytes 2–3 remaining capacity, mAh. Byte 4 VSOC. Byte 5 SOH. DLC 5. |
| `0x197` | BMS | DLC 8, bytes not named. |
| `0x19A` | BMS | Present, layout unknown. |
| `0x1AA` | BMS | Bytes 0–1 maximum discharge current, A. Bytes 2–3 maximum recharge current, A. DLC 4. |
| `0x1AC` | BMS | Bytes 4–7 time since the last BMS reset. DLC 4. The capture marks this as uncertain. |
| `0x1B0` | BMS | DLC 8, bytes not named. |
| `0x1B1` | BMS | Present, layout unknown. |
| `0x1C0` | BMS | Bytes 1–4 energy charged since on, watt-seconds. Bytes 5–7 energy discharged since on, watt-seconds. DLC 8. |
| `0x1C1` | BMS | Bytes 1–4 energy regenerated since on, watt-seconds. DLC 8. |
| `0x1DB` | BMS | Present, layout unknown. |
| `0x20A`, `0x20B` | BMS | DLC 8, bytes unknown. |
| `0x220`, `0x230`, `0x231` | BMS | Present. `0x230` is DLC 7. |
| `0x280` | ECU | Byte 0 drive mode. Byte 1 lamps: bit 0 left indicator, bit 1 right indicator, bit 2 high beam, bit 3 low beam, bit 7 hazards (a recording of 13 Nov 2026). DLC 4. |
| `0x297` | BMS | Bytes 4–7 time since ignition on. DLC 8. The capture marks this as uncertain. |
| `0x298`, `0x299` | BMS | Present, layout unknown. |
| `0x300` | ECU | Bytes 0–1 speed, km/h. Byte 2 remaining range, km. Byte 4 power bar. DLC 3. |
| `0x301` | ECU | Bytes 1–2 motor temperature, 0.1 °C. Bytes 3–4 inverter temperature, 0.1 °C. Bytes 5–6 a third temperature, 0.1 °C. DLC 6. |
| `0x310` | BMS | ASCII bytes `C H A R`. DLC 8. |
| `0x313` | BMS | ASCII bytes `S T A N`. DLC 8. |
| `0x371` | Sevcon / Votol | Bytes 0–1 motor Ud, 1/16 V. Bytes 2–3 motor Iq reference, 1/16 V in the capture. Byte 4 SVM index, 100/255 percent. Bytes 6–7 measured inductance, 0.0596 µH. DLC 8. |
| `0x381` | Sevcon | Present, layout unknown. |
| `0x391` | Sevcon / Votol | Bytes 1–2 speed, rpm. Bytes 3–4 Id reference, 1/16 A. Bytes 5–6 maximum battery discharge current, A. DLC 8. |
| `0x393` | Sevcon | Four int16 fields in 1/16 A: Id reference, Iq reference, Id actual, Iq actual. DLC 8. |
| `0x411` | Sevcon / Votol | Bytes 0–1 left-motor status word. Bytes 2–3 highest active fault. Byte 4 heatsink temperature, °C. Byte 6 motor temperature, °C. DLC 7. |
| `0x421` | ECU | Byte 0 footbrake switch. DLC 1. |
| `0x422` | Sevcon | Four int16 fields: battery current 1/16 A, capacitor voltage 1/16 V (the capture says 1/16 A), torque demand 1/16 N·m, torque actual 1/16 N·m. DLC 8. |
| `0x431` | Sevcon | Present, layout unknown. |
| `0x500` | Display | Byte 0 outside temperature, °C. Bytes 4–6 odometer, km. DLC 8. |
| `0x501` | — | DLC 0. |
| `0x502` | — | Two unknown bytes. |
| `0x601` | — | Present, layout unknown. |
| `0x701` | Sevcon | Byte 0 operating or pre-operational mode. DLC 1. |

## Recording B (debug mode on)

A scooter on its stand, recorded with this app with debug mode on, 35 s. The ECU sends a 50 Hz block of frames that Recording A did not have. The lamp-current reading below is a guess (see `CAN_NOTES.md`).

| ID | Layout |
| --- | --- |
| `0x28A` | DLC 8, constant `00 00 00 09 0C 00 00 00`. Byte 3 bit 0 debug mode on; byte 3 bit 3 also set and not in the catalogue; byte 4 BTSon and Normal Voltage. |
| `0x290` | DLC 8. Words at bytes 0–1, 2–3, 4–5 about 660, 1759 and 2817, nearly constant. Bytes 6–7 zero. |
| `0x291` | DLC 8. Four 16-bit words. Word 0 about 710 with the right blinker output on, word 1 about 719 with the left on, word 3 about 574 with the high beam on. Word 2 always 0. Near 0 otherwise. A guess: lamp current in counts. |
| `0x292` | DLC 8. Four 16-bit words. Word 0 about 581 with the low beam on. Words 1 and 2 about 722 and 707 for the two white lamps (`0x294` byte 3 bits 4 and 3). Word 3 about 412 with Position on. A guess: lamp current in counts. |
| `0x293` | DLC 8. Bytes 6–7 constant `FC 07`. Bytes 0–5 three 16-bit values: about 2050, about 3380 (it falls under load), 0. |
| `0x29F` | DLC 5, all zero. |
| `0x502` | DLC 2, about 1 Hz. Byte 0 a counter that stepped `03`, `13`, `23` after 15 s, the same value as `0x500` byte 1. |

## Recording A

A CANable on a Silence S01 standing on its stand, 332 s. The key was off for the first 160 s and on after that. There were throttle blips, then reverse was pressed (the Reverse bit on `0x401` was set once, confirmed), then the side stand was moved (the `0x310` message appeared once, confirmed as to timing). There was no riding and no charging. The first file is 26.9 s and the second 187.8 s, starting 144.6 s after the first, which is how the 332 s is made up. Times below run from the first frame of the first file. These layouts replace the rows above for the same identifiers.

| ID | Layout |
| --- | --- |
| `0x182` | DLC 1. Byte 0 was `00` throughout. |
| `0x190` | Not sent. |
| `0x19B` | DLC 5. Five bytes between `0A` and `10`. Meaning not settled. |
| `0x197` | DLC 8. Bytes 6 and 7 were `64 64`. Meaning not settled. |
| `0x1AC` | Bytes 0–3 uint32 milliseconds since the last battery restart (a guess as to the purpose). Bytes 4–7 zero. |
| `0x200` | The clock was marked valid and read 3 January 2001. It has not been set. |
| `0x280` | DLC 4. Byte 0: bit 0 ready to drive, bit 2 set while reverse is held, bit 3 side stand down, bits 4–5 drive mode (0 off, 1 ECO, 2 SPORT, 3 CITY). `00` key off, `21` key on (SPORT on one published mapping; which mode the scooter was in is not recorded), `24` reverse, `28` side stand down. The mode values are from a published mapping and have not been confirmed on this scooter. Byte 1 was `08` with the key on. For about 12 s after key on (161.9 s to 174.1 s) the mode and ready bits flicker, and the monitor shows them as sent. |
| `0x297` | Bytes 0–3 uint32 milliseconds. It runs with the key off and does not reset at key on. Bytes 4–7 zero. |
| `0x300` | DLC 3. Byte 0 speed, km/h. Byte 1 remaining range, km for the current drive mode (`68` key off, `52` key on; not checked against the dash). Byte 2 small signed number that follows the pack current, about 1 per kW. No byte 4. |
| `0x301` | DLC 6. int16 at bytes 0–1 motor, 2–3 inverter, 4–5 a third temperature, 0.1 °C. |
| `0x310` | DLC 8. Text for the dash, up to nine characters. The low seven bits of the eight bytes are the first eight characters; the high bit of byte n is bit n of the ninth. `53 49 C4 45 53 54 C1 4E` once, which spells SIDESTAN with the high bit cleared and SIDESTAND with the ninth character, then all zero. Seen while the side stand was down. A later recording has `4B F0 EC 75 73 20 CF 46`, Kplus OFF. |
| `0x391` | DLC 8. Bytes 0–3 int32 motor speed, rpm, negative in reverse. Bytes 4–5 uint16 follow the size of the motor current on `0x381`. Bytes 6–7 uint16 move smoothly between `00E8` and `0170`, like a limit moving to a target. Last two not settled. |
| `0x401` | DLC 5. Bytes 0–1 uint16 throttle input, 0 to 4676, unit not known. Byte 4 flags as in the table above. Bit 3 was set from 161.9 s to 174.1 s as the key came on, and clear after. |
| `0x411` | DLC 8. Byte 4 was 8–9 °C and byte 6 was 14–17 °C. Byte 6 follows the inverter on `0x301`. |
| `0x371` | DLC 8. Bytes 6–7 uint16 are the controller bus voltage in 1/16 V (`0393` is 57.2 V at rest). It differs from the pack voltage on `0x181` by 0.15 V on average and up to 3.6 V in the throttle blips, so it is a separate measurement. Checked in one capture only. The monitor shows it as the controller bus voltage. `0x431` bytes 2–3 carry the same value. |
| `0x181` | DLC 8. Bytes 6–7 int16 pack current, unit 0.1 A, same sign as `0x189` (−90.1 A when `0x189` read −90.18 A). |
| `0x189` | Bytes 0–1 current: negative is discharge and positive is charge or regeneration, from adding up the energy and comparing it with the counters on `0x1C0` and `0x1C1`. |
| `0x197` | DLC 8. Bytes 0–1 uint16 capacity left, unit 0.01 Ah (9964 to 9955), tracking the discharge energy. Bytes 4–5 a second uint16 that falls the same way. Bytes 2–3 small signed. Bytes 6–7 `64 64`. |
| `0x381` | DLC 8. Four int16. Bytes 0–1 and 4–5 follow motor speed (negative). Bytes 2–3 and 6–7 follow throttle. Probably Id and Iq, reference and actual. |
| `0x431` | DLC 8. Four int16 in 1/16 units: battery current (positive is a draw), bus voltage, torque asked for, torque delivered. Another listing calls this record `0x422`. |
| `0x502` | DLC 2. Byte 0 a 2-bit rolling counter on `58` (`58` to `5B`), byte 1 `01`. Sent only on change. `0x500` byte 1 carries the same value. |
| `0x313`, `0x422`, `0x701` | Not seen. |
| `0x28A`, `0x290`–`0x294`, `0x29F` | Not seen in Recording A; seen only in Recording B, see [Recording B](#recording-b-debug-mode-on). |

The drive modes are ECO, CITY, and SPORT, and reverse is a separate button. The drive mode and the side stand come from `0x280` byte 0, as laid out in the table above.

## Capture logs

The monitor writes and reads this line format.

```
# comment
time:17:45:30.991 RX: STDID:00197 data:EA 26 00 00 E8 1F 64 64
# MARK 17:46:34.400 pressed reverse
```

- `time:` is the time of day with milliseconds. A log written by the monitor uses UTC. A time that jumps back by most of a day is read as midnight.
- `STDID` is the 11-bit identifier in five hex digits. `data:` is zero to eight bytes in hex, or `RTR` for a remote request, which a replay skips.
- A `# MARK` line is a note from the Mark button. The scenario logs (`19:50:17.087 << t4015000000000A\r`) are also read, taking the received lines only.
- A replay starts its clock at the first line of the file.
- On the 25 Oct capture the frames with no reading in the monitor are `0x182`, `0x19B`, `0x1B0`, `0x1B1`, `0x381` and `0x502`.

The monitor also writes CSV, which is not a capture and cannot be replayed. Each file starts with a UTF-8 byte order mark, uses commas and CRLF, and ends with an `Origin` column.

| File | Columns |
| --- | --- |
| Readings | Group, Reading, Value, Unit, Frame, Origin |
| History | Time (UTC), Elapsed (s), the 11 charted values with units, Marker, Origin |
| Values log | Time (UTC), Elapsed (s), every reading with its unit, Marker, Origin |
| Frames | ID, Frames, Rate (Hz), DLC, Latest bytes, Bytes that changed, Decoded, Meaning, Origin |
| Markers | Number, Time (UTC), After first frame (s), Note, Origin |
| Parameters | Device, Section, Parameter, Value, Unit, Read at (UTC), Origin |
| Event log | Time, Code, Data, Text, Origin |
| Flags | Group, Flag, State, Frame, Byte, Bit, Frame bytes, Frame last seen (UTC), Meaning, Origin |

In the Flags file State is `Set`, `Clear` or `No data`. `No data` means the frame that carries the flag has not arrived, or arrived too short to hold the byte. The Flags screen draws such a flag as clear. A parameter that was not read is an empty cell, and the Parameters file is the one the monitor reads back to compare two sets of parameters, matching rows by Device, Section and Parameter. A reading that has not been seen is an empty cell. Time is `YYYY-MM-DD HH:MM:SS.mmm`, UTC. In a replay it counts from the moment the replay started, so use Elapsed.

## Notes

Pack current and the three probe temperatures are on `0x189`. A recording also shows a current, in 0.1 A, and a low and high temperature, in whole degrees, on `0x181`. Those `0x181` fields are not part of the current and temperature definition above.

`0x182` is labeled an ECU state frame in one listing. It has no field layout in this document.

Cell voltages are little-endian pairs that start on an even byte. Some listings print each cell name on the second byte of the pair.
