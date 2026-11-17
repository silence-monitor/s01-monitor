//! Parameter reads as rows, their CSV form, and the differences between two
//! reads. Rows are matched by device, section and parameter name, never by
//! position, so a file saved earlier still lines up.

use crate::export::Table;
use crate::params::Section;
use crate::stored::EventRow;

/// One parameter as read from a device. An empty value was not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParamRow {
    pub device: String,
    pub section: String,
    pub name: String,
    pub value: String,
    pub unit: String,
    /// UTC time the device was read, `YYYY-MM-DD HH:MM:SS.mmm`, or empty.
    pub read_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub device: String,
    pub section: String,
    pub name: String,
    pub unit: String,
    pub before: String,
    pub after: String,
    /// Position of the row among that device's parameters.
    pub index: usize,
}

/// A saved set of reads to compare against.
#[derive(Clone, Debug)]
pub struct Baseline {
    pub label: String,
    pub rows: Vec<ParamRow>,
}

/// The rows of one device, in catalogue order. `None` means it has not been
/// read, which gives no rows.
pub fn param_rows(device: &str, sections: &[Section], values: Option<&[String]>, read_at: &str) -> Vec<ParamRow> {
    let Some(values) = values else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    let mut index = 0;
    for section in sections {
        for row in section.rows {
            let value = values.get(index).map(String::as_str).unwrap_or("");
            rows.push(ParamRow {
                device: device.to_string(),
                section: section.title.to_string(),
                name: row.name.to_string(),
                value: if value == "—" { String::new() } else { value.to_string() },
                unit: row.unit.to_string(),
                read_at: read_at.to_string(),
            });
            index += 1;
        }
    }
    rows
}

pub fn param_table(rows: &[ParamRow], origin: &str) -> Table {
    Table {
        headers: ["Device", "Section", "Parameter", "Value", "Unit", "Read at (UTC)", "Origin"]
            .map(String::from)
            .to_vec(),
        rows: rows
            .iter()
            .map(|row| {
                vec![
                    row.device.clone(),
                    row.section.clone(),
                    row.name.clone(),
                    row.value.clone(),
                    row.unit.clone(),
                    row.read_at.clone(),
                    origin.to_string(),
                ]
            })
            .collect(),
    }
}

pub fn event_table(rows: &[EventRow], origin: &str) -> Table {
    Table {
        headers: ["Time", "Code", "Data", "Text", "Record", "Raw frames", "Origin"]
            .map(String::from)
            .to_vec(),
        rows: rows
            .iter()
            .map(|row| {
                let mut line = row.to_vec();
                line.push(origin.to_string());
                line
            })
            .collect(),
    }
}

/// Reads rows back from a file written by [`param_table`].
pub fn parse_param_file(text: &str) -> Result<Vec<ParamRow>, String> {
    let records = crate::export::parse_csv(text);
    let not_ours = || "That file is not a parameter export from this app.".to_string();
    let header = records.first().ok_or_else(not_ours)?;
    let find = |name: &str| header.iter().position(|cell| cell == name);
    let (Some(device), Some(section), Some(name), Some(value)) =
        (find("Device"), find("Section"), find("Parameter"), find("Value"))
    else {
        return Err(not_ours());
    };
    let unit = find("Unit");
    let read_at = find("Read at (UTC)");
    let mut rows = Vec::new();
    for record in &records[1..] {
        let cell = |index: usize| record.get(index).cloned().unwrap_or_default();
        if record.iter().all(String::is_empty) {
            continue;
        }
        rows.push(ParamRow {
            device: cell(device),
            section: cell(section),
            name: cell(name),
            value: cell(value),
            unit: unit.map_or_else(String::new, cell),
            read_at: read_at.map_or_else(String::new, cell),
        });
    }
    if rows.is_empty() {
        return Err("That file has no parameters in it.".into());
    }
    Ok(rows)
}

/// What differs between a baseline and the reads now. Only a device that has
/// values on both sides is compared, and only a parameter that was read on
/// both sides. Returns the changes and the number of values compared.
pub fn compare(baseline: &[ParamRow], now: &[ParamRow]) -> (Vec<Change>, usize) {
    let has_values = |rows: &[ParamRow], device: &str| {
        rows.iter().any(|row| row.device == device && !row.value.is_empty())
    };
    let mut changes = Vec::new();
    let mut compared = 0;
    let mut position = std::collections::HashMap::<&str, usize>::new();
    for row in now {
        let index = position.entry(row.device.as_str()).or_insert(0);
        let this = *index;
        *index += 1;
        if row.value.is_empty() || !has_values(baseline, &row.device) {
            continue;
        }
        let Some(old) = baseline.iter().find(|old| {
            old.device == row.device && old.section == row.section && old.name == row.name
        }) else {
            continue;
        };
        if old.value.is_empty() {
            continue;
        }
        compared += 1;
        if old.value != row.value {
            changes.push(Change {
                device: row.device.clone(),
                section: row.section.clone(),
                name: row.name.clone(),
                unit: row.unit.clone(),
                before: old.value.clone(),
                after: row.value.clone(),
                index: this,
            });
        }
    }
    (changes, compared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export;
    use crate::params;

    fn row(device: &str, section: &str, name: &str, value: &str) -> ParamRow {
        ParamRow {
            device: device.into(),
            section: section.into(),
            name: name.into(),
            value: value.into(),
            unit: "A".into(),
            read_at: String::new(),
        }
    }

    #[test]
    fn rows_follow_the_catalogue_and_unread_values_are_empty() {
        let battery = params::battery();
        let mut values = params::fill(battery);
        values[1] = "—".into();
        let rows = param_rows("Battery", battery, Some(&values), "2026-11-01 12:00:00.000");
        assert_eq!(rows.len(), params::len(battery));
        assert_eq!(rows[0].device, "Battery");
        assert_eq!(rows[0].section, battery[0].title);
        assert_eq!(rows[0].name, battery[0].rows[0].name);
        assert_eq!(rows[1].value, "");
        assert_eq!(rows[0].read_at, "2026-11-01 12:00:00.000");
        assert!(param_rows("ECU", params::ecu(), None, "").is_empty());
    }

    #[test]
    fn only_a_changed_value_is_reported() {
        let before = [row("Battery", "Limits", "Max current", "40"), row("Battery", "Limits", "Min cell", "2500")];
        let after = [row("Battery", "Limits", "Max current", "45"), row("Battery", "Limits", "Min cell", "2500")];
        let (changes, compared) = compare(&before, &after);
        assert_eq!(compared, 2);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].name, "Max current");
        assert_eq!((changes[0].before.as_str(), changes[0].after.as_str()), ("40", "45"));
        assert_eq!(changes[0].index, 0);
    }

    #[test]
    fn a_device_read_on_one_side_only_is_not_compared() {
        let before = [row("Battery", "Limits", "Max current", "40")];
        let after = [row("Battery", "Limits", "Max current", "40"), row("ECU", "Motor", "Limit", "9")];
        let (changes, compared) = compare(&before, &after);
        assert!(changes.is_empty());
        assert_eq!(compared, 1);
    }

    #[test]
    fn a_value_not_read_is_not_a_change() {
        let before = [row("Battery", "Limits", "Max current", "40"), row("Battery", "Limits", "Other", "3")];
        let after = [row("Battery", "Limits", "Max current", ""), row("Battery", "Limits", "Other", "3")];
        let (changes, compared) = compare(&before, &after);
        assert!(changes.is_empty());
        assert_eq!(compared, 1);
    }

    #[test]
    fn rows_are_matched_by_name_not_by_position() {
        let before = [row("Battery", "Limits", "A", "1"), row("Battery", "Limits", "B", "2")];
        let after = [row("Battery", "Limits", "B", "5"), row("Battery", "Limits", "A", "1")];
        let (changes, _) = compare(&before, &after);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].name, "B");
    }

    #[test]
    fn an_exported_file_reads_back_and_compares_clean() {
        let battery = params::battery();
        let ecu = params::ecu();
        let mut rows = param_rows("Battery", battery, Some(&params::fill(battery)), "2026-11-01 12:00:00.000");
        rows.extend(param_rows("ECU", ecu, Some(&params::fill(ecu)), "2026-11-01 12:00:05.000"));
        let path = std::env::temp_dir().join("s01-params-test.csv");
        export::write_table(&path, &param_table(&rows, "Simulation, not a bus")).unwrap();
        let back = parse_param_file(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back, rows);
        let (changes, compared) = compare(&back, &rows);
        assert!(changes.is_empty());
        assert!(compared > 20, "compared {compared}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_file_that_is_not_an_export_is_refused() {
        assert!(parse_param_file("a,b\r\n1,2\r\n").is_err());
        assert!(parse_param_file("").is_err());
        assert!(parse_param_file("Device,Section,Parameter,Value\r\n").is_err());
    }

    #[test]
    fn events_get_an_origin_column() {
        let row: EventRow = [
            "12:00".into(),
            "0x10A0".into(),
            "00".into(),
            "Start".into(),
            "7".into(),
            "70 00 | 00 | 00".into(),
        ];
        let table = event_table(&[row], "Live bus");
        assert_eq!(table.headers.last().unwrap(), "Origin");
        assert_eq!(table.headers[4..6], ["Record", "Raw frames"]);
        assert_eq!(table.rows[0], ["12:00", "0x10A0", "00", "Start", "7", "70 00 | 00 | 00", "Live bus"]);
    }
}
