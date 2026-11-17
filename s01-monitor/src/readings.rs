use std::sync::OnceLock;

use crate::explain;
use crate::snapshot::Snapshot;

#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub key: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub unit: &'static str,
    pub location: &'static str,
    pub text: &'static str,
}

pub fn rows() -> &'static [Row] {
    static ROWS: OnceLock<Vec<Row>> = OnceLock::new();
    ROWS.get_or_init(build).as_slice()
}

pub fn keys() -> Vec<&'static str> {
    rows().iter().map(|row| row.key).collect()
}

pub fn group_keys(group: &str) -> Vec<&'static str> {
    rows()
        .iter()
        .filter(|row| row.group == group)
        .map(|row| row.key)
        .collect()
}

pub fn figure(snapshot: &Snapshot, row: &Row) -> String {
    raw(snapshot, row.key).unwrap_or_else(|| "—".into())
}

pub fn shown(snapshot: &Snapshot, row: &Row) -> String {
    let figure = figure(snapshot, row);
    if figure == "—" || row.unit.is_empty() {
        figure
    } else {
        format!("{figure} {}", row.unit)
    }
}

/// How the cells sit around their middle value. The middle is the median, so
/// one cell that is far off does not drag the baseline and make its healthy
/// neighbours look wrong.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellSpread {
    /// Median of the cells that are present, in millivolts. With an even
    /// count it is the average of the two middle values.
    pub median: f32,
    /// The highest cell: its number (1 to 14) and its voltage.
    pub high: (usize, f32),
    /// The lowest cell: its number and its voltage.
    pub low: (usize, f32),
}

impl CellSpread {
    /// `None` until at least one cell has been heard. When two cells tie for
    /// highest or lowest, the lower cell number is named.
    pub fn of(cells: &[Option<f32>]) -> Option<Self> {
        let mut present: Vec<(usize, f32)> = cells
            .iter()
            .enumerate()
            .filter_map(|(index, value)| value.map(|value| (index + 1, value)))
            .collect();
        if present.is_empty() {
            return None;
        }
        let mut high = present[0];
        let mut low = present[0];
        for &(number, value) in &present {
            if value > high.1 {
                high = (number, value);
            }
            if value < low.1 {
                low = (number, value);
            }
        }
        present.sort_by(|a, b| a.1.total_cmp(&b.1));
        let middle = present.len() / 2;
        let median = if present.len() % 2 == 1 {
            present[middle].1
        } else {
            (present[middle - 1].1 + present[middle].1) / 2.0
        };
        Some(Self { median, high, low })
    }

    /// Highest minus lowest, in millivolts.
    pub fn delta(&self) -> f32 {
        self.high.1 - self.low.1
    }

    /// How far a voltage is from the median, in millivolts.
    pub fn offset(&self, millivolts: f32) -> f32 {
        millivolts - self.median
    }

    /// For example `+50 mV (cell 3)   -20 mV (cell 2)   median 3986 mV`.
    pub fn summary(&self) -> String {
        format!(
            "{} mV (cell {})   {} mV (cell {})   median {} mV",
            signed_mv(self.offset(self.high.1)),
            self.high.0,
            signed_mv(self.offset(self.low.1)),
            self.low.0,
            mv_text(self.median)
        )
    }
}

/// Whole millivolts, or one decimal when the value is a half (the median of
/// an even number of cells can be).
pub fn mv_text(millivolts: f32) -> String {
    let tenths = (millivolts * 10.0).round() as i64;
    if tenths % 10 == 0 {
        format!("{}", tenths / 10)
    } else {
        format!("{:.1}", tenths as f32 / 10.0)
    }
}

/// `+50`, `-20` or `0`.
pub fn signed_mv(millivolts: f32) -> String {
    let text = mv_text(millivolts);
    if text == "0" || text.starts_with('-') {
        text
    } else {
        format!("+{text}")
    }
}

fn raw(snapshot: &Snapshot, key: &str) -> Option<String> {
    if let Some(index) = key.strip_prefix("cell") {
        let index = index.parse::<usize>().ok()?;
        return snapshot
            .cells
            .get(index)
            .copied()
            .flatten()
            .map(|value| format!("{}", value.round() as i32));
    }
    Some(match key {
        "packVoltage" => tenth(snapshot.voltage)?,
        "soc" => whole(snapshot.soc)?,
        "counter" => snapshot.counter.map(|value| value.to_string())?,
        "tempLow" => snapshot.temp_low.map(|value| value.to_string())?,
        "tempHigh" => snapshot.temp_high.map(|value| value.to_string())?,
        "current" => tenth(snapshot.current)?,
        "currentSummary" => tenth(snapshot.current_summary)?,
        "capacityAh" => hundredth(snapshot.capacity_ah)?,
        "tempA" => tenth(snapshot.temp_a)?,
        "tempB" => tenth(snapshot.temp_b)?,
        "tempC" => tenth(snapshot.temp_c)?,
        "clock" => snapshot.clock.clone()?,
        "socFine" => snapshot.soc_fine.map(|value| value.to_string())?,
        "remain" => snapshot.remain_mah.map(|value| value.to_string())?,
        "vsoc" => snapshot.vsoc.map(|value| value.to_string())?,
        "soh" => snapshot.soh.map(|value| value.to_string())?,
        "maxDischarge" => snapshot.max_discharge.map(|value| value.to_string())?,
        "maxCharge" => snapshot.max_charge.map(|value| value.to_string())?,
        "sinceReset" => snapshot.since_reset.map(|value| value.to_string())?,
        "charged" => snapshot.charged_ws.map(|value| value.to_string())?,
        "discharged" => snapshot.discharged_ws.map(|value| value.to_string())?,
        "regenerated" => snapshot.regenerated_ws.map(|value| value.to_string())?,
        "speed" => whole(snapshot.speed)?,
        "range" => whole(snapshot.range_km)?,
        "powerBar" => snapshot.power_bar.map(|value| value.to_string())?,
        "driveFlags" => snapshot.drive_flags.map(|value| format!("{value:02X}"))?,
        "driveMode" => snapshot.drive_flags.map(|value| {
            match (value >> 4) & 0x03 {
                0 => "Off",
                1 => "ECO",
                2 => "SPORT",
                _ => "CITY",
            }
            .to_string()
        })?,
        "sideStand" => snapshot
            .drive_flags
            .map(|value| if value & 0x08 != 0 { "Down" } else { "Up" }.to_string())?,
        "driveReady" => snapshot
            .drive_flags
            .map(|value| if value & 0x01 != 0 { "Yes" } else { "No" }.to_string())?,
        "outside" => snapshot.outside.map(|value| value.to_string())?,
        "odometer" => snapshot.odometer.map(|value| value.to_string())?,
        "throttle" => whole(snapshot.throttle)?,
        "footbrake" => snapshot
            .footbrake
            .map(|on| if on { "On" } else { "Off" }.into())?,
        "sinceIgnition" => snapshot.since_ignition.map(|value| value.to_string())?,
        "controller" => snapshot.controller.map(|value| value.to_string())?,
        "markChar" => snapshot.mark_char.clone()?,
        "markStan" => snapshot.mark_stan.clone()?,
        "rpm" => whole(snapshot.rpm)?,
        "motor" => tenth(snapshot.motor)?,
        "inverter" => tenth(snapshot.inverter)?,
        "tempThird" => tenth(snapshot.temp_third)?,
        "heatsink" => whole(snapshot.heatsink)?,
        "motorCtrl" => tenth(snapshot.motor_ctrl)?,
        "capVoltage" => tenth(snapshot.cap_voltage)?,
        "batteryCurrent" => tenth(snapshot.battery_current)?,
        "torqueDemand" => tenth(snapshot.torque_demand)?,
        "torqueActual" => tenth(snapshot.torque_actual)?,
        _ => return None,
    })
}

fn whole(value: Option<f32>) -> Option<String> {
    value.map(|number| format!("{}", number.round() as i32))
}

fn hundredth(value: Option<f32>) -> Option<String> {
    value.map(|number| format!("{number:.2}"))
}

fn tenth(value: Option<f32>) -> Option<String> {
    value.map(|number| format!("{number:.1}"))
}

fn row(
    key: &'static str,
    group: &'static str,
    label: &'static str,
    unit: &'static str,
    location: &'static str,
    text: &'static str,
) -> Row {
    Row {
        key,
        group,
        label,
        unit,
        location,
        text,
    }
}

fn build() -> Vec<Row> {
    let mut rows = vec![
        row("packVoltage", "Pack", "Pack voltage", "V", "0x181 bytes 4–5", "Pack voltage, unit 0.1 V. Shown in volts."),
        row("soc", "Pack", "State of charge", "%", "0x181 byte 1", "State of charge, percent."),
        row("counter", "Pack", "Pack counter", "", "0x181 byte 0", "A counter on the pack summary."),
        row("tempLow", "Pack", "Lowest pack temperature", "°C", "0x181 byte 2", "Lowest temperature on the pack summary, in whole degrees."),
        row("tempHigh", "Pack", "Highest pack temperature", "°C", "0x181 byte 3", "Highest temperature on the pack summary, in whole degrees."),
        row("current", "Pack", "Current", "A", "0x189 bytes 0–1", "Pack current, unit 10 mA. Shown in amperes. Negative is discharge, current drawn from the pack. Positive is charge or regeneration."),
        row("currentSummary", "Pack", "Current (pack summary)", "A", "0x181 bytes 6–7", "Pack current from the pack summary, unit 0.1 A. It agrees with the Current reading and has the same sign. Negative is discharge."),
        row("tempA", "Pack", "Temperature A", "°C", "0x189 bytes 2–3", "Probe temperature, unit 0.01 °C."),
        row("tempB", "Pack", "Temperature B", "°C", "0x189 bytes 4–5", "Probe temperature, unit 0.01 °C."),
        row("tempC", "Pack", "Temperature C", "°C", "0x189 bytes 6–7", "Probe temperature, unit 0.01 °C."),
        row("clock", "Pack", "Clock", "", "0x200 bytes 1–6", "This is the battery clock. It counts only when the clock is marked valid."),
        row("capacityAh", "Pack", "Capacity left", "Ah", "0x197 bytes 0–1", "Capacity left in the pack, unit 0.01 Ah. It fell in step with the energy drawn in the capture. It has not been checked against a second source."),
        row("socFine", "Pack", "Finer state of charge", "1/1000", "0x190 byte 1", "This is a finer state of charge."),
        row("remain", "Pack", "Remaining capacity", "mAh", "0x190 bytes 2–3", "Remaining capacity."),
        row("vsoc", "Pack", "VSOC", "", "0x190 byte 4", "VSOC."),
        row("soh", "Pack", "State of health", "", "0x190 byte 5", "State of health."),
        row("maxDischarge", "Pack", "Maximum discharge current", "A", "0x1AA", "The maximum current the battery will allow for discharge."),
        row("maxCharge", "Pack", "Maximum charge current", "A", "0x1AA", "The maximum current the battery will allow for charge."),
        row("sinceReset", "Pack", "Time since battery restart", "s", "0x1AC bytes 0–3", "Time since the battery manager restarted, in seconds."),
        row("charged", "Pack", "Energy charged", "W·s", "0x1C0 bytes 0–3", "Energy charged since the scooter was switched on, in watt-seconds."),
        row("discharged", "Pack", "Energy discharged", "W·s", "0x1C0 bytes 4–7", "Energy discharged since the scooter was switched on, in watt-seconds."),
        row("regenerated", "Pack", "Energy regenerated", "W·s", "0x1C1 bytes 0–3", "Energy regenerated since the scooter was switched on, in watt-seconds."),
    ];
    for index in 0..14 {
        let key: &'static str = match index {
            0 => "cell0",
            1 => "cell1",
            2 => "cell2",
            3 => "cell3",
            4 => "cell4",
            5 => "cell5",
            6 => "cell6",
            7 => "cell7",
            8 => "cell8",
            9 => "cell9",
            10 => "cell10",
            11 => "cell11",
            12 => "cell12",
            _ => "cell13",
        };
        let label: &'static str = match index {
            0 => "Cell 1",
            1 => "Cell 2",
            2 => "Cell 3",
            3 => "Cell 4",
            4 => "Cell 5",
            5 => "Cell 6",
            6 => "Cell 7",
            7 => "Cell 8",
            8 => "Cell 9",
            9 => "Cell 10",
            10 => "Cell 11",
            11 => "Cell 12",
            12 => "Cell 13",
            _ => "Cell 14",
        };
        rows.push(row(
            key,
            "Cells",
            label,
            "mV",
            explain::cell_frame(index),
            "Cell voltage in millivolts.",
        ));
    }
    rows.extend([
        row("speed", "Scooter", "Speed", "km/h", "0x300 byte 0", "Road speed."),
        row("range", "Scooter", "Remaining range", "km", "0x300 byte 1", "Remaining range. This reading has not been checked against the scooter's own display."),
        row("powerBar", "Scooter", "Power bar", "", "0x300 byte 4", "Power bar."),
        row("driveMode", "Scooter", "Drive mode", "", "0x280 byte 0, bits 4–5", "The drive mode: ECO, CITY, or SPORT. Off when the key is off."),
        row("sideStand", "Scooter", "Side stand", "", "0x280 byte 0, bit 3", "Whether the side stand is down."),
        row("driveReady", "Scooter", "Ready to drive", "", "0x280 byte 0, bit 0", "Whether the scooter is ready to drive."),
        row("driveFlags", "Scooter", "Drive switches (hex)", "", "0x280 byte 0", "The drive switches as one hex number. The mode, side stand, and ready bits are also shown on their own."),
        row("outside", "Scooter", "Outside temperature", "°C", "0x500 byte 0", "Outside temperature."),
        row("odometer", "Scooter", "Odometer", "km", "0x500 bytes 4–6", "Odometer."),
        row("throttle", "Scooter", "Throttle input", "", "0x401 bytes 0–1", "Throttle input, 16-bit. The unit is not known."),
        row("footbrake", "Scooter", "Footbrake", "", "0x421 byte 0", "Footbrake switch."),
        row("sinceIgnition", "Scooter", "Running timer", "ms", "0x297 bytes 0–3", "A timer that counts up in milliseconds. It kept counting with the key off, so it is not the time since the ignition. What it counts from is not known."),
        row("controller", "Scooter", "Controller state", "", "0x701 byte 0", "Whether the motor controller is in operating mode or still starting."),
        row("markChar", "Scooter", "Display message", "", "0x310", "The text sent to the dash, up to nine characters, for messages such as the side stand being down. It is empty when no message is showing. That is a guess as to the purpose."),
        row("markStan", "Scooter", "STAN", "", "0x313", "This message carries the characters STAN. What they stand for is not known."),
        row("rpm", "Motor", "Motor speed", "rpm", "0x391 bytes 0–3", "Motor speed. A negative figure appeared while the scooter was in reverse."),
        row("motor", "Motor", "Motor temperature", "°C", "0x301 bytes 0–1", "Motor temperature, unit 0.1 °C."),
        row("inverter", "Motor", "Inverter temperature", "°C", "0x301 bytes 2–3", "Inverter temperature, unit 0.1 °C."),
        row("tempThird", "Motor", "Third temperature", "°C", "0x301 bytes 4–5", "A third temperature, unit 0.1 °C."),
        row("heatsink", "Motor", "Heatsink", "°C", "0x411 byte 4", "Heatsink temperature."),
        row("motorCtrl", "Motor", "Controller motor temperature", "°C", "0x411 byte 6", "Motor temperature reported by the controller."),
        row("capVoltage", "Motor", "Controller bus voltage", "V", "0x371 bytes 6–7, 0x431 bytes 2–3", "The voltage inside the motor controller, unit 1/16 V. It follows the pack voltage and moves under load. It has been checked in one capture only, not against a second source."),
        row("batteryCurrent", "Motor", "Battery current (controller)", "A", "0x431 bytes 0–1", "Battery current as the motor controller reports it, unit 1/16 A. Positive is current drawn from the pack, the opposite sign to the Current reading. It follows the pack current but peaks about 10% higher. It has not been checked against a second source."),
        row("torqueDemand", "Motor", "Torque asked for", "N·m", "0x431 bytes 4–5", "Torque asked for, unit 1/16 N·m. It follows the throttle. The unit has not been checked."),
        row("torqueActual", "Motor", "Torque delivered", "N·m", "0x431 bytes 6–7", "Torque delivered, unit 1/16 N·m. It follows the throttle. The unit has not been checked."),
    ]);
    rows
}

/// A saved name is kept when it is non-empty, unused, and at least one row is on.
pub fn can_save(name: &str, used: &[String], selected: usize) -> bool {
    let name = name.trim();
    !name.is_empty() && selected > 0 && !used.iter().any(|item| item == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(values: &[f32]) -> Vec<Option<f32>> {
        values.iter().map(|value| Some(*value)).collect()
    }

    #[test]
    fn the_median_ignores_one_far_off_cell_where_a_mean_would_not() {
        // Thirteen cells at 3986 and one at 3900: the mean is pulled to
        // about 3980, the median stays put.
        let mut values = vec![3986.0; 14];
        values[1] = 3900.0;
        let spread = CellSpread::of(&cells(&values)).unwrap();
        assert_eq!(spread.median, 3986.0);
        assert_eq!(spread.low, (2, 3900.0));
        assert_eq!(spread.offset(3986.0), 0.0);
        assert_eq!(spread.offset(3900.0), -86.0);
    }

    #[test]
    fn the_median_of_an_even_count_is_the_middle_pair_averaged() {
        let spread = CellSpread::of(&cells(&[3980.0, 3984.0, 3990.0, 4000.0])).unwrap();
        assert_eq!(spread.median, 3987.0);
        let spread = CellSpread::of(&cells(&[3980.0, 3985.0])).unwrap();
        assert_eq!(spread.median, 3982.5);
        assert_eq!(mv_text(spread.median), "3982.5");
    }

    #[test]
    fn the_median_of_an_odd_count_is_the_middle_value() {
        let spread = CellSpread::of(&cells(&[4000.0, 3980.0, 3990.0])).unwrap();
        assert_eq!(spread.median, 3990.0);
        let one = CellSpread::of(&[None, Some(3977.0), None]).unwrap();
        assert_eq!(one.median, 3977.0);
        assert_eq!(one.high, (2, 3977.0));
        assert_eq!(one.low, (2, 3977.0));
    }

    #[test]
    fn cells_not_heard_yet_are_left_out_and_numbers_count_from_one() {
        let mut values: Vec<Option<f32>> = vec![None; 14];
        values[2] = Some(4036.0);
        values[5] = Some(3986.0);
        values[9] = Some(3966.0);
        let spread = CellSpread::of(&values).unwrap();
        assert_eq!(spread.median, 3986.0);
        assert_eq!(spread.high, (3, 4036.0));
        assert_eq!(spread.low, (10, 3966.0));
        assert_eq!(spread.delta(), 70.0);
        assert!(CellSpread::of(&[None; 14]).is_none());
        assert!(CellSpread::of(&[]).is_none());
    }

    #[test]
    fn a_tie_names_the_lower_cell_number() {
        let spread = CellSpread::of(&cells(&[3990.0, 3970.0, 3990.0, 3970.0, 3980.0])).unwrap();
        assert_eq!(spread.high.0, 1);
        assert_eq!(spread.low.0, 2);
    }

    #[test]
    fn the_summary_reads_like_the_users_example() {
        let mut values = vec![3986.0; 14];
        values[2] = 4036.0;
        values[1] = 3966.0;
        let spread = CellSpread::of(&cells(&values)).unwrap();
        assert_eq!(spread.summary(), "+50 mV (cell 3)   -20 mV (cell 2)   median 3986 mV");
        let flat = CellSpread::of(&cells(&[3986.0; 14])).unwrap();
        assert_eq!(flat.summary(), "0 mV (cell 1)   0 mV (cell 1)   median 3986 mV");
        assert_eq!(signed_mv(2.5), "+2.5");
        assert_eq!(signed_mv(-0.4), "-0.4");
    }

    #[test]
    fn a_view_name_needs_a_selection_and_a_new_name() {
        let used = vec!["All".into(), "Pack".into()];
        assert!(!can_save("  ", &used, 2));
        assert!(!can_save("Pack", &used, 2));
        assert!(!can_save("Morning", &used, 0));
        assert!(can_save(" Morning ", &used, 2));
    }

    #[test]
    fn drive_message_bits_read_as_mode_stand_and_ready() {
        let show = |flags: u8, key: &str| {
            let mut snapshot = Snapshot::default();
            snapshot.drive_flags = Some(flags);
            let row = rows().iter().find(|row| row.key == key).unwrap();
            figure(&snapshot, row)
        };
        // 21 is the steady value from the capture; 28 is the side stand down.
        assert_eq!(show(0x21, "driveMode"), "SPORT");
        assert_eq!(show(0x21, "sideStand"), "Up");
        assert_eq!(show(0x21, "driveReady"), "Yes");
        assert_eq!(show(0x28, "driveMode"), "SPORT");
        assert_eq!(show(0x28, "sideStand"), "Down");
        assert_eq!(show(0x28, "driveReady"), "No");
        assert_eq!(show(0x24, "sideStand"), "Up");
        assert_eq!(show(0x11, "driveMode"), "ECO");
        assert_eq!(show(0x31, "driveMode"), "CITY");
        assert_eq!(show(0x00, "driveMode"), "Off");
        assert_eq!(show(0x21, "driveFlags"), "21");
    }
}
