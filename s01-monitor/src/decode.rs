use crate::snapshot::{Bytes, Frame, Snapshot};

/// Apply one received frame. Identifiers outside the live set are ignored.
pub fn apply(snapshot: &mut Snapshot, frame: &Frame) {
    let data = frame.payload();
    match frame.id {
        0x183 => {
            if data.len() >= 4 {
                let raw = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                if raw != u32::MAX {
                    snapshot.pack_id = Some(raw.to_string());
                }
            }
        }
        0x181 => {
            if let Some(counter) = data.first() {
                snapshot.counter = Some(*counter);
            }
            if let Some(soc) = data.get(1) {
                snapshot.soc = Some(f32::from(*soc));
            }
            if let Some(low) = data.get(2) {
                snapshot.temp_low = Some(*low as i8);
            }
            if let Some(high) = data.get(3) {
                snapshot.temp_high = Some(*high as i8);
            }
            if let Some(raw) = u16_at(data, 4) {
                snapshot.voltage = Some(f32::from(raw) / 10.0);
            }
            if let Some(raw) = i16_at(data, 6) {
                snapshot.current_summary = Some(f32::from(raw) / 10.0);
            }
        }
        0x197 => {
            if let Some(raw) = u16_at(data, 0) {
                snapshot.capacity_ah = Some(f32::from(raw) / 100.0);
            }
        }
        0x189 => {
            if let Some(raw) = i16_at(data, 0) {
                snapshot.current = Some(f32::from(raw) / 100.0);
            }
            if let Some(raw) = i16_at(data, 2) {
                snapshot.temp_a = Some(f32::from(raw) / 100.0);
            }
            if let Some(raw) = i16_at(data, 4) {
                snapshot.temp_b = Some(f32::from(raw) / 100.0);
            }
            if let Some(raw) = i16_at(data, 6) {
                snapshot.temp_c = Some(f32::from(raw) / 100.0);
            }
        }
        0x185 | 0x186 | 0x187 | 0x188 => {
            let start = match frame.id {
                0x185 => 0,
                0x186 => 4,
                0x187 => 8,
                _ => 12,
            };
            let mut offset = 0;
            while offset + 1 < data.len() && start + offset / 2 < 14 {
                if let Some(millivolts) = u16_at(data, offset) {
                    snapshot.cells[start + offset / 2] = Some(f32::from(millivolts));
                }
                offset += 2;
            }
        }
        0x184 => snapshot.bms = Some(Bytes::from(frame)),
        0x18E => snapshot.balance = Some(Bytes::from(frame)),
        0x191 => snapshot.extra = Some(Bytes::from(frame)),
        0x200 => {
            snapshot.battery_clock = crate::clock::from_frame(data);
            if data.first().is_some_and(|status| status & 0x02 != 0) && data.len() >= 4 {
                snapshot.clock = Some(format!("{:02X}:{:02X}:{:02X}", data[1], data[2], data[3]));
            }
        }
        0x293 => snapshot.ready = Some(Bytes::from(frame)),
        0x294 => snapshot.io = Some(Bytes::from(frame)),
        0x28A => snapshot.ecu_status = Some(Bytes::from(frame)),
        0x401 => {
            snapshot.sevcon = Some(Bytes::from(frame));
            if let Some(raw) = u16_at(data, 0) {
                snapshot.throttle = Some(f32::from(raw));
            }
        }
        0x371 => {
            // Same quantity as 0x422 bytes 2-3 (1/16 V); it follows the pack
            // voltage but moves under load, so it is a separate measurement.
            if let Some(raw) = u16_at(data, 6) {
                snapshot.cap_voltage = Some(f32::from(raw) / 16.0);
            }
        }
        0x190 => {
            if let Some(fine) = data.get(1) {
                snapshot.soc_fine = Some(u16::from(*fine));
            }
            if let Some(raw) = u16_at(data, 2) {
                snapshot.remain_mah = Some(raw);
            }
            if let Some(vsoc) = data.get(4) {
                snapshot.vsoc = Some(*vsoc);
            }
            if let Some(soh) = data.get(5) {
                snapshot.soh = Some(*soh);
            }
        }
        0x1AA => {
            if let Some(raw) = u16_at(data, 0) {
                snapshot.max_discharge = Some(raw);
            }
            if let Some(raw) = i16_at(data, 2) {
                snapshot.max_charge = Some(raw);
            }
        }
        0x1AC => {
            if let Some(raw) = u32_at(data, 0) {
                snapshot.since_reset = Some(raw / 1000);
            }
        }
        0x1C0 => {
            if let Some(raw) = u32_at(data, 0) {
                snapshot.charged_ws = Some(raw);
            }
            if let Some(raw) = u32_at(data, 4) {
                snapshot.discharged_ws = Some(raw);
            }
        }
        0x1C1 => {
            if let Some(raw) = u32_at(data, 0) {
                snapshot.regenerated_ws = Some(raw);
            }
        }
        0x280 => {
            if let Some(mode) = data.first() {
                snapshot.drive_flags = Some(*mode);
            }
        }
        0x297 => {
            if let Some(raw) = u32_at(data, 0) {
                snapshot.since_ignition = Some(raw);
            }
        }
        0x300 => {
            if let Some(raw) = data.first() {
                snapshot.speed = Some(f32::from(*raw));
            }
            if let Some(range) = data.get(1) {
                snapshot.range_km = Some(f32::from(*range));
            }
            if let Some(bar) = data.get(4) {
                snapshot.power_bar = Some(*bar);
            }
        }
        0x301 => {
            if let Some(raw) = i16_at(data, 0) {
                snapshot.motor = Some(f32::from(raw) / 10.0);
            }
            if let Some(raw) = i16_at(data, 2) {
                snapshot.inverter = Some(f32::from(raw) / 10.0);
            }
            if let Some(raw) = i16_at(data, 4) {
                snapshot.temp_third = Some(f32::from(raw) / 10.0);
            }
        }
        0x310 => snapshot.mark_char = Some(dash_text(data)),
        0x313 => snapshot.mark_stan = ascii(data),
        0x391 => {
            if let Some(raw) = i32_at(data, 0) {
                snapshot.rpm = Some(raw as f32);
            }
        }
        0x411 => {
            if let Some(heat) = data.get(4) {
                snapshot.heatsink = Some(f32::from(*heat));
            }
            if let Some(motor) = data.get(6) {
                snapshot.motor_ctrl = Some(f32::from(*motor));
            }
        }
        0x421 => {
            if let Some(state) = data.first() {
                snapshot.footbrake = Some(*state != 0);
            }
        }
        // The same record. This scooter sends it as 0x431. Another listing calls it 0x422, so either identifier fills the same readings.
        0x422 | 0x431 => {
            if let Some(raw) = i16_at(data, 0) {
                snapshot.battery_current = Some(f32::from(raw) / 16.0);
            }
            if let Some(raw) = i16_at(data, 2) {
                snapshot.cap_voltage = Some(f32::from(raw) / 16.0);
            }
            if let Some(raw) = i16_at(data, 4) {
                snapshot.torque_demand = Some(f32::from(raw) / 16.0);
            }
            if let Some(raw) = i16_at(data, 6) {
                snapshot.torque_actual = Some(f32::from(raw) / 16.0);
            }
        }
        0x500 => {
            if let Some(temp) = data.first() {
                snapshot.outside = Some(*temp as i8);
            }
            if let Some(raw) = u24_at(data, 4) {
                snapshot.odometer = Some(raw);
            }
        }
        0x701 => {
            if let Some(mode) = data.first() {
                snapshot.controller = Some(*mode);
            }
        }
        _ => {}
    }
}

fn u16_at(data: &[u8], index: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        *data.get(index)?,
        *data.get(index + 1)?,
    ]))
}

fn i16_at(data: &[u8], index: usize) -> Option<i16> {
    Some(i16::from_le_bytes([
        *data.get(index)?,
        *data.get(index + 1)?,
    ]))
}

fn u24_at(data: &[u8], index: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *data.get(index)?,
        *data.get(index + 1)?,
        *data.get(index + 2)?,
        0,
    ]))
}

fn i32_at(data: &[u8], index: usize) -> Option<i32> {
    u32_at(data, index).map(|raw| raw as i32)
}

fn u32_at(data: &[u8], index: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *data.get(index)?,
        *data.get(index + 1)?,
        *data.get(index + 2)?,
        *data.get(index + 3)?,
    ]))
}

/// The text for the dash, up to nine characters. Eight sit in the low seven
/// bits of the eight bytes. The high bit of each byte carries the ninth: the
/// high bit of byte 0 is bit 0 of that character, byte 1 is bit 1, and so on
/// (a published report; it fits both messages in recordings:
/// `53 49 C4 45 53 54 C1 4E` is SIDESTAND and `4B F0 EC 75 73 20 CF 46` is
/// Kplus OFF). A zero byte ends the text, and a ninth character that is not
/// printable is left out. All zero bytes mean the message has cleared.
fn dash_text(data: &[u8]) -> String {
    let bytes = &data[..data.len().min(8)];
    let mut text: String = bytes
        .iter()
        .map(|byte| byte & 0x7F)
        .take_while(|byte| *byte != 0)
        .map(char::from)
        .collect();
    // The ninth character follows the eighth, so it is only there if no zero
    // byte cut the text short.
    if text.len() == bytes.len() {
        let ninth = bytes
            .iter()
            .enumerate()
            .fold(0u8, |bits, (place, byte)| bits | ((byte >> 7) << place));
        if (0x20..0x7F).contains(&ninth) {
            text.push(char::from(ninth));
        }
    }
    if text.trim().is_empty() {
        "Clear".into()
    } else {
        text
    }
}

fn ascii(data: &[u8]) -> Option<String> {
    let end = data.iter().position(|byte| *byte == 0).unwrap_or(data.len());
    let text = String::from_utf8_lossy(&data[..end]).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_live_broadcast() {
        let mut snapshot = Snapshot::default();
        apply(
            &mut snapshot,
            &Frame::new(0x181, &[0x00, 82, 0, 0, 0x1E, 0x02, 0, 0]),
        );
        apply(
            &mut snapshot,
            &Frame::new(0x189, &[0x98, 0xFE, 0x30, 0x0C, 0x80, 0x0C, 0xE0, 0x0B]),
        );
        apply(
            &mut snapshot,
            &Frame::new(0x185, &[0x50, 0x0F, 0x41, 0x0F, 0x3A, 0x0F, 0x2C, 0x0F]),
        );
        apply(&mut snapshot, &Frame::new(0x188, &[0x46, 0x0F, 0x23, 0x0F]));

        assert_eq!(snapshot.soc, Some(82.0));
        assert_eq!(snapshot.voltage, Some(54.2));
        assert_eq!(snapshot.current, Some(-3.6));
        assert_eq!(snapshot.temp_a, Some(31.2));
        assert_eq!(snapshot.temp_b, Some(32.0));
        assert_eq!(snapshot.temp_c, Some(30.4));
        assert_eq!(snapshot.cells[0], Some(3920.0));
        assert_eq!(snapshot.cells[3], Some(3884.0));
        assert_eq!(snapshot.cells[12], Some(3910.0));
        assert_eq!(snapshot.cells[13], Some(3875.0));
    }

    #[test]
    fn clock_uses_bcd_bytes_when_valid() {
        let mut snapshot = Snapshot::default();
        apply(
            &mut snapshot,
            &Frame::new(0x200, &[0x03, 0x14, 0x32, 0x08, 0x19, 0x10, 0x26]),
        );
        assert_eq!(snapshot.clock.as_deref(), Some("14:32:08"));

        apply(&mut snapshot, &Frame::new(0x200, &[0x00, 0x01, 0x02, 0x03]));
        assert_eq!(snapshot.clock.as_deref(), Some("14:32:08"));
    }

    #[test]
    fn ignores_parameter_and_event_frames() {
        let mut snapshot = Snapshot::default();
        apply(
            &mut snapshot,
            &Frame::new(0x234, &[0x21, 0, 0, 0, 0, 0, 0, 0]),
        );
        apply(&mut snapshot, &Frame::new(0x1ED, &[0, 0, 0, 0, 0, 0, 0, 0]));
        apply(&mut snapshot, &Frame::new(0x183, &[0xFF, 0xFF, 0xFF, 0xFF]));
        assert!(!snapshot.has_signal());
        assert_eq!(snapshot.pack_id.as_deref(), None);
        apply(&mut snapshot, &Frame::new(0x183, &[0x19, 0xE2, 0xC3, 0xA1]));
        assert!(!snapshot.has_signal());
        assert_eq!(snapshot.pack_id.as_deref(), Some("2713969177"));
    }

    #[test]
    fn decodes_named_readings() {
        let mut snapshot = Snapshot::default();
        apply(
            &mut snapshot,
            &Frame::new(0x181, &[12, 82, 30, 33, 0x1E, 0x02, 0, 0]),
        );
        apply(
            &mut snapshot,
            &Frame::new(0x190, &[0, 200, 0xE8, 0x03, 80, 97]),
        );
        apply(&mut snapshot, &Frame::new(0x1AA, &[80, 0, 12, 0]));
        apply(
            &mut snapshot,
            &Frame::new(0x1AC, &[0xA0, 0x86, 0x01, 0x00, 0, 0, 0, 0]),
        );
        apply(
            &mut snapshot,
            &Frame::new(0x1C0, &[0, 0xE8, 0x03, 0, 0, 0xF4, 0x01, 0]),
        );
        apply(
            &mut snapshot,
            &Frame::new(0x1C1, &[0, 0x68, 0x10, 0, 0, 0, 0, 0]),
        );
        apply(&mut snapshot, &Frame::new(0x280, &[0x21, 0x08, 0, 0]));
        apply(
            &mut snapshot,
            &Frame::new(0x297, &[0x78, 0x05, 0, 0, 0, 0, 0, 0]),
        );
        apply(&mut snapshot, &Frame::new(0x300, &[36, 48, 0, 0, 55]));
        apply(
            &mut snapshot,
            &Frame::new(0x301, &[0x5A, 0x00, 0xA0, 0x00, 0x5A, 0x00]),
        );
        apply(&mut snapshot, &Frame::new(0x310, b"CHAR"));
        apply(&mut snapshot, &Frame::new(0x313, b"STAN"));
        assert_eq!(snapshot.mark_char.as_deref(), Some("CHAR"));
        apply(
            &mut snapshot,
            &Frame::new(0x391, &[0x5C, 0x08, 0, 0, 19, 0, 0x70, 0x01]),
        );
        apply(
            &mut snapshot,
            &Frame::new(0x411, &[0, 0, 0, 0, 44, 0, 48]),
        );
        apply(&mut snapshot, &Frame::new(0x421, &[0]));
        apply(
            &mut snapshot,
            &Frame::new(0x422, &[0xC6, 0xFF, 0x62, 0x03, 6, 0, 3, 0]),
        );
        apply(
            &mut snapshot,
            &Frame::new(0x500, &[14, 0, 0, 0, 0x7A, 0x10, 0, 0]),
        );
        apply(&mut snapshot, &Frame::new(0x401, &[0xB0, 0x04, 0, 0, 0x13]));
        apply(&mut snapshot, &Frame::new(0x701, &[5]));

        assert_eq!(snapshot.counter, Some(12));
        assert_eq!(snapshot.temp_low, Some(30));
        assert_eq!(snapshot.temp_high, Some(33));
        assert_eq!(snapshot.soc_fine, Some(200));
        assert_eq!(snapshot.remain_mah, Some(1000));
        assert_eq!(snapshot.vsoc, Some(80));
        assert_eq!(snapshot.soh, Some(97));
        assert_eq!(snapshot.max_discharge, Some(80));
        assert_eq!(snapshot.max_charge, Some(12));
        assert_eq!(snapshot.since_reset, Some(100));
        assert_eq!(snapshot.charged_ws, Some(256_000));
        assert_eq!(snapshot.discharged_ws, Some(128_000));
        assert_eq!(snapshot.regenerated_ws, Some(1_075_200));
        assert_eq!(snapshot.drive_flags, Some(0x21));
        assert_eq!(snapshot.since_ignition, Some(1400));
        assert_eq!(snapshot.speed, Some(36.0));
        assert_eq!(snapshot.range_km, Some(48.0));
        assert_eq!(snapshot.power_bar, Some(55));
        assert!((snapshot.motor.unwrap() - 9.0).abs() < 0.05);
        assert!((snapshot.inverter.unwrap() - 16.0).abs() < 0.05);
        assert!((snapshot.temp_third.unwrap() - 9.0).abs() < 0.05);
        assert_eq!(snapshot.mark_stan.as_deref(), Some("STAN"));
        assert_eq!(snapshot.rpm, Some(2140.0));
        assert_eq!(snapshot.heatsink, Some(44.0));
        assert_eq!(snapshot.motor_ctrl, Some(48.0));
        assert_eq!(snapshot.footbrake, Some(false));
        assert!((snapshot.battery_current.unwrap() - (-58.0 / 16.0)).abs() < 0.001);
        assert!((snapshot.cap_voltage.unwrap() - (866.0 / 16.0)).abs() < 0.001);
        assert!((snapshot.torque_demand.unwrap() - (6.0 / 16.0)).abs() < 0.001);
        assert!((snapshot.torque_actual.unwrap() - (3.0 / 16.0)).abs() < 0.001);
        assert_eq!(snapshot.outside, Some(14));
        assert_eq!(snapshot.odometer, Some(4218));
        assert_eq!(snapshot.throttle, Some(1200.0));
        assert_eq!(snapshot.controller, Some(5));
        apply(&mut snapshot, &Frame::new(0x300, &[0x00, 0x52, 0x00]));
        assert_eq!(snapshot.speed, Some(0.0));
        apply(&mut snapshot, &Frame::new(0x1AA, &[0x18, 0x01, 0xB0, 0xFF]));
        assert_eq!(snapshot.max_discharge, Some(280));
        assert_eq!(snapshot.max_charge, Some(-80));
    }

    /// Frames copied from the capture of 25 Oct 2026 (scooter on its stand,
    /// key on, throttle blips, reverse, side stand).
    fn real(snapshot: &mut Snapshot, id: u16, hex: &str) {
        let bytes: Vec<u8> = hex
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).unwrap())
            .collect();
        apply(snapshot, &Frame::new(id, &bytes));
    }

    #[test]
    fn frames_from_the_real_capture() {
        let mut snapshot = Snapshot::default();
        real(&mut snapshot, 0x181, "74 64 09 09 3D 02 FC FF");
        real(&mut snapshot, 0x189, "D2 FF 94 03 9F 03 A3 03");
        assert_eq!(snapshot.soc, Some(100.0));
        assert!((snapshot.voltage.unwrap() - 57.3).abs() < 0.01);
        assert!((snapshot.current.unwrap() - -0.46).abs() < 0.001);
        assert!((snapshot.temp_a.unwrap() - 9.16).abs() < 0.001);

        // Key on, wheel still.
        real(&mut snapshot, 0x300, "00 52 00");
        real(&mut snapshot, 0x391, "00 00 00 00 0C 00 70 01");
        real(&mut snapshot, 0x401, "00 00 00 00 02");
        real(&mut snapshot, 0x280, "21 08 00 00");
        real(&mut snapshot, 0x297, "F4 58 03 00 00 00 00 00");
        real(&mut snapshot, 0x301, "5A 00 A0 00 5A 00");
        real(&mut snapshot, 0x500, "08 58 00 00 F7 24 00 00");
        assert_eq!(snapshot.speed, Some(0.0));
        assert_eq!(snapshot.range_km, Some(82.0));
        assert_eq!(snapshot.rpm, Some(0.0));
        assert_eq!(snapshot.drive_flags, Some(0x21));
        assert_eq!(snapshot.since_ignition, Some(219_380));
        assert_eq!(snapshot.odometer, Some(9463));
        assert!((snapshot.inverter.unwrap() - 16.0).abs() < 0.05);

        // Throttle blip. Speed follows motor speed at about 0.1127 km/h per rpm.
        real(&mut snapshot, 0x300, "58 52 FB");
        real(&mut snapshot, 0x391, "0F 03 00 00 12 09 44 01");
        real(&mut snapshot, 0x401, "27 12 00 00 03");
        assert_eq!(snapshot.speed, Some(88.0));
        assert_eq!(snapshot.rpm, Some(783.0));
        assert_eq!(snapshot.range_km, Some(82.0));
        assert_eq!(snapshot.throttle, Some(4647.0));

        // Reverse: the motor speed goes negative and bit 2 of 0x280 is set.
        real(&mut snapshot, 0x391, "F3 FF FF FF 49 01 64 01");
        real(&mut snapshot, 0x401, "30 03 00 00 25");
        real(&mut snapshot, 0x280, "24 08 00 00");
        assert_eq!(snapshot.rpm, Some(-13.0));
        assert_eq!(snapshot.drive_flags, Some(0x24));

        // Side stand message, then clear.
        real(&mut snapshot, 0x310, "53 49 C4 45 53 54 C1 4E");
        assert_eq!(snapshot.mark_char.as_deref(), Some("SIDESTAND"));
        real(&mut snapshot, 0x310, "00 00 00 00 00 00 00 00");
        assert_eq!(snapshot.mark_char.as_deref(), Some("Clear"));

        // Pack current on 0x181 bytes 6-7, unit 0.1 A. At 177.5 s it read -90.1 A
        // while 0x189 read -90.18 A, so a draw is negative here too.
        real(&mut snapshot, 0x181, "D8 64 09 09 33 02 7B FC");
        assert!((snapshot.current_summary.unwrap() - -90.1).abs() < 0.001);
        real(&mut snapshot, 0x181, "DA 64 09 09 2F 02 2B 01");
        assert!((snapshot.current_summary.unwrap() - 29.9).abs() < 0.001);

        // Capacity left, unit 0.01 Ah. First and last frames of the capture.
        real(&mut snapshot, 0x197, "EC 26 00 00 EA 1F 64 64");
        assert!((snapshot.capacity_ah.unwrap() - 99.64).abs() < 0.001);
        real(&mut snapshot, 0x197, "E3 26 00 00 E3 1F 64 64");
        assert!((snapshot.capacity_ah.unwrap() - 99.55).abs() < 0.001);

        // 0x431 carries the record another listing calls 0x422, in 1/16 units.
        // This frame came at 221.1 s: 108.19 A, 53.56 V, 98.44 and 93.75 N.m.
        real(&mut snapshot, 0x431, "C3 06 59 03 27 06 DC 05");
        assert!((snapshot.battery_current.unwrap() - 108.1875).abs() < 0.001);
        assert!((snapshot.cap_voltage.unwrap() - 53.5625).abs() < 0.001);
        assert!((snapshot.torque_demand.unwrap() - 98.4375).abs() < 0.001);
        assert!((snapshot.torque_actual.unwrap() - 93.75).abs() < 0.001);
        // And the 0x371 value is the same voltage.
        real(&mut snapshot, 0x371, "F8 FE C6 02 E2 00 59 03");
        assert!((snapshot.cap_voltage.unwrap() - 53.5625).abs() < 0.001);

        // Controller bus voltage, 1/16 V. This frame came at 221.1 s while the
        // pack read 57.2 V; 0x0359 is 857, which is 53.6 V.
        real(&mut snapshot, 0x371, "F8 FE C6 02 E2 00 59 03");
        assert!((snapshot.cap_voltage.unwrap() - 53.5625).abs() < 0.001);
        real(&mut snapshot, 0x371, "F0 FF 24 00 0B 00 93 03");
        assert!((snapshot.cap_voltage.unwrap() - 57.1875).abs() < 0.001);
    }

    #[test]
    fn the_high_bits_of_the_eight_bytes_are_the_ninth_character_of_the_dash_text() {
        let text = |hex: &str| {
            let data: Vec<u8> = hex.split(' ').map(|byte| u8::from_str_radix(byte, 16).unwrap()).collect();
            dash_text(&data)
        };
        // The two messages in recordings, and one from a published report.
        assert_eq!(text("53 49 C4 45 53 54 C1 4E"), "SIDESTAND", "recording A");
        assert_eq!(text("4B F0 EC 75 73 20 CF 46"), "Kplus OFF", "recording B");
        assert_eq!(text("53 C5 C1 D4 20 4F D0 45"), "SEAT OPEN", "published report");
        // Eight plain characters have no ninth.
        assert_eq!(text("43 48 41 52 47 49 4E 47"), "CHARGING");
        // Nothing means clear.
        assert_eq!(text("00 00 00 00 00 00 00 00"), "Clear");
        assert_eq!(text("20 20 20 20 20 20 20 20"), "Clear");
    }

    #[test]
    fn a_short_dash_message_or_an_odd_ninth_character_is_not_invented() {
        let text = |data: &[u8]| dash_text(data);
        // A zero byte ends the text, and the high bits after it do not make a ninth character.
        assert_eq!(text(&[0x43, 0x48, 0x41, 0x52, 0, 0, 0, 0x80]), "CHAR");
        assert_eq!(text(b"CHAR"), "CHAR", "a short frame");
        // High bits that would make a character that cannot be printed are left out.
        assert_eq!(text(&[0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0xC8]), "ABCDEFGH");
        assert_eq!(text(&[0xC1, 0xC1, 0x41, 0x41, 0x41, 0x41, 0x41, 0x41]), "AAAAAAAA", "0x03 is not printable");
        // Bit 7 set on a byte whose low bits are zero ends the text.
        assert_eq!(text(&[0x41, 0x80, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47]), "A");
    }

    #[test]
    fn the_battery_clock_frame_gives_the_time_for_the_header_and_the_date_for_the_card() {
        let mut snapshot = Snapshot::default();
        real(&mut snapshot, 0x200, "03 22 28 04 09 11 26");
        assert_eq!(snapshot.clock.as_deref(), Some("22:28:04"));
        assert_eq!(snapshot.battery_clock.map(|moment| moment.text()).as_deref(), Some("2026-11-09 22:28:04"));
        // The scooter on 9 Nov 2026 before it was set: 18 January 2001.
        real(&mut snapshot, 0x200, "03 11 30 30 18 01 01");
        assert_eq!(snapshot.battery_clock.map(|moment| moment.text()).as_deref(), Some("2001-01-18 11:30:30"));
        // Not marked valid: neither the time nor the date is taken.
        real(&mut snapshot, 0x200, "01 12 00 00 01 01 01");
        assert_eq!(snapshot.battery_clock, None);
    }
}
