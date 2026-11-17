#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Int,
    Float,
    Hex,
    Bin,
    Ascii,
}

#[derive(Clone, Copy, Debug)]
pub struct Wire {
    pub index: u8,
    pub first: u8,
    pub last: u8,
    pub kind: Kind,
    pub scale: i8,
    pub signed: bool,
    pub slices: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub name: &'static str,
    pub unit: &'static str,
    pub wire: Option<Wire>,
}

pub struct Section {
    pub title: &'static str,
    pub rows: &'static [Row],
}

const fn row(
    name: &'static str,
    unit: &'static str,
    index: u8,
    first: u8,
    last: u8,
    kind: Kind,
    scale: i8,
    signed: bool,
    slices: u8,
) -> Row {
    Row {
        name,
        unit,
        wire: Some(Wire { index, first, last, kind, scale, signed, slices }),
    }
}

const fn plain(name: &'static str, unit: &'static str) -> Row {
    Row { name, unit, wire: None }
}

pub fn battery() -> &'static [Section] {
    BATTERY
}

pub fn ecu() -> &'static [Section] {
    ECU
}

pub fn len(sections: &[Section]) -> usize {
    sections.iter().map(|section| section.rows.len()).sum()
}

pub fn sample_number(units: &str, index: usize) -> String {
    if units.is_empty() {
        return format!("{:X}", index % 7 + 1);
    }
    let base = match units {
        "Ah" => 26.0,
        "A" => 40.0,
        "mA" => 120.0,
        "mV" => 4150.0,
        "V" => 1.2,
        "degC" => 45.0,
        "ºC" => 70.0,
        "s" => 8.0,
        "min" => 15.0,
        "%" => 12.0,
        "A/mVs" => 0.02,
        "A/s" => 5.0,
        "degC/min" => 1.5,
        "s/%" => 4.0,
        "uOhm" => 250.0,
        "Ohm" => 10000.0,
        "h" => 6.0,
        "g₀" => 8.0,
        "ms" => 40.0,
        "m" => 1.85,
        _ => 1.0,
    };
    let wobble = ((index * 17) % 9) as f64 - 4.0;
    let value = base + wobble * if base > 20.0 { 1.0 } else { 0.1 };
    if units == "A/mVs" {
        format!("{value:.3}")
    } else {
        let rounded = (value * 10.0).round() / 10.0;
        if (rounded - rounded.round()).abs() < 0.05 {
            format!("{}", rounded.round() as i32)
        } else {
            format!("{rounded:.1}")
        }
    }
}

pub fn fill(sections: &[Section]) -> Vec<String> {
    let mut index = 0;
    let mut values = Vec::new();
    for section in sections {
        for row in section.rows {
            values.push(sample_number(row.unit, index));
            index += 1;
        }
    }
    values
}

pub fn event_text(code: &str) -> &'static str {
    match code {
        "1010" => "Off",
        "1021" => "Periodic wake, AFE refresh reading",
        "1022" => "Key on",
        "1023" => "On from the internal charger",
        "1024" => "On from the external charger",
        "10A0" => "System start, reason for reset",
        "1100" => "Outputs Forced Disable",
        "1110" => "Keyless Enabled",
        "1120" => "Keyless Disabled",
        "1121" => "Keyless Disabled Timeout",
        "1200" => "Shutdown Event",
        "1A00" => "Status message sent",
        "1A01" => "Message sent, Miguel",
        "1C00" => "Interlock change",
        "2201" => "Error Over Voltage",
        "2202" => "Error Under Voltage",
        "2203" => "Error Over Current Discharge",
        "2204" => "Error Over Current Regenerative",
        "2205" => "Error Over Current Charge",
        "2206" => "Error Over Temperature",
        "2207" => "Error Under Temperature",
        "2208" => "SCD",
        "2301" => "Shutdown Undervoltage: 12V.UI , VbatE , etc",
        "2302" => "Harakiri Voltage: 5V.UI , +5V.uC , etc.",
        "3004" => "Time change over CAN, 4 bytes",
        "3007" => "Time change over CAN, 7 bytes",
        "3010" => "Time change from GPS",
        "3011" => "GPS OFFSET",
        "3101" => "MCU BOOTLOADER Update",
        "3102" => "MCU BOOTLOADER Updated",
        "3F00" => "Downlink",
        "4020" => "Parameters Flags",
        "4021" => "Parameters BPID",
        "5A01" => "Stack Overflow",
        _ => "",
    }
}

pub const EVENTS: [[&str; 4]; 4] = [
    ["19/10/26 14:02:11", "0x1023", "00000000", "On from the internal charger"],
    ["19/10/26 14:02:16", "0x2201", "00000045", "Error Over Voltage"],
    ["19/10/26 13:40:02", "0x1022", "00000000", "Key on"],
    ["19/10/26 09:15:44", "0x10A0", "00000001", "System start, reason for reset"],
];

const BATTERY: &[Section] = &[
    Section { title: "Currents", rows: &[
        row("Capacity", "Ah", 1, 1, 3, Kind::Int, -3, false, 1),
        row("Charge current", "A", 2, 1, 3, Kind::Int, -3, false, 1),
        row("VCT current", "A", 2, 4, 6, Kind::Int, -3, false, 1),
        row("Precharge current", "A", 3, 1, 3, Kind::Int, -3, false, 1),
        row("Revive current", "A", 3, 4, 6, Kind::Int, -3, false, 1),
        row("Current dsch max", "A", 5, 1, 3, Kind::Int, -3, false, 1),
        row("Current dsch high", "A", 5, 4, 6, Kind::Int, -3, false, 1),
        row("Current reg max", "A", 6, 1, 3, Kind::Int, -3, false, 1),
        row("Current reg high", "A", 6, 4, 6, Kind::Int, -3, false, 1),
        row("Current chg max", "A", 7, 1, 3, Kind::Int, -3, false, 1),
        row("Current chg ob high", "A", 8, 1, 3, Kind::Int, -3, false, 1),
        row("Current chg ex high", "A", 8, 4, 6, Kind::Int, -3, false, 1),
        row("Current dsch Temp Limit WARN", "A", 26, 1, 2, Kind::Int, -1, false, 1),
        row("Current dsch Temp Limit ERROR", "A", 26, 3, 4, Kind::Int, -1, false, 1),
        row("Temp Reg Chg Min Current", "A", 42, 5, 7, Kind::Int, -3, false, 1),
        row("Idle current", "mA", 22, 1, 4, Kind::Float, 3, false, 1),
        row("Kmain Open Counter Current", "A", 24, 5, 7, Kind::Int, -3, false, 1),
        row("Kmain Coil Current", "mA", 37, 5, 6, Kind::Int, 0, false, 1),
    ]},
    Section { title: "Voltages", rows: &[
        row("Cell voltage max", "mV", 9, 1, 2, Kind::Int, 0, false, 1),
        row("Cell voltage high", "mV", 9, 3, 4, Kind::Int, 0, false, 1),
        row("Cell voltage max chg", "mV", 9, 5, 6, Kind::Int, 0, false, 1),
        row("Cell voltage min", "mV", 10, 1, 2, Kind::Int, 0, false, 1),
        row("Cell voltage low", "mV", 10, 3, 4, Kind::Int, 0, false, 1),
        row("Cell voltage min chg", "mV", 10, 5, 6, Kind::Int, 0, false, 1),
        row("Cell voltage hyteresi chg", "mV", 22, 5, 6, Kind::Int, 0, false, 1),
        row("Cell voltage harakiri", "mV", 11, 1, 2, Kind::Int, 0, false, 1),
        row("Cell voltage harakiri delta", "mV", 11, 3, 4, Kind::Int, 0, false, 1),
        row("Cell voltage power down", "mV", 12, 1, 2, Kind::Int, 0, false, 1),
        row("Cell voltage power down delta", "mV", 12, 3, 4, Kind::Int, 0, false, 1),
        row("Cell voltage harakiri No Charge", "mV", 23, 5, 6, Kind::Int, 0, false, 1),
        row("Kweld Hysteresis Voltage", "V", 37, 3, 4, Kind::Int, -3, false, 1),
    ]},
    Section { title: "Temperatures", rows: &[
        row("Temp max dsch", "degC", 13, 1, 1, Kind::Int, 0, true, 1),
        row("Temp high dsch", "degC", 13, 2, 2, Kind::Int, 0, true, 1),
        row("Temp min dsch", "degC", 13, 3, 3, Kind::Int, 0, true, 1),
        row("Temp low dsch", "degC", 13, 4, 4, Kind::Int, 0, true, 1),
        row("Temp min chg", "degC", 13, 5, 5, Kind::Int, 0, true, 1),
        row("Temp low chg", "degC", 13, 6, 6, Kind::Int, 0, true, 1),
        row("Temp heater OFF", "degC", 14, 1, 1, Kind::Int, 0, true, 1),
        row("Temp fan OFF", "degC", 14, 2, 2, Kind::Int, 0, true, 1),
        row("Temp max chg", "degC", 14, 5, 5, Kind::Int, 0, true, 1),
        row("Temp high chg", "degC", 14, 6, 6, Kind::Int, 0, true, 1),
    ]},
    Section { title: "Delays", rows: &[
        row("Time fan OFF", "s", 14, 3, 4, Kind::Int, 0, false, 1),
        row("WOV Time", "s", 17, 1, 2, Kind::Int, -1, false, 1),
        row("WUV Time", "s", 17, 3, 4, Kind::Int, -1, false, 1),
        row("WOT Time", "s", 17, 5, 6, Kind::Int, -1, false, 1),
        row("WOCD time", "s", 18, 1, 2, Kind::Int, -1, false, 1),
        row("WOCR Time", "s", 18, 3, 4, Kind::Int, -1, false, 1),
        row("WOCC Time", "s", 18, 5, 6, Kind::Int, -1, false, 1),
        row("EOV Time", "s", 19, 1, 2, Kind::Int, -1, false, 1),
        row("EUV Time", "s", 19, 3, 4, Kind::Int, -1, false, 1),
        row("EOT Time", "s", 19, 5, 6, Kind::Int, -1, false, 1),
        row("EOCD Time", "s", 20, 1, 2, Kind::Int, -1, false, 1),
        row("EOCR Time", "s", 20, 3, 4, Kind::Int, -1, false, 1),
        row("EOCC Time", "s", 20, 5, 6, Kind::Int, -1, false, 1),
        row("WUT Time", "s", 21, 1, 2, Kind::Int, -1, false, 1),
        row("EUT Time", "s", 21, 3, 4, Kind::Int, -1, false, 1),
        row("Pre-Error Time", "s", 21, 5, 5, Kind::Int, -1, false, 1),
        row("Pre-Limit Time", "s", 21, 6, 6, Kind::Int, -1, false, 1),
        row("Cell voltage harakiri delay", "s", 11, 5, 6, Kind::Int, 0, false, 1),
        row("Cell voltage power down delay", "s", 12, 5, 6, Kind::Int, 0, false, 1),
        row("Update AFE Readings Min Period", "min", 36, 1, 2, Kind::Int, 0, false, 1),
        row("Kweld Timeout", "s", 37, 1, 2, Kind::Int, -3, false, 1),
    ]},
    Section { title: "Warnings", rows: &[
        row("OverTemp Warning", "degC", 16, 1, 1, Kind::Int, -1, false, 1),
        row("UnderTemp Warning", "degC", 16, 2, 2, Kind::Int, -1, false, 1),
        row("UnderVoltage Warning", "mV", 16, 3, 3, Kind::Int, 0, false, 1),
        row("Low SoC", "%", 16, 4, 4, Kind::Int, 0, false, 1),
    ]},
    Section { title: "Limits & Regulation", rows: &[
        row("Chg CC: Ki", "", 4, 1, 2, Kind::Int, -1, false, 1),
        row("Chg CV: Ki", "", 4, 3, 4, Kind::Int, -1, false, 1),
        row("Chg Ext Ki Factor", "", 4, 5, 5, Kind::Int, -1, false, 1),
        row("Dsch CV: Ki", "A/mVs", 24, 1, 4, Kind::Float, 0, false, 1),
        row("Current Limit Ramp", "A/s", 23, 1, 4, Kind::Float, 0, false, 1),
        row("Temp Reg Chg Mode", "", 43, 7, 7, Kind::Int, 0, false, 1),
        row("Temp Reg Chg Slope Threshold", "degC/min", 42, 1, 4, Kind::Float, 0, false, 1),
        row("Temp Reg Chg Filter", "", 44, 5, 5, Kind::Int, 0, false, 1),
        row("Temp Reg Chg Kp", "", 43, 1, 4, Kind::Float, 0, false, 1),
        row("Temp Reg Chg Ki", "", 44, 1, 4, Kind::Float, 0, false, 1),
        row("Temp Reg Chg Kd", "", 45, 1, 4, Kind::Float, 0, false, 1),
        row("Temp Reg Chg nFactor", "", 44, 6, 6, Kind::Int, 0, false, 1),
        row("Temp Reg Chg Legacy Coef", "", 43, 5, 6, Kind::Int, -1, false, 1),
    ]},
    Section { title: "Flags & Misc", rows: &[
        row("Temp Enable", "", 15, 1, 1, Kind::Bin, 0, false, 1),
        row("Temp detected", "", 15, 2, 2, Kind::Bin, 0, false, 1),
        row("Enable flags", "", 32, 1, 4, Kind::Hex, 0, false, 1),
        row("Current dsch Temp Limit MODE", "", 26, 5, 5, Kind::Hex, 0, false, 1),
        row("Protection Flags", "", 15, 3, 3, Kind::Bin, 0, false, 1),
        row("LEDr tap", "", 32, 5, 5, Kind::Int, 0, false, 1),
        row("LEDr chg", "", 32, 6, 6, Kind::Int, 0, false, 1),
        row("LEDr chd", "", 32, 7, 7, Kind::Int, 0, false, 1),
        row("Kmain Open Counter Inhibick", "", 37, 7, 7, Kind::Int, 0, false, 1),
    ]},
    Section { title: "Balance", rows: &[
        row("Balance threshold H", "mV", 27, 1, 2, Kind::Int, 0, false, 1),
        row("Balance threshold L", "mV", 27, 3, 4, Kind::Int, 0, false, 1),
        row("Temperature balance max", "degC", 27, 5, 5, Kind::Int, 0, false, 1),
        row("Temperature balance min", "degC", 27, 6, 6, Kind::Int, 0, false, 1),
        row("Balance algorithm", "", 27, 7, 7, Kind::Int, 0, false, 1),
        row("Time max ON", "s", 28, 1, 2, Kind::Int, 0, false, 1),
        row("Time min OFF", "s", 28, 3, 4, Kind::Int, 0, false, 1),
        row("Balance TempSensorEn", "", 28, 5, 6, Kind::Hex, 0, false, 1),
        row("Allowed balance cell voltage max", "mV", 29, 1, 2, Kind::Int, 0, false, 1),
        row("Allowed balance cell voltage min", "mV", 29, 3, 4, Kind::Int, 0, false, 1),
    ]},
    Section { title: "VSOC", rows: &[
        row("Max Current Threshold", "A", 25, 1, 2, Kind::Int, -2, false, 1),
        row("Min Current Timeout", "s", 25, 3, 4, Kind::Int, 0, false, 1),
        row("Min delta from SoC", "%", 25, 5, 5, Kind::Int, 0, false, 1),
        row("Smooth Transition Slope", "s/%", 25, 6, 6, Kind::Int, 0, false, 1),
        row("Smooth Transition Current Threshold", "A", 25, 7, 7, Kind::Int, 0, false, 1),
    ]},
    Section { title: "AFE", rows: &[
        row("ADC GAIN", "", 48, 1, 1, Kind::Int, 0, false, 1),
        row("ADC OFFSET", "", 48, 2, 2, Kind::Int, 0, false, 1),
        row("bq ADC GAIN", "", 48, 3, 3, Kind::Int, 0, false, 1),
        row("bq ADC OFFSET", "", 48, 4, 4, Kind::Int, 0, false, 1),
        row("Shunt Value", "uOhm", 49, 1, 4, Kind::Float, 6, false, 1),
        row("NTCs β", "", 50, 1, 4, Kind::Float, 0, false, 1),
        row("NTCs R0", "Ohm", 51, 1, 4, Kind::Float, 0, false, 1),
        row("NTCs T0", "degC", 52, 1, 4, Kind::Float, 0, false, 1),
        row("SCD (PROTECT 1)", "", 53, 1, 1, Kind::Hex, 0, false, 1),
        row("bq CRC", "", 48, 7, 7, Kind::Hex, 0, false, 1),
    ]},
    Section { title: "AFE Ni Compensation", rows: &[
        row("Cell 01 Ni", "uOhm", 112, 1, 4, Kind::Float, 3, false, 1),
        row("Cell 02 Ni", "uOhm", 113, 1, 4, Kind::Float, 3, false, 1),
        row("Cell 03 Ni", "uOhm", 114, 1, 4, Kind::Float, 3, false, 1),
        row("Cell 04 Ni", "uOhm", 115, 1, 4, Kind::Float, 3, false, 1),
        row("Cell 05 Ni", "uOhm", 116, 1, 4, Kind::Float, 3, false, 1),
        row("Cell 06 Ni", "uOhm", 117, 1, 4, Kind::Float, 3, false, 1),
        row("Cell 07 Ni", "uOhm", 118, 1, 4, Kind::Float, 3, false, 1),
        row("Cell 08 Ni", "uOhm", 119, 1, 4, Kind::Float, 3, false, 1),
        row("Cell 09 Ni", "uOhm", 120, 1, 4, Kind::Float, 3, false, 1),
        row("Cell 10 Ni", "uOhm", 121, 1, 4, Kind::Float, 3, false, 1),
        row("Cell 11 Ni", "uOhm", 122, 1, 4, Kind::Float, 3, false, 1),
        row("Cell 12 Ni", "uOhm", 123, 1, 4, Kind::Float, 3, false, 1),
        row("Cell 13 Ni", "uOhm", 124, 1, 4, Kind::Float, 3, false, 1),
        row("Cell 14 Ni", "uOhm", 125, 1, 4, Kind::Float, 3, false, 1),
    ]},
    Section { title: "AFE mV Compensation", rows: &[
        row("Cell 01 offset mV", "mV", 112, 5, 6, Kind::Int, 0, true, 1),
        row("Cell 02 offset mV", "mV", 113, 5, 6, Kind::Int, 0, true, 1),
        row("Cell 03 offset mV", "mV", 114, 5, 6, Kind::Int, 0, true, 1),
        row("Cell 04 offset mV", "mV", 115, 5, 6, Kind::Int, 0, true, 1),
        row("Cell 05 offset mV", "mV", 116, 5, 6, Kind::Int, 0, true, 1),
        row("Cell 06 offset mV", "mV", 117, 5, 6, Kind::Int, 0, true, 1),
        row("Cell 07 offset mV", "mV", 118, 5, 6, Kind::Int, 0, true, 1),
        row("Cell 08 offset mV", "mV", 119, 5, 6, Kind::Int, 0, true, 1),
        row("Cell 09 offset mV", "mV", 120, 5, 6, Kind::Int, 0, true, 1),
        row("Cell 10 offset mV", "mV", 121, 5, 6, Kind::Int, 0, true, 1),
        row("Cell 11 offset mV", "mV", 122, 5, 6, Kind::Int, 0, true, 1),
        row("Cell 12 offset mV", "mV", 123, 5, 6, Kind::Int, 0, true, 1),
        row("Cell 13 offset mV", "mV", 124, 5, 6, Kind::Int, 0, true, 1),
        row("Cell 14 offset mV", "mV", 125, 5, 6, Kind::Int, 0, true, 1),
    ]},
    Section { title: "Sigfox", rows: &[
        row("Config Flags", "", 95, 5, 6, Kind::Hex, 0, false, 1),
        row("Downlink 1 HH", "h", 95, 1, 1, Kind::Int, 0, false, 1),
        row("Downlink 2 HH", "h", 95, 2, 2, Kind::Int, 0, false, 1),
        row("Downlink 3 HH", "h", 95, 3, 3, Kind::Int, 0, false, 1),
        row("Downlink 4 HH", "h", 95, 4, 4, Kind::Int, 0, false, 1),
        row("Periodic Send GPS", "min", 96, 1, 2, Kind::Int, 0, false, 1),
        row("Periodic Send VA", "min", 96, 3, 4, Kind::Int, 0, false, 1),
        row("Periodic Send kWh", "min", 96, 5, 6, Kind::Int, 0, false, 1),
        row("Periodic Send Flags", "min", 97, 1, 2, Kind::Int, 0, false, 1),
        row("GPS Timeout RTCC", "s", 98, 1, 2, Kind::Int, 0, false, 1),
        row("GPS Timeout Position", "s", 98, 3, 4, Kind::Int, 0, false, 1),
        row("GPS Timeout HDOP", "s", 98, 5, 6, Kind::Int, 0, false, 1),
        row("Downlink Timeout", "s", 99, 1, 2, Kind::Int, 0, false, 1),
    ]},
    Section { title: "Accelerometer", rows: &[
        row("Pulse Threshold", "g₀", 38, 1, 2, Kind::Int, -3, false, 1),
        row("Pulse Duration", "ms", 38, 3, 4, Kind::Int, -1, false, 1),
    ]},
];

const ECU: &[Section] = &[
    Section { title: "Numbers", rows: &[
        row("Serial number", "", 1, 1, 4, Kind::Int, 0, false, 1),
        row("VIN number", "", 2, 1, 3, Kind::Ascii, 0, false, 3),
        row("CALID number", "", 5, 1, 2, Kind::Ascii, 0, false, 3),
        plain("Battery ID", ""),
        row("HW version", "", 9, 1, 6, Kind::Ascii, 0, false, 1),
        plain("IMEI number ID", ""),
    ]},
    Section { title: "Throttle", rows: &[
        row("Throttle filter mode", "", 16, 1, 1, Kind::Int, 0, false, 1),
        row("Throttle num median", "", 16, 2, 2, Kind::Int, 0, false, 1),
        row("ThrottleFwMAX_CAN", "", 17, 1, 1, Kind::Int, 0, false, 1),
        row("ThrottleRevMAX_CAN", "", 17, 2, 2, Kind::Int, 0, false, 1),
        row("ThrottleRegMAX_CAN", "", 17, 3, 3, Kind::Int, 0, false, 1),
        row("Throttle min voltage", "V", 18, 1, 2, Kind::Int, 0, false, 1),
        row("Throttle max voltage", "V", 18, 3, 4, Kind::Int, 0, false, 1),
        row("Throttle val lower", "", 18, 5, 6, Kind::Int, 0, false, 1),
        row("Throttle val higher", "", 19, 1, 2, Kind::Int, 0, false, 1),
        row("Throttle supply threshold", "", 19, 3, 4, Kind::Int, 0, false, 1),
    ]},
    Section { title: "Speeds", rows: &[
        row("Max Speed by design", "", 20, 1, 1, Kind::Int, 0, false, 1),
        plain("Speed undervoltage", ""),
    ]},
    Section { title: "Motorbike params", rows: &[
        row("Wheel perimeter", "m", 21, 1, 4, Kind::Float, 0, false, 1),
        row("Conversion factor dash", "", 22, 1, 4, Kind::Float, 0, false, 1),
    ]},
    Section { title: "Lightning current thresholds", rows: &[
        row("Position current threshold", "", 23, 1, 2, Kind::Int, 0, false, 1),
        row("Brake current threshold", "", 23, 3, 4, Kind::Int, 0, false, 1),
        row("Whitelights current threshold", "", 23, 5, 6, Kind::Int, 0, false, 1),
        row("Blinkers current threshold CAN", "", 24, 1, 2, Kind::Int, 0, false, 1),
        row("Highbeam current threshold", "", 24, 3, 4, Kind::Int, 0, false, 1),
        row("Normalbeam current threshold", "", 24, 5, 6, Kind::Int, 0, false, 1),
    ]},
    Section { title: "Temperature limits", rows: &[
        row("Motor warning", "ºC", 25, 1, 1, Kind::Int, 0, false, 1),
        row("Motor error", "ºC", 25, 2, 2, Kind::Int, 0, false, 1),
        row("Sevcon warning", "ºC", 25, 3, 3, Kind::Int, 0, false, 1),
        row("Sevcon error", "ºC", 25, 4, 4, Kind::Int, 0, false, 1),
    ]},
    Section { title: "Energy bar", rows: &[
        row("Max current discharge", "", 26, 1, 1, Kind::Int, 0, false, 1),
        row("Max current rengen", "", 26, 2, 2, Kind::Int, 0, false, 1),
    ]},
    Section { title: "Range factor", rows: &[
        row("Range ECO", "", 27, 1, 1, Kind::Int, 0, false, 1),
        row("Range CITY", "", 27, 2, 2, Kind::Int, 0, false, 1),
        row("Range SPORT", "", 27, 3, 3, Kind::Int, 0, false, 1),
    ]},
    Section { title: "Funcionalities", rows: &[
        row("Funcionalities", "", 32, 1, 2, Kind::Int, 0, false, 1),
    ]},
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commented_names_stay_off_the_read() {
        for section in ecu() {
            for row in section.rows {
                if matches!(row.name, "Battery ID" | "IMEI number ID" | "Speed undervoltage") {
                    assert!(row.wire.is_none(), "{}", row.name);
                }
            }
        }
    }

    #[test]
    fn sample_events_use_the_english_lines() {
        assert_eq!(EVENTS[0][3], event_text("1023"));
        assert_eq!(EVENTS[2][3], event_text("1022"));
        assert_eq!(EVENTS[3][3], event_text("10A0"));
        assert_eq!(event_text("2208"), "SCD");
    }
}
