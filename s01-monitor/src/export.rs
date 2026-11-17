//! CSV exports and the values logger.
//!
//! Every file starts with a UTF-8 byte order mark so a spreadsheet reads the
//! degree signs, uses a comma between fields and ends each line with CRLF.
//! A reading that has not been seen is an empty cell, never 0. Every file
//! ends with an Origin column, so a simulation or a replay cannot be mistaken
//! for the scooter.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::capture::iso_utc;
use crate::explain;
use crate::frames;
use crate::readings::{self, Row};
use crate::sink::Sink;
use crate::snapshot::{Sample, Snapshot};

const BOM: &str = "\u{feff}";

/// The chart history keys, in the order they are written.
const HISTORY_KEYS: [&str; 11] = [
    "packVoltage",
    "current",
    "soc",
    "tempA",
    "tempB",
    "tempC",
    "speed",
    "range",
    "rpm",
    "motor",
    "inverter",
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

pub fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

pub fn csv_line<S: AsRef<str>>(fields: &[S]) -> String {
    let mut line = String::new();
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            line.push(',');
        }
        line.push_str(&csv_field(field.as_ref()));
    }
    line
}

/// The whole file as bytes, ready to write or to hand to a browser.
pub fn table_bytes(table: &Table) -> Vec<u8> {
    let mut text = String::from(BOM);
    text.push_str(&csv_line(&table.headers));
    text.push_str("\r\n");
    for row in &table.rows {
        text.push_str(&csv_line(row));
        text.push_str("\r\n");
    }
    text.into_bytes()
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn write_table(path: &Path, table: &Table) -> io::Result<()> {
    std::fs::write(path, table_bytes(table))
}

/// Reads CSV text the way a spreadsheet would: quoted fields, doubled quotes
/// and line breaks inside quotes, either line ending, and an optional byte
/// order mark.
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix(BOM).unwrap_or(text);
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => record.push(std::mem::take(&mut field)),
            '\r' if !quoted => {}
            '\n' if !quoted => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

/// `2026-10-31 22:47:01.250`, UTC. Spreadsheets read this as a date and time.
pub fn stamp(t_ms: u64) -> String {
    iso_utc(t_ms).replace('T', " ").trim_end_matches('Z').to_string()
}

fn seconds(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

fn heading(row: &Row) -> String {
    if row.unit.is_empty() {
        row.label.to_string()
    } else {
        format!("{} ({})", row.label, row.unit)
    }
}

/// What the readings screen shows for a row, as text. A reading that has not
/// been seen is empty.
fn value_of(snapshot: &Snapshot, row: &Row) -> String {
    let figure = readings::figure(snapshot, row);
    if figure == "—" {
        String::new()
    } else {
        figure
    }
}

pub fn readings_table(snapshot: &Snapshot, rows: &[&Row], origin: &str) -> Table {
    Table {
        headers: ["Group", "Reading", "Value", "Unit", "Frame", "Origin"]
            .map(String::from)
            .to_vec(),
        rows: rows
            .iter()
            .map(|row| {
                vec![
                    row.group.to_string(),
                    row.label.to_string(),
                    value_of(snapshot, row),
                    row.unit.to_string(),
                    row.location.to_string(),
                    origin.to_string(),
                ]
            })
            .collect(),
    }
}

/// The chart history: every sample still held, with markers on the first
/// sample at or after the moment they were dropped.
pub fn history_table(snapshot: &Snapshot, origin: &str) -> Table {
    let mut headers = vec!["Time (UTC)".to_string(), "Elapsed (s)".to_string()];
    for key in HISTORY_KEYS {
        match readings::rows().iter().find(|row| row.key == key) {
            Some(row) => headers.push(heading(row)),
            None => headers.push(key.to_string()),
        }
    }
    headers.push("Marker".into());
    headers.push("Origin".into());
    let first = snapshot.history.front().map_or(0, |sample| sample.t_ms);
    let marks = &snapshot.explorer.marks;
    let mut next_mark = 0;
    let mut rows = Vec::new();
    for sample in &snapshot.history {
        // Saturating: if the computer's clock was set back during the session a
        // later sample can carry an earlier time, and that must not wrap round.
        let mut row = vec![stamp(sample.t_ms), seconds(sample.t_ms.saturating_sub(first))];
        for key in HISTORY_KEYS {
            row.push(sample.field(key).map_or_else(String::new, |value| value.to_string()));
        }
        // Markers older than the first sample are skipped, then each marker
        // is attached to the first sample that is not earlier than it.
        while next_mark < marks.len() && marks[next_mark].t_ms < first {
            next_mark += 1;
        }
        let mut notes = Vec::new();
        while next_mark < marks.len() && marks[next_mark].t_ms <= sample.t_ms {
            notes.push(marks[next_mark].text.clone());
            next_mark += 1;
        }
        row.push(notes.join(" | "));
        row.push(origin.to_string());
        rows.push(row);
    }
    Table { headers, rows }
}

/// Every flag with its state. A flag whose frame has not arrived, or arrived
/// too short to hold the byte, is "No data", not "Clear".
pub fn flag_table(snapshot: &Snapshot, origin: &str) -> Table {
    let mut rows = Vec::new();
    for flag in explain::catalogue() {
        let bytes = snapshot.bytes_for(flag.id).filter(|bytes| flag.byte < bytes.len);
        let state = match bytes {
            None => "No data",
            Some(_) if explain::is_set(snapshot, flag) => "Set",
            Some(_) => "Clear",
        };
        let frame_bytes = bytes.map_or_else(String::new, |bytes| {
            bytes.data[..bytes.len]
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        });
        let seen = snapshot
            .explorer
            .ids
            .get(&flag.id)
            .map_or_else(String::new, |stats| stamp(stats.last_ms));
        rows.push(vec![
            flag.group.to_string(),
            flag.name.to_string(),
            state.to_string(),
            format!("{:#X}", flag.id),
            flag.byte.to_string(),
            flag.bit.to_string(),
            frame_bytes,
            seen,
            explain::flag_detail(flag),
            origin.to_string(),
        ]);
    }
    Table {
        headers: [
            "Group",
            "Flag",
            "State",
            "Frame",
            "Byte",
            "Bit",
            "Frame bytes",
            "Frame last seen (UTC)",
            "Meaning",
            "Origin",
        ]
        .map(String::from)
        .to_vec(),
        rows,
    }
}

pub fn frame_list(snapshot: &Snapshot, origin: &str) -> Table {
    let mut rows = Vec::new();
    for (id, stats) in &snapshot.explorer.ids {
        let data = stats.data[..stats.len]
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        let moved = (0..stats.len)
            .filter(|index| stats.has_varied(*index))
            .map(|index| index.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        rows.push(vec![
            format!("0x{id:03X}"),
            stats.count.to_string(),
            stats.rate().map_or_else(String::new, |rate| format!("{rate:.2}")),
            stats.len.to_string(),
            data,
            moved,
            if stats.decoded { "yes" } else { "no" }.to_string(),
            frames::label(*id).to_string(),
            origin.to_string(),
        ]);
    }
    Table {
        headers: [
            "ID",
            "Frames",
            "Rate (Hz)",
            "DLC",
            "Latest bytes",
            "Bytes that changed",
            "Decoded",
            "Meaning",
            "Origin",
        ]
        .map(String::from)
        .to_vec(),
        rows,
    }
}

pub fn marker_list(snapshot: &Snapshot, origin: &str) -> Table {
    let start = snapshot.explorer.ids.values().map(|stats| stats.first_ms).min().unwrap_or(0);
    Table {
        headers: ["Number", "Time (UTC)", "After first frame (s)", "Note", "Origin"]
            .map(String::from)
            .to_vec(),
        rows: snapshot
            .explorer
            .marks
            .iter()
            .enumerate()
            .map(|(index, mark)| {
                vec![
                    (index + 1).to_string(),
                    stamp(mark.t_ms),
                    seconds(mark.t_ms.saturating_sub(start)),
                    mark.text.clone(),
                    origin.to_string(),
                ]
            })
            .collect(),
    }
}

/// Writes one row of every reading at a steady rate for as long as it is
/// attached to the snapshot. The sources already take a sample every 250 ms.
#[derive(Debug)]
pub struct ValueLogger {
    out: Sink,
    pub path: PathBuf,
    pub rows: u64,
    pub error: Option<String>,
    interval_ms: u64,
    origin: String,
    first_ms: Option<u64>,
    last_ms: Option<u64>,
    marks_seen: usize,
    flushed_at: u64,
    columns: Vec<&'static Row>,
}

impl ValueLogger {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn create(path: &Path, origin: &str, interval_ms: u64) -> io::Result<Self> {
        Self::begin(Sink::file(path)?, path.to_path_buf(), origin, interval_ms)
    }

    /// A log kept in memory, for a browser. `name` is the file name it will
    /// be downloaded as.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn in_memory(name: &str, origin: &str, interval_ms: u64) -> io::Result<Self> {
        Self::begin(Sink::memory(), PathBuf::from(name), origin, interval_ms)
    }

    fn begin(mut out: Sink, path: PathBuf, origin: &str, interval_ms: u64) -> io::Result<Self> {
        let columns: Vec<&'static Row> = readings::rows().iter().collect();
        let mut headers = vec!["Time (UTC)".to_string(), "Elapsed (s)".to_string()];
        headers.extend(columns.iter().map(|row| heading(row)));
        headers.push("Marker".into());
        headers.push("Origin".into());
        out.write_all(BOM.as_bytes())?;
        write!(out, "{}\r\n", csv_line(&headers))?;
        out.flush()?;
        Ok(Self {
            out,
            path,
            rows: 0,
            error: None,
            interval_ms,
            origin: origin.to_string(),
            first_ms: None,
            last_ms: None,
            marks_seen: 0,
            flushed_at: 0,
            columns,
        })
    }

    /// Ends the log and returns its bytes when it was kept in memory.
    pub fn take_bytes(&mut self) -> Option<Vec<u8>> {
        self.finish();
        self.out.take_memory()
    }

    pub fn sample(&mut self, snapshot: &Snapshot, t_ms: u64) {
        if self.error.is_some() {
            return;
        }
        if let Some(last) = self.last_ms {
            if t_ms < last + self.interval_ms {
                return;
            }
        }
        let first = *self.first_ms.get_or_insert(t_ms);
        self.last_ms = Some(t_ms);
        let mut row = vec![stamp(t_ms), seconds(t_ms.saturating_sub(first))];
        row.extend(self.columns.iter().map(|column| value_of(snapshot, column)));
        let marks = &snapshot.explorer.marks;
        let seen = self.marks_seen.min(marks.len());
        row.push(
            marks[seen..]
                .iter()
                .map(|mark| mark.text.as_str())
                .collect::<Vec<_>>()
                .join(" | "),
        );
        self.marks_seen = marks.len();
        row.push(self.origin.clone());
        if let Err(err) = write!(self.out, "{}\r\n", csv_line(&row)) {
            self.error = Some(format!("Could not write the log: {err}"));
            return;
        }
        self.rows += 1;
        if t_ms.abs_diff(self.flushed_at) >= 2000 {
            self.flushed_at = t_ms;
            self.finish();
        }
    }

    pub fn finish(&mut self) {
        if let Err(err) = self.out.flush() {
            self.error.get_or_insert(format!("Could not write the log: {err}"));
        }
    }
}

impl Drop for ValueLogger {
    fn drop(&mut self) {
        let _ = self.out.flush();
    }
}

impl Sample {
    /// A sample's value for one of the chart keys.
    pub fn field(&self, key: &str) -> Option<f32> {
        match key {
            "packVoltage" => self.voltage,
            "current" => self.current,
            "soc" => self.soc,
            "tempA" => self.temp_a,
            "tempB" => self.temp_b,
            "tempC" => self.temp_c,
            "speed" => self.speed,
            "range" => self.range_km,
            "rpm" => self.rpm,
            "motor" => self.motor,
            "inverter" => self.inverter,
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode;
    use crate::snapshot::Frame;
    use std::sync::{Arc, Mutex};

    /// Splits one CSV line the way a spreadsheet would.
    fn split(line: &str) -> Vec<String> {
        let mut fields = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, quoted) {
                ('"', true) if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                ('"', _) => quoted = !quoted,
                (',', false) => fields.push(std::mem::take(&mut field)),
                _ => field.push(c),
            }
        }
        fields.push(field);
        fields
    }

    fn snapshot_with_pack() -> Snapshot {
        let mut snapshot = Snapshot::default();
        decode::apply(&mut snapshot, &Frame::new(0x181, &[1, 80, 0, 0, 0x42, 0x02, 0xFC, 0xFF]));
        snapshot
    }

    #[test]
    fn fields_with_commas_and_quotes_are_quoted() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_line(&["a,b", "say \"hi\"", "x"]), "\"a,b\",\"say \"\"hi\"\"\",x");
        assert_eq!(split(&csv_line(&["a,b", "say \"hi\"", "x"])), ["a,b", "say \"hi\"", "x"]);
    }

    #[test]
    fn csv_text_reads_back_with_quotes_and_line_breaks() {
        let text = "\u{feff}a,b\r\n\"x,1\",\"say \"\"hi\"\"\"\r\n\"two\nlines\",\r\n";
        let records = parse_csv(text);
        assert_eq!(records.len(), 3);
        assert_eq!(records[0], ["a", "b"]);
        assert_eq!(records[1], ["x,1", "say \"hi\""]);
        assert_eq!(records[2], ["two\nlines", ""]);
    }

    #[test]
    fn readings_come_out_as_shown_and_unseen_ones_are_empty() {
        let snapshot = snapshot_with_pack();
        let rows: Vec<&Row> = readings::rows().iter().collect();
        let table = readings_table(&snapshot, &rows, "Live bus");
        assert_eq!(table.rows.len(), rows.len());
        let voltage = table.rows.iter().find(|row| row[1] == "Pack voltage").unwrap();
        assert_eq!(voltage[2], "57.8");
        assert_eq!(voltage[3], "V");
        assert_eq!(voltage[4], "0x181 bytes 4–5");
        assert_eq!(voltage[5], "Live bus");
        let speed = table.rows.iter().find(|row| row[1] == "Speed").unwrap();
        assert_eq!(speed[2], "", "a reading never seen is empty, not 0");
        for row in &table.rows {
            assert_eq!(row.len(), table.headers.len());
        }
    }

    #[test]
    fn history_rows_carry_markers_and_the_origin() {
        let mut snapshot = Snapshot::default();
        snapshot.voltage = Some(57.3);
        snapshot.push_sample(1_793_448_180_000);
        snapshot.push_sample(1_793_448_180_250);
        snapshot.speed = Some(12.0);
        snapshot.push_sample(1_793_448_180_500);
        snapshot.explorer.add_mark("pressed reverse", 1_793_448_180_300);
        let table = history_table(&snapshot, "Replay of x.log");
        assert_eq!(table.rows.len(), 3);
        assert_eq!(table.headers[0], "Time (UTC)");
        assert_eq!(table.headers[2], "Pack voltage (V)");
        assert_eq!(table.rows[0][0], "2026-10-31 12:03:00.000");
        assert_eq!(table.rows[2][1], "0.500");
        assert_eq!(table.rows[0][2], "57.3");
        let speed = table.headers.iter().position(|h| h == "Speed (km/h)").unwrap();
        assert_eq!(table.rows[0][speed], "");
        assert_eq!(table.rows[2][speed], "12");
        let marker = table.headers.iter().position(|h| h == "Marker").unwrap();
        assert_eq!(table.rows[1][marker], "");
        assert_eq!(table.rows[2][marker], "pressed reverse");
        assert_eq!(table.rows[2][marker + 1], "Replay of x.log");
    }

    #[test]
    fn a_clock_set_back_mid_session_does_not_wrap_the_elapsed_time() {
        let mut snapshot = Snapshot::default();
        snapshot.voltage = Some(57.3);
        snapshot.push_sample(1_793_448_180_000);
        // The next sample is stamped earlier, as after a clock correction.
        snapshot.history.push_back(Sample {
            t_ms: 1_793_448_179_000,
            ..Sample::default()
        });
        let table = history_table(&snapshot, "Live bus");
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0][1], "0.000");
        assert_eq!(table.rows[1][1], "0.000", "held at zero, not a huge wrapped number");
    }

    #[test]
    fn flags_are_set_clear_or_no_data() {
        let first = explain::catalogue().iter().find(|flag| flag.id == 0x184).unwrap();
        let mut data = [0u8; 8];
        data[first.byte] = 1 << first.bit;
        let mut snapshot = Snapshot::default();
        let table = flag_table(&snapshot, "Live bus");
        assert_eq!(table.rows.len(), explain::catalogue().len());
        assert!(table.rows.iter().all(|row| row[2] == "No data" && row[6].is_empty()));
        let frame = Frame::new(0x184, &data);
        snapshot.explorer.observe(&frame, 1_793_448_180_000);
        decode::apply(&mut snapshot, &frame);
        let table = flag_table(&snapshot, "Live bus");
        let mine = table.rows.iter().find(|row| row[1] == first.name && row[3] == "0x184").unwrap();
        assert_eq!(mine[2], "Set");
        assert_eq!(mine[4], first.byte.to_string());
        assert_eq!(mine[5], first.bit.to_string());
        assert_eq!(mine[7], "2026-10-31 12:03:00.000");
        assert!(mine[6].starts_with(&format!("{:02X}", data[0])));
        let clear = table.rows.iter().filter(|row| row[3] == "0x184" && row[2] == "Clear").count();
        let in_frame = explain::catalogue().iter().filter(|flag| flag.id == 0x184).count();
        assert_eq!(clear, in_frame - 1);
        assert!(table.rows.iter().any(|row| row[3] != "0x184" && row[2] == "No data"));
        for row in &table.rows {
            assert_eq!(row.len(), table.headers.len());
            assert_eq!(row[9], "Live bus");
        }
    }

    #[test]
    fn the_frame_list_flags_what_is_not_decoded() {
        let mut snapshot = Snapshot::default();
        snapshot.explorer.observe(&Frame::new(0x7E0, &[1, 2, 3]), 0);
        snapshot.explorer.observe(&Frame::new(0x7E0, &[1, 9, 3]), 100);
        let table = frame_list(&snapshot, "Live bus");
        assert_eq!(table.rows.len(), 1);
        let row = &table.rows[0];
        assert_eq!(row[0], "0x7E0");
        assert_eq!(row[1], "2");
        assert_eq!(row[4], "01 09 03");
        assert_eq!(row[5], "1");
        assert_eq!(row[6], "no");
    }

    #[test]
    fn the_marker_list_counts_from_the_first_frame() {
        let mut snapshot = Snapshot::default();
        snapshot.explorer.observe(&Frame::new(0x280, &[0]), 1_000);
        snapshot.explorer.add_mark("side stand down", 3_500);
        let table = marker_list(&snapshot, "Live bus");
        assert_eq!(table.rows[0][0], "1");
        assert_eq!(table.rows[0][2], "2.500");
        assert_eq!(table.rows[0][3], "side stand down");
    }

    #[test]
    fn a_table_file_has_a_byte_order_mark_and_crlf_lines() {
        let path = std::env::temp_dir().join("s01-table-test.csv");
        let table = Table {
            headers: vec!["Temp (°C)".into(), "Note".into()],
            rows: vec![vec!["12.5".into(), "a,b".into()]],
        };
        write_table(&path, &table).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..3], [0xEF, 0xBB, 0xBF]);
        let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
        assert_eq!(text, "Temp (°C),Note\r\n12.5,\"a,b\"\r\n");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_logger_writes_at_its_interval_and_attaches_markers() {
        let path = std::env::temp_dir().join("s01-values-test.csv");
        let mut snapshot = snapshot_with_pack();
        let logger = ValueLogger::create(&path, "Simulation, not a bus", 1000).unwrap();
        snapshot.logger = Some(Arc::new(Mutex::new(logger)));
        let t0 = 1_793_448_180_000;
        for step in 0..9 {
            if step == 5 {
                snapshot.mark_at("pressed reverse", t0 + 1_240);
            }
            snapshot.push_sample(t0 + step * 250);
        }
        let (_, rows, error) = snapshot.logger_info().unwrap();
        assert_eq!(rows, 3, "t0, t0 + 1 s and t0 + 2 s");
        assert!(error.is_none());
        snapshot.logger = None;
        let text = std::fs::read_to_string(&path).unwrap();
        let text = text.strip_prefix('\u{feff}').unwrap();
        let lines: Vec<&str> = text.split("\r\n").filter(|line| !line.is_empty()).collect();
        assert_eq!(lines.len(), 4);
        let header = split(lines[0]);
        assert_eq!(header.len(), readings::rows().len() + 4);
        let marker = header.iter().position(|h| h == "Marker").unwrap();
        for line in &lines[1..] {
            assert_eq!(split(line).len(), header.len());
        }
        assert_eq!(split(lines[1])[marker], "");
        assert_eq!(split(lines[2])[marker], "");
        assert_eq!(split(lines[3])[marker], "pressed reverse");
        assert_eq!(split(lines[3])[1], "2.000");
        let voltage = header.iter().position(|h| h == "Pack voltage (V)").unwrap();
        assert_eq!(split(lines[1])[voltage], "57.8");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_fast_logger_takes_every_sample() {
        let path = std::env::temp_dir().join("s01-values-fast-test.csv");
        let mut snapshot = snapshot_with_pack();
        let logger = ValueLogger::create(&path, "Live bus", 250).unwrap();
        snapshot.logger = Some(Arc::new(Mutex::new(logger)));
        for step in 0..4 {
            snapshot.push_sample(1_000_000 + step * 250);
        }
        assert_eq!(snapshot.logger_info().unwrap().1, 4);
        snapshot.logger = None;
        let _ = std::fs::remove_file(path);
    }
}
