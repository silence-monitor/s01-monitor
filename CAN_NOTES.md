# CAN notes

What is known about each frame on the scooter's bus, and how well.
[PROTOCOL.md](PROTOCOL.md) is the byte layout, and it wins where the two differ.

Each entry has these parts:

- **Record.** What is known about the frame's bytes.
- **Wording.** The one plain sentence the app may show for it. Nothing else in
  this file is shown to a rider.
- **Basis.** How well it is known, in words, without saying where it came from:
  *documented elsewhere* (written down by someone else), *seen on a scooter* (an
  observation of what the bus did), or a *guess* about the purpose, which always
  says it is a guess and gives a confidence: *context* (several facts point the
  same way), *letter* (the abbreviation is the main reason) or *none*. A guess
  is never copied into the Wording.

A guess does not replace an observation or a layout, and an observation wins
over a byte position written down elsewhere. When a search found no definition
for something, that is stated, so it is not treated as a new gap.

## Broadcast frames

### `0x181` Pack summary

**Record.** Byte 1 is state of charge, percent. Bytes 4–5 are pack voltage, uint16, unit 0.1 V.

**Wording.** State of charge is a percent of the pack. Pack voltage is in volts.

**Basis.** Documented elsewhere.

**Record.** Byte 0 is a counter. Byte 2 is the lowest temperature in whole degrees. Byte 3 is the highest temperature in whole degrees. Bytes 6–7 are a current in units of 0.1 A.

**Wording.** None for the counter and the temperatures. Pack current and the three probe temperatures are taken from `0x189`. Bytes 6–7 are also shown as a second current reading, called Current (pack summary).

**Basis.** Documented elsewhere; seen on a scooter.

### `0x183` Pack identity

**Record.** Remote frame asks for the record. Bytes 0–3 are the pack ID. `0xFFFFFFFF` means unprogrammed. Bytes 4–6 are manufacture year, month, and day. The same record is parameter index `0x21`.

**Wording.** This is the pack identity and the date it was programmed. The monitor shows the ID as a decimal number, with only the last three digits in the header and the full number on hover.

**Basis.** Documented elsewhere; seen on a scooter.

### `0x184` Battery status

Eight flag bytes. The names and the byte and bit positions come from a published listing. Unused bits are omitted.

**Basis.** Documented elsewhere.

Where a flag has only that name, the wording is the name and nothing more. The longer notes are the ones where another source added a sentence, an observation, or a guess.

#### Name only

| Flag | Byte | Bit | Wording |
| --- | --- | --- | --- |
| ON | 0 | 0 | The battery reports ON. |
| DSCH | 0 | 1 | The battery reports DSCH. |
| CHGi | 0 | 2 | The battery reports CHGi. |
| CHGe | 0 | 3 | The battery reports CHGe. |
| CHGD | 0 | 4 | The battery reports CHGD. |
| WarnMode | 0 | 5 | The battery reports WarnMode. |
| LEDr | 0 | 6 | The battery reports LEDr. |
| Alarm | 0 | 7 | The battery reports Alarm. |
| CC | 1 | 0 | The battery reports CC. |
| CV | 1 | 1 | The battery reports CV. |
| Bal | 1 | 2 | The battery reports Bal. |
| VKL | 1 | 3 | The battery reports VKL. |
| OFD | 1 | 4 | The battery reports OFD. |
| UVPD | 1 | 6 | The battery reports UVPD. |
| ILDSCH | 1 | 7 | The battery reports ILDSCH. |
| Key_In | 4 | 0 | The battery reports Key_In. |
| Chg_ext | 4 | 1 | The battery reports Chg_ext. |
| Chg_ob | 4 | 2 | The battery reports Chg_ob. |
| PiezoTap | 4 | 3 | The battery reports PiezoTap. |
| Accel | 4 | 4 | The battery reports Accel. |
| RTCC | 4 | 5 | The battery reports RTCC. |
| Kch_ob | 5 | 3 | The battery reports Kch_ob. |
| Unfiltered data | 6 | 0 | The battery reports unfiltered data. |
| GOD Mode | 6 | 1 | The battery reports GOD Mode. This is a battery status, separate from the ECU god-mode command on `0x28A`. |
| Fast Acq | 6 | 2 | The battery reports Fast Acq. |
| RAW data | 6 | 3 | The battery reports RAW data. |
| Debug | 6 | 4 | The battery reports Debug. This is a battery status, separate from the ECU debug command on `0x28A`. |
| Developer | 6 | 5 | The battery reports Developer. |

**Basis.** Documented elsewhere.

**Guess.** `ON` means the pack is switched on. It is set whenever the scooter is running. Confidence: context.

**Guess.** `DSCH` means the pack is in a discharge state. This is not “discharge allowed.” Confidence: context.

**Guess.** `CHGi` means charging from the internal charger. Confidence: letter.

**Guess.** `CHGe` means charging from the external charger. Confidence: letter.

**Guess.** `CHGD` means the charge has finished. Confidence: letter.

**Guess.** `WarnMode` means a warning bit in byte 2 is set. Confidence: letter.

**Guess.** `LEDr` means the pack light ring is being driven. Confidence: context.

**Guess.** `Alarm` means an alarm is latched. Confidence: letter.

**Guess.** `CC` means the charge is in its constant-current phase. Confidence: context.

**Guess.** `CV` means the charge is in its constant-voltage phase. Confidence: context.

**Guess.** `Bal` means balancing is underway. The per-cell bits are on `0x18E`. Confidence: context.

**Guess.** `INH` is an error condition that blocks the high-current output and charging. Confidence: context.

**Guess.** `Key_In` means the ignition input is closed. Confidence: context.

**Guess.** `Chg_ext` means an external charger is connected. The event log already separates an external charger from the internal one. Confidence: context.

**Guess.** `Chg_ob` means the onboard charger is connected. Confidence: context.

**Guess.** `PiezoTap` means the tap on the front of the pack that wakes the light ring. Confidence: context.

**Guess.** `Accel` means an accelerometer input, separate from the `HiGs Impact` event. Confidence: letter.

**Guess.** `RTCC` means the real-time clock is running. The clock message is `0x200`. Confidence: letter.

**Guess.** `Kch_ob` means the onboard charger relay. Confidence: letter.

**Guess.** `Unfiltered data` means the reported values are not smoothed. Confidence: letter.

**Guess.** battery `GOD Mode` means a battery engineering override. It stays distinct from the ECU command on `0x28A`. Confidence: letter.

**Guess.** `Fast Acq` means the pack is sampling faster. Confidence: letter.

**Guess.** `RAW data` means the reported values are not smoothed. Confidence: letter.

**Guess.** battery `Debug` means an engineering debug mode. It stays distinct from the ECU debug command on `0x28A`. Confidence: letter.

**Guess.** `Developer` means an engineering developer mode. Confidence: letter.

#### INH

**Record.** `INH` is `0x184` byte 1, bit 5, in the BMScutum section. INH is an error condition caused by faults such as overtemperature or a hard shock (a "g-shock"). While it is set, the high-current output (`K_plus`, byte 5 bit 0) must not be enabled and charging is blocked.

**Wording.** An error condition, caused by faults such as overtemperature or a hard shock. While it is set, the high-current output (K_plus) must not be switched on and charging is blocked.

**Basis.** Documented elsewhere.

#### No written definition

`ILDSCH`, `VKL`, `OFD`, and `UVPD` have no expansion in the listing or the event log. `INH` was in this list until 11 Nov 2026; see its own entry above.

**Wording.** No written definition.

**Basis.** Documented elsewhere.

#### Warnings and limits

The event-log lines below are the written sentences for the matching warning and limit flags. W and L are the warning and the limit. The event log does not contain a line for WLS.

| Flag | Byte | Bit | Wording |
| --- | --- | --- | --- |
| WOV | 2 | 0 | Error Over Voltage. |
| WUV | 2 | 1 | Error Under Voltage. |
| WOCD | 2 | 2 | Error Over Current Discharge. |
| WOCR | 2 | 3 | Error Over Current Regenerative. |
| WOCC | 2 | 4 | Error Over Current Charge. |
| WOT | 2 | 5 | Error Over Temperature. |
| WUT | 2 | 6 | Error Under Temperature. |
| LOV | 3 | 0 | Error Over Voltage. |
| LUV | 3 | 1 | Error Under Voltage. |
| LOCD | 3 | 2 | Error Over Current Discharge. |
| LOCR | 3 | 3 | Error Over Current Regenerative. |
| LOCC | 3 | 4 | Error Over Current Charge. |
| LOT | 3 | 5 | Error Over Temperature. |
| LUT | 3 | 6 | Error Under Temperature. |

**Basis.** Documented elsewhere.

#### SCD

**Record.** Byte 3 bit 7.

**Wording.** SCD.

**Basis.** Documented elsewhere.

**Guess.** `SCD` means short-circuit detection. The wording stays “SCD”. Confidence: letter.

#### Outputs

| Flag | Byte | Bit | Wording |
| --- | --- | --- | --- |
| K_plus | 5 | 0 | The battery reports K_plus. |
| PMOS_E | 5 | 1 | The battery reports PMOS_E. |
| PMOS_I | 5 | 2 | The battery reports PMOS_I. |
| Heater | 5 | 4 | The battery reports Heater. |
| Fan_A | 5 | 5 | The battery reports Fan_A. |
| Fan_B | 5 | 6 | The battery reports Fan_B. |

**Basis.** Documented elsewhere.

**Guess.** `K_plus` means the high-current contactor is closed. The dash text is KPLUS OFF when the scooter will not drive, and the bit is clear while only charging. Confidence: context.

**Guess.** `PMOS_E` means the electronic switch for the scooter electrics, VBatE. Confidence: context.

**Guess.** `PMOS_I` means the electronic switch for the motor-controller supply, VBatI. That supply is the one that is off while charging. Confidence: context.

**Guess.** `Heater` means the pack heater output. Confidence: context.

#### Misc, byte 7

| Flag | Byte | Bit | Wording |
| --- | --- | --- | --- |
| Kmain error | 7 | 2 | The battery reports Kmain error. |
| HiGs Impact | 7 | 3 | The battery reports HiGs Impact. The same name is also on `0x191` byte 0 bit 3. |
| Thermal Event | 7 | 4 | The battery reports Thermal Event. |

**Basis.** Documented elsewhere.

**Guess.** `Kmain error` means the main contactor reported a fault. This is separate from `K_plus`, which is the contactor state. Confidence: context.

**Guess.** `HiGs Impact` means the shock event. The same guess applies to the same name on `0x191`. Confidence: context.

**Guess.** `Thermal Event` means the temperature event. Confidence: context.

A shock sensor treats an unusually hard impact as possible damage. An abnormal reading on one of three temperature sensors is treated as a hot spot under load. Either one leaves the high-current output off and also blocks charging. False alarms are described, including a pack that reported one while parked. It is not a listing sentence that these bits are those sensors.

### `0x185`–`0x188` Cell voltages

**Record.** Each cell is a uint16 in millivolts, little-endian, from byte 0. `0x185` is cells 1–4, `0x186` cells 5–8, `0x187` cells 9–12, `0x188` cells 13 and 14.

**Wording.** Cell voltage is in millivolts.

**Basis.** Documented elsewhere.

### `0x189` Current and probe temperatures

**Record.** Bytes 0–1 are pack current, int16, unit 10 mA. Divide by 100 for amperes. Negative is discharge, current drawn from the pack. Positive is charge or regeneration. Bytes 2–3, 4–5, and 6–7 are temperatures A, B, and C, int16, unit 0.01 °C.

**Wording.** Pack current is in amperes. A negative current is discharge. A positive current is charge or regeneration. The three temperatures are in degrees.

**Basis.** Documented elsewhere; seen on a scooter.

### `0x18E` Balancing

**Record.** Byte 0 bits 0–7 are cells 1–8 balancing. Byte 1 bits 0–5 are cells 9–14 balancing. Byte 2 bits are Enabled, Allowed, Inhibited, AUTO, Imbalance, Viddle, High Temp, and Over Temp.

**Wording.** These bits say which cells are balancing, and whether balancing is enabled, allowed, inhibited, automatic, or held for imbalance or temperature.

**Basis.** Documented elsewhere; seen on a scooter.

### `0x191` Extra battery flags

**Record.** Byte 0 bit 2 is Kopen. Byte 0 bit 3 is HiGs Impact, the same name as `0x184` byte 7 bit 3. The section name is Inhibick. The section carries the comment “Inhibit + Lock”, which is not a definition of Kopen, INH, or ILDSCH.

**Wording.** The battery reports Kopen. HiGs Impact is the same name as on the battery status frame.

**Basis.** Documented elsewhere.

### `0x200` Clock

**Record.** Broadcast. No request. BCD. Byte 0 bit 1 set means the clock is valid. Bytes 1–6 are hour, minute, second, day, month, and a two-digit year.

**Wording.** This is the battery clock. It counts only when the clock is marked valid.

**Basis.** Documented elsewhere; seen on a scooter.

### `0x401` Throttle and drive switches

**Record.** Byte 4 bits are FS1, Forward, Reverse, DM1, DM2, and DM3.

**Wording.** These are the throttle and drive-direction switches.

**Basis.** Documented elsewhere; seen on a scooter.

## ECU flags

### `0x28A` Status

**Record.** Broadcast status. Byte 0 bit 0 is God Mode. Byte 3 bits are Sending on debug mode, BMS timeout, and Sevcon timeout. Byte 4 bits are PCB undervoltage, BTSoff, BTSon, Normal PCB voltage, and PCB overvoltage.

The same identifier is also a command. See Debug and god mode below.

**Wording.** This reports ECU god mode, debug mode, timeouts, and the ECU supply voltage.

**Basis.** Documented elsewhere; seen on a scooter.

### `0x293`

**Record.** Byte 6 bits: mode switch is not pressed, ready-to-drive is 0, throttle is not released, alarm is not fired, controller run time is over the minimum delay, not in charging mode, not in charged mode, not in warnings mode. Byte 7 bits: PMOS I is active, no MCU timeout, battery sensor detects a battery, left brake is pressed.

**Wording.** These are ECU conditions for drive, charging, the battery output, and the left brake.

**Basis.** Documented elsewhere; seen on a scooter.

**Guess.** word 1 is a supply voltage reading that sags under load, and word 0, near half of a 12-bit range, is the zero level of a current sensor. Confidence: none.

### `0x294`

**Record.** Byte 0 bits: Brake L, Normal beam, Warnings, Brake R, Blink L, High beam, Blink R, Reverse. Byte 1 bits: Side stand, Mode, Horn, Seat actuator, Battery block, Main stand, Helmet 1, Helmet 2.

Bytes 2 and 3 are the ECU's outputs. Byte 2 bits: Blink L, Blink R, Highbeam, Position, Normalbeam, Brake L, Brake R, Horn. Byte 3 bits: Seat actuator, Fans, Battery block, White R (bit 3), White L (bit 4), Buzzer, Dash, Steering Lock. **White L and White R are the other way round from an earlier listing**, which has bit 3 as White L and bit 4 as White R (see below).

**Wording.** These are the lamps, brakes, stands, horn, and seat and helmet switches, and what the ECU is switching on: lamps, horn, seat actuator, fans, battery block, buzzer, dash and steering lock.

**Basis.** Documented elsewhere; seen on a scooter.

## Asked frames

These are not on the bus until something asks. The monitor sends read requests only: one pack-identity request after it connects, and the parameter and event-log reads when a Read button is pressed. It writes nothing.

### `0x234` / `0x235` / `0x233` Battery parameters

**Record.** `0x234` asks, byte 0 is the index. `0x235` replies with the index and the value. `0x233` writes. A commit is index `0xFF`. Index `0x21` is the pack identity and is not part of the bulk write.

**Wording.** These are stored battery settings. Reading them does not change them.

**Basis.** Documented elsewhere.

### `0x31B` / `0x31C` / `0x31A` ECU parameters

**Record.** `0x31B` asks. `0x31C` replies. `0x31A` writes. There is no `0xFF` commit. A capture shows the VIN on `0x31C`.

**Wording.** These are stored ECU settings. Reading them does not change them.

**Basis.** Documented elsewhere.

### `0x201` Clock set

**Record.** Seven bytes. Byte 0 is `0x04`. The rest is BCD hour, minute, second, day, month, and year.

**Wording.** This sets the battery clock.

**Wording.** Setting the clock sends one message to the battery with this device's date and time, and the battery writes the change in its event log.

**Wording.** The battery clock now matches this device's time.

**Wording.** The battery clock still does not match this device's time.

**Wording.** The battery clock is more than a minute away from this device's clock.

**Basis.** Documented elsewhere; seen on a scooter.

### `0x1ED` / `0x1EE` Event log

**Record.** `0x1ED` asks. An empty first request brings the newest ten records. A four-byte request is a start index and a count of records, counting down from the start. `0x1EE` returns three frames per record: index and date, then the event code in bytes 2–3 of the second frame.

**Wording.** This is the battery event log. Each line is a time and an event code.

**Wording.** The log is read ten records at a time, with a short pause between, because reading it all at once can make the motor controller report a fault.

**Wording.** The motor controller reported a fault, so the read was stopped; switch the scooter off and on again to clear it.

**Basis.** Documented elsewhere; seen on a scooter.

| Code | Wording |
| --- | --- |
| `0x1010` | Apagado |
| `0x1021` | Encendido periodico AFE refresh reading |
| `0x1022` | Encendido por llave |
| `0x1023` | Encendido por cargador interno |
| `0x1024` | Encendido por cargador externo |
| `0x10A0` | Inicio de sistema: motivo de reset |
| `0x1100` | Outputs Forced Disable |
| `0x1110` | Keyless Enabled |
| `0x1120` | Keyless Disabled |
| `0x1121` | Keyless Disabled Timeout |
| `0x1200` | Shutdown Event |
| `0x1A00` | Enviado mensaje Status |
| `0x1A01` | Enviado mensaje Miguel |
| `0x1C00` | Interlock cambio |
| `0x2201` | Error Over Voltage |
| `0x2202` | Error Under Voltage |
| `0x2203` | Error Over Current Discharge |
| `0x2204` | Error Over Current Regenerative |
| `0x2205` | Error Over Current Charge |
| `0x2206` | Error Over Temperature |
| `0x2207` | Error Under Temperature |
| `0x2208` | SCD |
| `0x2301` | Shutdown Undervoltage: 12V.UI , VbatE , etc |
| `0x2302` | Harakiri Voltage: 5V.UI , +5V.uC , etc. |
| `0x3004` | Cambio de hora via CANbus half (4 bytes) |
| `0x3007` | Cambio de hora via CANbus full (7 bytes) |
| `0x3010` | Cambio de hora via GPS (automatico) |
| `0x3011` | GPS OFFSET |
| `0x3101` | MCU BOOTLOADER Update |
| `0x3102` | MCU BOOTLOADER Updated |
| `0x3F00` | Downlink |
| `0x4020` | Parameters Flags |
| `0x4021` | Parameters BPID |
| `0x5A01` | Stack Overflow |

### `0x250` Battery reset

**Record.** Data bytes `FA 05`. No reply is defined. This does not write the parameter listing and it does not clear the event log.

**Wording.** This restarts the battery manager. The event log then records a start and the reason.

**Basis.** Documented elsewhere.

### `0x28A` Debug and god mode command

**Record.** Eight bytes. Debug on is byte 3 = `0x01`. Debug off is all zeros. God mode on is byte 0 = `0x01` and byte 3 = `0x01`. God mode off is byte 3 = `0x01` and byte 0 clear, so debug stays on.

**Wording.** Debug mode and god mode are two ECU controls on the same message. Turning god mode off leaves debug mode on.

**Basis.** Documented elsewhere; seen on a scooter.

## Seen on a scooter, layout not described

**Basis.** Documented elsewhere.

### `0x182`

**Record.** Labeled an ECU state frame. Byte 1 state. Byte 4 warnings. DLC 7. No field layout is settled.

**Wording.** The ECU reports a state on this message. The fields are not settled.

**Basis.** Seen on a scooter.

### `0x190`

**Record.** Byte 1 state of charge in thousandths. Bytes 2–3 remaining capacity, mAh. Byte 4 VSOC. Byte 5 SOH. DLC 5.

**Wording.** This is a finer state of charge, the remaining capacity, and a state of health.

**Basis.** Seen on a scooter.

### `0x197`

**Record.** DLC 8. Bytes 0–1 are capacity left, uint16, unit 0.01 Ah. Bytes 4–5 are a second uint16 that falls in step with it. Bytes 2–3 are a small signed number. Bytes 6 and 7 were `64 64`.

**Wording.** This is the capacity left in the pack, in ampere-hours, from bytes 0–1. The other bytes are not shown.

**Basis.** Seen on a scooter.

### `0x19A`

**Record.** Present. Layout unknown.

**Wording.** This message is on the bus. Its contents are not known.

**Basis.** Seen on a scooter.

### `0x19B`

**Record.** DLC 5. Five bytes, each between `0A` and `10` throughout the capture.

**Wording.** This message is on the bus. Its contents are not known.

**Basis.** Seen on a scooter.

**Guess.** five temperatures in whole degrees, one per sensor. Confidence: context.

### `0x1AA`

**Record.** Bytes 0–1 maximum discharge current, A. Bytes 2–3 maximum recharge current, A. DLC 4.

**Wording.** This is the maximum current the battery will allow for discharge and for charge.

**Basis.** Documented elsewhere.

### `0x1AC`

**Record.** Bytes 0–3 are a uint32 that counts up. Bytes 4–7 are zero. An earlier listing put the count in bytes 4–7 and marked it uncertain.

**Wording.** This may be the time since the battery manager last restarted. That reading is uncertain.

**Basis.** Seen on a scooter.

**Record.** DLC 8. Bytes not named.

**Wording.** This message is on the bus. Its contents are not settled.

**Basis.** Documented elsewhere; seen on a scooter.

### `0x1B1`

**Record.** Present. Layout unknown.

**Wording.** This message is on the bus. Its contents are not settled.

**Basis.** Seen on a scooter.

### `0x1C0`

**Record.** Bytes 1–4 energy charged since on, watt-seconds. Bytes 5–7 energy discharged since on, watt-seconds. DLC 8.

**Wording.** This is the energy charged and discharged since the scooter was switched on.

**Basis.** Documented elsewhere; seen on a scooter.

### `0x1C1`

**Record.** Bytes 1–4 energy regenerated since on, watt-seconds. DLC 8.

**Wording.** This is the energy regenerated since the scooter was switched on.

**Basis.** Seen on a scooter.

### `0x1DB`

**Record.** Present. Layout unknown.

**Wording.** This message is on the bus. Its contents are not known.

### `0x20A`, `0x20B`

**Record.** DLC 8. Bytes unknown.

**Wording.** These messages are on the bus. Their contents are not known.

### `0x220`, `0x230`, `0x231`

**Record.** Present. `0x230` is DLC 7.

**Wording.** These messages are on the bus. Their contents are not known.

**Basis.** Documented elsewhere.

### `0x280`

**Record.** DLC 4. Byte 0 bit 0 is ready to drive. Bit 2 was set while reverse was held. Bit 3 is the side stand down. Bits 4–5 are the drive mode: 0 off, 1 ECO, 2 SPORT, 3 CITY. Bits 1, 6 and 7 are not settled. Byte 1 is the lamps: bit 0 left indicator, bit 1 right indicator, bit 2 high beam, bit 3 low beam, bit 7 hazards (`08` once the key is on is the low beam). Bytes 2 and 3 were zero.

**Wording.** This is the drive mode, whether the side stand is down, whether the scooter is ready to drive, and the other drive switches.

**Basis.** Documented elsewhere; seen on a scooter.

### `0x290`, `0x291`, `0x292`

**Record.** ECU messages, DLC 8, 50 Hz. Read as four little-endian 16-bit words each. They were seen only with debug mode on, on one scooter. `0x290`: word 0 about 660, word 1 about 1759, word 2 2817, nearly constant (moves of one or two counts), bytes 6 and 7 zero. `0x291`: word 0 about 710 while the right blinker output was on, word 1 about 719 while the left blinker output was on, word 2 always 0, word 3 about 574 while the high beam output was on, and each of them near 0 otherwise. `0x292`: word 0 about 581 while the low beam output was on, word 1 about 722 while the lamp that goes out with the left indicator was on (`0x294` byte 3 bit 4), word 2 about 707 for the other white lamp (bit 3), and word 3 about 412 throughout (397 to 420), while Position was on.

**Wording.** These messages are on the bus. Their contents are not known.

**Basis.** Seen on a scooter.

**Guess.** the words of `0x291` and `0x292` are the current drawn by each lamp output, as the ECU measures it, in counts. Confidence: context.

**Guess.** `0x290` is slow analog readings, such as supply or reference levels. Confidence: none.

### `0x29F`

**Record.** DLC 5, 50 Hz. All five bytes were zero in all 1,760 frames of the same recording. Not seen in any capture of ours.

**Wording.** This message is on the bus. Its contents are not known.

**Basis.** Seen on a scooter.

**Guess.** fault flags, for example a lamp that is drawing too little current. Confidence: none.

### `0x297`

**Record.** Bytes 0–3 are a uint32 that counts up, in milliseconds. Bytes 4–7 were zero. An earlier listing put the count in bytes 4–7 and called it the time since ignition on.

**Wording.** This is a timer that counts up in milliseconds. It is not the time since the ignition was switched on. What it counts from is not known.

**Basis.** Seen on a scooter.

**Record.** Present. Layout unknown.

**Wording.** These messages are on the bus. Their contents are not known.

### `0x300`

**Record.** DLC 3. Byte 0 speed, km/h. Byte 1 remaining range, km (not yet checked against the scooter's display). Byte 2 is a small signed number that follows the pack current and is not shown. There is no byte 4, so there is no power bar on this scooter.

**Wording.** This is road speed and the remaining range.

**Basis.** Documented elsewhere; seen on a scooter.

**Guess.** byte 1 is the remaining range. Confidence: context.

### `0x301`

**Record.** DLC 6. Bytes 0–1 motor temperature, int16, 0.1 °C. Bytes 2–3 inverter temperature, int16, 0.1 °C. Bytes 4–5 a third temperature, int16, 0.1 °C. An earlier listing numbered these from 1.

**Wording.** This is the motor temperature, the inverter temperature, and a third temperature.

**Basis.** Seen on a scooter.

### `0x310`

**Record.** DLC 8. Text for the dash, up to nine characters. The low seven bits of the eight bytes are the first eight characters. The high bit of each byte carries the ninth: the high bit of byte 0 is bit 0 of that character, byte 1 is bit 1, and so on up to byte 7. A zero byte ends the text, and all eight bytes zero mean no message. Seen on two captures: `53 49 C4 45 53 54 C1 4E` is SIDESTAND (at 217.0 s of the 25 Oct capture, and all eight bytes were zero at 217.9 s), and `4B F0 EC 75 73 20 CF 46` is Kplus OFF.

**Wording.** The text sent to the dash, up to nine characters, for messages such as the side stand being down. It is empty when no message is showing. That is a guess as to the purpose.

**Basis.** Documented elsewhere; seen on a scooter.

**Guess.** a text channel to the dash. Two different words on the same identifier (CHAR from a published listing, SIDESTAN here) fit it, and the SIDESTAN text appeared while `0x280` showed the side stand down. Confidence: context.

### `0x313`

**Record.** ASCII bytes `S T A N`. DLC 8.

**Wording.** This message carries the characters STAN. What they stand for is not known.

**Basis.** Seen on a scooter.

### `0x371`

**Record.** Motor controller. Bytes 0–1 motor Ud, 1/16 V. Bytes 2–3 motor Iq reference, 1/16 V in the capture. Byte 4 SVM index, 100/255 percent. Bytes 6–7 are the controller bus voltage, uint16 in 1/16 V (an earlier listing read them as a measured inductance). DLC 8.

**Wording.** Bytes 6–7 are shown as the controller bus voltage, the voltage inside the motor controller. It has been checked in one capture only, not against a second source. The other bytes are not shown.

**Basis.** Documented elsewhere; seen on a scooter.

### `0x381`

**Record.** DLC 8. Four int16 fields. Bytes 0–1 and 4–5 move together and go more negative as motor speed rises. Bytes 2–3 and 6–7 move together and follow the throttle. Probably the Id and Iq current commands and actual currents, the same layout as `0x393`.

**Wording.** This message carries motor-controller currents. The units and the order are not settled, and the monitor does not show them.

**Basis.** Seen on a scooter.

### `0x391`

**Record.** DLC 8. Bytes 0–3 motor speed, int32, rpm. It is negative in reverse. Bytes 4–5 are a uint16 that rises with the throttle. Bytes 6–7 are a uint16 that is not settled. An earlier listing numbered the bytes from 1 and the other fields are not settled.

**Wording.** This is the motor speed from the motor controller. It is negative in reverse.

**Basis.** Documented elsewhere; seen on a scooter.

### `0x393`

**Record.** Four int16 fields in 1/16 A: Id reference, Iq reference, Id actual, Iq actual. DLC 8.

**Wording.** These are the motor current commands and the currents the motor is actually drawing.

### `0x411`

**Record.** Bytes 0–1 left-motor status word. Bytes 2–3 highest active fault. Byte 4 heatsink temperature, °C. Byte 6 motor temperature, °C. DLC 7.

**Wording.** This is a motor status word, the highest fault, and the heatsink and motor temperatures.

**Basis.** Seen on a scooter.

### `0x421`

**Record.** Byte 0 footbrake switch. DLC 1. Also listed as an ECU switch in the listing work for the motor controller.

**Wording.** This is the footbrake switch.

**Basis.** Documented elsewhere.

### `0x422`

**Record.** Four int16 fields: battery current 1/16 A, capacitor voltage 1/16 V (the capture says 1/16 A), torque demand 1/16 N·m, torque actual 1/16 N·m. DLC 8.

**Wording.** These are battery current, capacitor voltage, and the torque asked for and the torque delivered. The monitor shows the capacitor voltage as the controller bus voltage, and takes it from `0x371` bytes 6–7 when this message is not sent (it was not sent in the capture). The same four fields arrived on `0x431`, and the monitor decodes both identifiers the same way, so a scooter that sends `0x422` fills the same readings.

### `0x431`

**Record.** DLC 8. Four int16 fields in 1/16 units: battery current, controller bus voltage, torque asked for, torque delivered. This is the record other listings give under `0x422`.

**Wording.** These are the battery current as the motor controller reports it, the controller bus voltage, and the torque asked for and the torque delivered. The units are as recorded and have not been checked against a second source.

**Basis.** Seen on a scooter.

### `0x500`

**Record.** Display. Byte 0 outside temperature, °C. Bytes 4–6 odometer, km. DLC 8.

**Wording.** This is the outside temperature and the odometer.

**Basis.** Seen on a scooter.

### `0x501`

**Record.** DLC 0.

**Wording.** This message is on the bus with no data bytes. What it means is not known.

### `0x502`

**Record.** DLC 2. Byte 0 is a 2-bit rolling counter on top of `58`, running `58`, `59`, `5A`, `5B`, `58`. Byte 1 was `01`. It is sent only when the counter changes.

**Wording.** This message is on the bus. It carries a counter. What it counts is not known.

**Basis.** Seen on a scooter.

### `0x601`

**Record.** Present. Layout unknown.

**Wording.** This message is on the bus. Its contents are not known.

### `0x701`

**Record.** Byte 0 operating or pre-operational mode. DLC 1.

**Wording.** This says whether the motor controller is in operating mode or still starting.
