use std::sync::OnceLock;

use crate::snapshot::Snapshot;

#[derive(Clone, Copy, Debug)]
pub struct FlagDef {
    pub name: &'static str,
    pub id: u16,
    pub byte: usize,
    pub bit: u8,
    pub group: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Lit,
    Warn,
    Alarm,
}

pub fn catalogue() -> &'static [FlagDef] {
    static FLAGS: OnceLock<Vec<FlagDef>> = OnceLock::new();
    FLAGS.get_or_init(build_catalogue).as_slice()
}

pub fn is_set(snapshot: &Snapshot, flag: &FlagDef) -> bool {
    let Some(bytes) = snapshot.bytes_for(flag.id) else {
        return false;
    };
    let Some(byte) = bytes.data.get(flag.byte).copied() else {
        return false;
    };
    if flag.byte >= bytes.len {
        return false;
    }
    let mask = 1u8 << flag.bit;
    (byte & mask) == mask
}

pub fn set_flags(snapshot: &Snapshot) -> Vec<&'static FlagDef> {
    catalogue()
        .iter()
        .filter(|flag| is_set(snapshot, flag))
        .collect()
}

pub fn tone(name: &str) -> Tone {
    const ALARM: &[&str] = &[
        "Alarm",
        "SCD",
        "Kmain error",
        "Thermal Event",
        "HiGs Impact",
        "Kopen",
    ];
    const WARN: &[&str] = &[
        "WOV", "WUV", "WOCD", "WOCR", "WOCC", "WOT", "WUT", "WLS", "LOV", "LUV", "LOCD", "LOCR",
        "LOCC", "LOT", "LUT",
    ];
    if ALARM.contains(&name) {
        Tone::Alarm
    } else if WARN.contains(&name) {
        Tone::Warn
    } else {
        Tone::Lit
    }
}

/// The meaning of a flag by name alone. Only the tests ask for it this way; the
/// app always has the flag's frame as well (`flag_detail`).
#[cfg(test)]
pub fn flag_text(name: &str) -> String {
    flag_meaning(name, 0)
}

pub fn flag_detail(flag: &FlagDef) -> String {
    flag_meaning(flag.name, flag.id)
}

fn flag_meaning(name: &str, id: u16) -> String {
    const NO_DEFINITION: &[&str] = &["ILDSCH", "VKL", "OFD", "UVPD"];
    if NO_DEFINITION.contains(&name) {
        return "No written definition.".into();
    }
    const CODES: &[(&str, &str)] = &[
        ("OV", "Error Over Voltage"),
        ("UV", "Error Under Voltage"),
        ("OCD", "Error Over Current Discharge"),
        ("OCR", "Error Over Current Regenerative"),
        ("OCC", "Error Over Current Charge"),
        ("OT", "Error Over Temperature"),
        ("UT", "Error Under Temperature"),
    ];
    if let Some(rest) = name.strip_prefix('W').or_else(|| name.strip_prefix('L')) {
        if let Some((_, event)) = CODES.iter().find(|(code, _)| *code == rest) {
            let group = if name.starts_with('W') {
                "Warnings"
            } else {
                "Limit"
            };
            return format!("{group} group. Event log line: {event}.");
        }
    }
    const MEANINGS: &[(&str, &str)] = &[
        ("ON", "The pack is switched on."),
        ("DSCH", "The pack is in a discharge state. This is not a permission to discharge."),
        ("CHGi", "Charging from the internal charger."),
        ("CHGe", "Charging from the external charger."),
        ("CHGD", "The charge has finished."),
        ("WarnMode", "A warning bit in byte 2 is set."),
        ("LEDr", "The pack light ring is being driven."),
        ("Alarm", "An alarm is latched."),
        ("CC", "The charge is in its constant-current phase."),
        ("CV", "The charge is in its constant-voltage phase."),
        ("Bal", "Balancing is underway. The per-cell bits are on 0x18E."),
        ("Key_In", "The ignition input is closed."),
        ("Chg_ext", "An external charger is connected."),
        ("Chg_ob", "The onboard charger is connected."),
        ("PiezoTap", "The tap on the front of the pack that wakes the light ring."),
        ("Accel", "An accelerometer input, separate from HiGs Impact."),
        ("RTCC", "The real-time clock is running."),
        ("Kch_ob", "The onboard charger relay."),
        ("Unfiltered data", "The reported values are not smoothed."),
        ("GOD Mode", "A battery engineering override, separate from the ECU command."),
        ("Fast Acq", "The pack is sampling faster."),
        ("RAW data", "The reported values are not smoothed."),
        ("Debug", "A battery engineering debug mode, separate from the ECU debug command."),
        ("Developer", "An engineering developer mode."),
        ("WLS", "A low state-of-charge warning."),
        ("SCD", "Short-circuit detection."),
        (
            "INH",
            "An error condition, caused by faults such as overtemperature or a hard shock. While it is set, the high-current output (K_plus) must not be switched on and charging is blocked.",
        ),
        ("K_plus", "The high-current contactor is closed."),
        ("PMOS_E", "The electronic switch for the scooter electrics, VBatE."),
        ("PMOS_I", "The electronic switch for the motor-controller supply, VBatI."),
        ("Heater", "The pack heater output."),
        ("Kmain error", "The main contactor reported a fault, separate from K_plus."),
        ("HiGs Impact", "A shock that opens the high-current output."),
        ("Thermal Event", "The temperature event."),
        ("Cell 01", "Cell 1 is balancing."),
        ("Cell 02", "Cell 2 is balancing."),
        ("Cell 03", "Cell 3 is balancing."),
        ("Cell 04", "Cell 4 is balancing."),
        ("Cell 05", "Cell 5 is balancing."),
        ("Cell 06", "Cell 6 is balancing."),
        ("Cell 07", "Cell 7 is balancing."),
        ("Cell 08", "Cell 8 is balancing."),
        ("Cell 09", "Cell 9 is balancing."),
        ("Cell 10", "Cell 10 is balancing."),
        ("Cell 11", "Cell 11 is balancing."),
        ("Cell 12", "Cell 12 is balancing."),
        ("Cell 13", "Cell 13 is balancing."),
        ("Cell 14", "Cell 14 is balancing."),
        ("Enabled", "Balancing is enabled."),
        ("Allowed", "Balancing is allowed."),
        ("Inhibited", "Balancing is inhibited."),
        ("AUTO", "Balancing is automatic."),
        ("Imbalance", "Balancing is held for imbalance."),
        ("High Temp", "Balancing is held for a high temperature."),
        ("Over Temp", "Balancing is held for an over temperature."),
        ("Brake L", "The left brake."),
        ("Brake R", "The right brake."),
        ("Blink L", "The left indicator."),
        ("Blink R", "The right indicator."),
        ("Normal beam", "The normal beam."),
        ("High beam", "The high beam."),
        ("Normalbeam", "The normal beam."),
        ("Highbeam", "The high beam."),
        ("Side stand", "The side stand."),
        ("Main stand", "The main stand."),
        ("Seat act", "The seat switch."),
        ("Seat actuator", "The seat actuator."),
        ("Horn", "The horn is on."),
        ("Helmet 1", "Helmet 1 is on."),
        ("Helmet 2", "Helmet 2 is on."),
        ("Battery block", "The battery block is on."),
        ("Position", "Position is on."),
        ("White L", "White L is on."),
        ("White R", "White R is on."),
        ("Buzzer", "The buzzer is on."),
        ("Dash", "The dash is on."),
        ("Steering Lock", "The steering lock is on."),
        ("Fans", "The fans are on."),
        ("Forward", "Drive direction is forward."),
        ("Reverse", "Drive direction is reverse."),
        ("FS1", "A switch input on the motor controller. It came on with both forward and reverse."),
        ("DM1", "One of the motor controller's drive-mode bits. This is a guess as to the purpose: it selects a drive profile."),
        ("DM2", "One of the motor controller's drive-mode bits. This is a guess as to the purpose: it selects a drive profile."),
        ("DM3", "One of the motor controller's drive-mode bits. This is a guess as to the purpose: it selects a drive profile. DM3 was on whenever reverse was selected."),
        ("God Mode", "ECU god mode, separate from the battery GOD Mode flag."),
        ("Sending on Debug Mode", "ECU debug mode."),
        ("BMS Timeout", "A timeout talking to the battery."),
        ("Sevcon Timeout", "A timeout from the Sevcon."),
        ("UnderVoltage PCB", "The ECU board supply is under voltage."),
        ("Normal Voltage", "The ECU board supply is in its normal range."),
        ("OverVoltage PCB", "The ECU board supply is over voltage."),
        ("Mode Switch is not pressed", "Mode Switch is not pressed"),
        ("ReadyToDrive is \"0\"", "ReadyToDrive is \"0\""),
        ("Throttle is not released", "Throttle is not released"),
        ("Alarm is not Fired", "Alarm is not Fired"),
        ("uC running time is over min delay", "uC running time is over min delay"),
        ("Motorbike is not in Charging mode", "Motorbike is not in Charging mode"),
        ("Motorbike is not in Charged mode", "Motorbike is not in Charged mode"),
        ("Motorbike is not in Warnings mode", "Motorbike is not in Warnings mode"),
        ("PMOS I is active", "PMOS I is active"),
        ("There is no timeout from MCU", "There is no timeout from MCU"),
        ("Battery sensor detects battery", "Battery sensor detects battery"),
        ("Brake Left is pressed", "Brake Left is pressed"),
    ];
    if let Some((_, text)) = MEANINGS.iter().find(|(flag, _)| *flag == name) {
        if name == "Reverse" && id != 0x401 {
            return "The source only gives this name.".into();
        }
        return (*text).into();
    }
    "The source only gives this name.".into()
}

pub fn where_text(flag: &FlagDef) -> String {
    if flag.name == "HiGs Impact" {
        return "0x184 byte 7 bit 3, and 0x191 byte 0 bit 3".into();
    }
    format!("{:#X} byte {} bit {}", flag.id, flag.byte, flag.bit)
}

pub fn cell_frame(index: usize) -> &'static str {
    match index {
        0..=3 => "0x185",
        4..=7 => "0x186",
        8..=11 => "0x187",
        _ => "0x188",
    }
}

fn build_catalogue() -> Vec<FlagDef> {
    let mut flags = Vec::new();
    push(
        &mut flags,
        0x184,
        0,
        "BMScutum",
        &[
            "ON", "DSCH", "CHGi", "CHGe", "CHGD", "WarnMode", "LEDr", "Alarm",
        ],
    );
    push(
        &mut flags,
        0x184,
        1,
        "BMScutum",
        &["CC", "CV", "Bal", "VKL", "OFD", "INH", "UVPD", "ILDSCH"],
    );
    push(
        &mut flags,
        0x184,
        2,
        "Warnings",
        &["WOV", "WUV", "WOCD", "WOCR", "WOCC", "WOT", "WUT", "WLS"],
    );
    push(
        &mut flags,
        0x184,
        3,
        "Limit",
        &["LOV", "LUV", "LOCD", "LOCR", "LOCC", "LOT", "LUT", "SCD"],
    );
    push(
        &mut flags,
        0x184,
        4,
        "Inputs",
        &["Key_In", "Chg_ext", "Chg_ob", "PiezoTap", "Accel", "RTCC"],
    );
    push(
        &mut flags,
        0x184,
        5,
        "Outputs",
        &[
            "K_plus", "PMOS_E", "PMOS_I", "Kch_ob", "Heater", "Fan_A", "Fan_B",
        ],
    );
    push(
        &mut flags,
        0x184,
        6,
        "Misc",
        &[
            "Unfiltered data",
            "GOD Mode",
            "Fast Acq",
            "RAW data",
            "Debug",
            "Developer",
        ],
    );
    flags.push(flag("Kmain error", 0x184, 7, 2, "Misc"));
    flags.push(flag("HiGs Impact", 0x184, 7, 3, "Misc"));
    flags.push(flag("Thermal Event", 0x184, 7, 4, "Misc"));
    push(
        &mut flags,
        0x18E,
        0,
        "Cell Balancing",
        &[
            "Cell 01", "Cell 02", "Cell 03", "Cell 04", "Cell 05", "Cell 06", "Cell 07", "Cell 08",
        ],
    );
    push(
        &mut flags,
        0x18E,
        1,
        "Cell Balancing",
        &[
            "Cell 09", "Cell 10", "Cell 11", "Cell 12", "Cell 13", "Cell 14",
        ],
    );
    push(
        &mut flags,
        0x18E,
        2,
        "Balance",
        &[
            "Enabled",
            "Allowed",
            "Inhibited",
            "AUTO",
            "Imbalance",
            "Viddle",
            "High Temp",
            "Over Temp",
        ],
    );
    flags.push(flag("Kopen", 0x191, 0, 2, "Inhibick"));
    flags.push(flag("HiGs Impact", 0x191, 0, 3, "Inhibick"));

    push(
        &mut flags,
        0x294,
        0,
        "ECU Inputs",
        &[
            "Brake L",
            "Normal beam",
            "Warnings",
            "Brake R",
            "Blink L",
            "High beam",
            "Blink R",
            "Reverse",
        ],
    );
    push(
        &mut flags,
        0x294,
        1,
        "ECU Inputs",
        &[
            "Side stand",
            "Mode",
            "Horn",
            "Seat act",
            "Battery block",
            "Main stand",
            "Helmet 1",
            "Helmet 2",
        ],
    );
    push(
        &mut flags,
        0x294,
        2,
        "ECU Outputs",
        &[
            "Blink L",
            "Blink R",
            "Highbeam",
            "Position",
            "Normalbeam",
            "Brake L",
            "Brake R",
            "Horn",
        ],
    );
    push(
        &mut flags,
        0x294,
        3,
        "ECU Outputs",
        &[
            "Seat actuator",
            "Fans",
            "Battery block",
            // An earlier listing calls bit 3 White L and bit 4 White R. A recording showed the opposite: bit 4 goes out with
            // the left indicator and bit 3 with the right, and the lamp next
            // to the flashing indicator is the one that goes out.
            "White R",
            "White L",
            "Buzzer",
            "Dash",
            "Steering Lock",
        ],
    );
    push(
        &mut flags,
        0x401,
        4,
        "Virtual Inputs Sevcon",
        &["FS1", "Forward", "Reverse", "DM1", "DM2", "DM3"],
    );
    flags.push(flag("God Mode", 0x28A, 0, 0, "ECU status"));
    flags.push(flag("Sending on Debug Mode", 0x28A, 3, 0, "ECU status"));
    flags.push(flag("BMS Timeout", 0x28A, 3, 1, "ECU status"));
    flags.push(flag("Sevcon Timeout", 0x28A, 3, 2, "ECU status"));
    push(
        &mut flags,
        0x28A,
        4,
        "ECU status",
        &[
            "UnderVoltage PCB",
            "BTSoff",
            "BTSon",
            "Normal Voltage",
            "OverVoltage PCB",
        ],
    );
    push(
        &mut flags,
        0x293,
        6,
        "Ready Conditions",
        &[
            "Mode Switch is not pressed",
            "ReadyToDrive is \"0\"",
            "Throttle is not released",
            "Alarm is not Fired",
            "uC running time is over min delay",
            "Motorbike is not in Charging mode",
            "Motorbike is not in Charged mode",
            "Motorbike is not in Warnings mode",
        ],
    );
    push(
        &mut flags,
        0x293,
        7,
        "Ready Conditions",
        &[
            "PMOS I is active",
            "There is no timeout from MCU",
            "Battery sensor detects battery",
            "Brake Left is pressed",
        ],
    );
    flags
}

fn flag(name: &'static str, id: u16, byte: usize, bit: u8, group: &'static str) -> FlagDef {
    FlagDef {
        name,
        id,
        byte,
        bit,
        group,
    }
}

fn push(out: &mut Vec<FlagDef>, id: u16, byte: usize, group: &'static str, names: &[&'static str]) {
    for (bit, name) in names.iter().enumerate() {
        out.push(flag(name, id, byte, bit as u8, group));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode;
    use crate::snapshot::Frame;

    #[test]
    fn captured_status_lights_the_sample_flags() {
        let mut snapshot = Snapshot::default();
        decode::apply(
            &mut snapshot,
            &Frame::new(0x184, &[0x45, 0x82, 0x01, 0x00, 0x04, 0x0A, 0x00, 0x00]),
        );
        decode::apply(&mut snapshot, &Frame::new(0x18E, &[0; 8]));
        decode::apply(&mut snapshot, &Frame::new(0x191, &[0; 8]));
        let names: Vec<_> = set_flags(&snapshot).iter().map(|flag| flag.name).collect();
        assert_eq!(
            names,
            vec!["ON", "CHGi", "LEDr", "CV", "ILDSCH", "WOV", "Chg_ob", "PMOS_E", "Kch_ob"]
        );
    }

    #[test]
    fn the_two_white_lamps_are_named_the_way_round_the_scooter_shows_them() {
        // A recording on 13 Nov 2026: with the left indicator on, byte 3 bit 4
        // of 0x294 went out; with the right indicator on, bit 3 went out.
        let at = |bit: u8| catalogue().iter().find(|flag| flag.id == 0x294 && flag.byte == 3 && flag.bit == bit).unwrap();
        assert_eq!(at(3).name, "White R");
        assert_eq!(at(4).name, "White L");
        // Everything else in the outputs is where the earlier listing puts it.
        assert_eq!(at(0).name, "Seat actuator");
        assert_eq!(at(5).name, "Buzzer");
        assert_eq!(at(7).name, "Steering Lock");
    }

    #[test]
    fn warning_text_uses_the_event_log_and_named_meanings() {
        assert_eq!(
            flag_text("WOV"),
            "Warnings group. Event log line: Error Over Voltage."
        );
        assert_eq!(
            flag_text("LOCD"),
            "Limit group. Event log line: Error Over Current Discharge."
        );
        assert_eq!(flag_text("WLS"), "A low state-of-charge warning.");
        assert_eq!(flag_text("SCD"), "Short-circuit detection.");
        assert_eq!(flag_text("ON"), "The pack is switched on.");
        assert_eq!(flag_text("ILDSCH"), "No written definition.");
        assert!(flag_text("INH").starts_with("An error condition"), "{}", flag_text("INH"));
        assert!(flag_text("INH").contains("charging is blocked"));
        assert_eq!(flag_text("VKL"), "No written definition.");
        assert_eq!(flag_text("Cell 01"), "Cell 1 is balancing.");
        assert_eq!(flag_text("Enabled"), "Balancing is enabled.");
        assert_eq!(flag_text("Viddle"), "The source only gives this name.");
        assert_eq!(flag_text("Fan_A"), "The source only gives this name.");
        assert_eq!(
            flag_detail(&flag("Reverse", 0x401, 4, 2, "Virtual Inputs Sevcon")),
            "Drive direction is reverse."
        );
        assert_eq!(
            flag_detail(&flag("Reverse", 0x294, 0, 7, "ECU Inputs")),
            "The source only gives this name."
        );
    }

    #[test]
    fn a_short_frame_does_not_set_a_later_byte() {
        let mut snapshot = Snapshot::default();
        decode::apply(&mut snapshot, &Frame::new(0x184, &[0xFF]));
        assert!(set_flags(&snapshot).iter().all(|flag| flag.byte == 0));
    }
}
