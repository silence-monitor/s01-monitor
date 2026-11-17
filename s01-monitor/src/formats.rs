//! Recordings made by other tools, read for replay. **Experimental.**
//!
//! The app's own recordings are read in `capture.rs`. This reads four common
//! text formats from other software, recognised from their first lines:
//!
//! * **candump log**, from `candump -L` (SocketCAN on Linux, python-can):
//!   `(1731234567.123456) can0 280#31000000`
//! * **candump, classic layout**, from plain `candump` with a time stamp:
//!   `(1731234567.123456)  can0  280   [4]  31 00 00 00`
//! * **SavvyCAN CSV**: a header `Time Stamp,ID,Extended,Dir,Bus,LEN,D1,...`
//!   and a row per frame, the time stamp in microseconds
//! * **Vector ASC**, from CANalyzer, CANoe and many loggers:
//!   `   0.012345 1  280  Rx   d 4 31 00 00 00`
//!
//! Only ordinary 11-bit data frames the scooter could have sent are kept.
//! Everything else is counted and left out: 29-bit identifiers, remote and
//! error frames, CAN FD, frames the logger sent itself, and every channel but
//! the busiest. The formats have variants and the readers have only been tried
//! on files written from their documentation, hence "experimental".

use crate::snapshot::Frame;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    /// This app's own recordings, and the CANable dump logs.
    #[default]
    Own,
    CandumpLog,
    CandumpClassic,
    SavvyCsv,
    VectorAsc,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Self::Own => "this app's own format",
            Self::CandumpLog => "candump log (candump -L)",
            Self::CandumpClassic => "candump, classic layout",
            Self::SavvyCsv => "SavvyCAN CSV",
            Self::VectorAsc => "Vector ASC",
        }
    }
}

/// What was left out of a file, and why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Skipped {
    /// 29-bit identifiers.
    pub extended: usize,
    /// Remote frames, error frames, CAN FD and anything else that is not a
    /// classic data frame.
    pub other: usize,
    /// Frames the logger itself sent (marked Tx).
    pub sent: usize,
    /// Frames on a channel other than the one used.
    pub other_channel: usize,
    /// Lines that looked like frames but could not be read.
    pub unreadable: usize,
}

impl Skipped {
    pub fn total(&self) -> usize {
        self.extended + self.other + self.sent + self.other_channel + self.unreadable
    }

    pub fn text(&self) -> String {
        let parts = [
            (self.extended, "29-bit identifier"),
            (self.other, "remote, error or CAN FD"),
            (self.sent, "sent by the logger"),
            (self.other_channel, "on another channel"),
            (self.unreadable, "unreadable"),
        ];
        let listed: Vec<String> = parts
            .iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, why)| format!("{count} {why}"))
            .collect();
        listed.join(", ")
    }
}

/// A file read in another tool's format.
#[derive(Debug)]
pub struct Foreign {
    pub format: Format,
    /// Frames in file order, with milliseconds since the first one kept.
    pub frames: Vec<(u64, Frame)>,
    pub skipped: Skipped,
    /// The channel the frames are from, and how many there were.
    pub channel: String,
    pub channels: usize,
}

impl Foreign {
    /// One sentence for the screen: the format, that it is experimental, and
    /// what was left out.
    pub fn note(&self) -> String {
        let mut note = format!("Experimental: read as {}.", self.format.label());
        if self.channels > 1 {
            note.push_str(&format!(" Channel {} used, of {}.", self.channel, self.channels));
        }
        if self.skipped.total() > 0 {
            note.push_str(&format!(" Left out: {}.", self.skipped.text()));
        }
        note
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Data,
    Remote,
    Error,
}

/// One line of a foreign file, before any choosing.
struct Row {
    /// Microseconds on the file's own clock; `None` when the line has no time.
    t_us: Option<u64>,
    channel: String,
    id: u32,
    extended: bool,
    sent: bool,
    kind: Kind,
    data: Vec<u8>,
}

/// Which foreign format a file is, if it is one. Looks only at the first lines.
/// The app's own formats are never returned here.
pub fn sniff(text: &str) -> Option<Format> {
    let text = text.trim_start_matches('\u{feff}');
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|line| !line.is_empty()).take(60).collect();
    // The app's own recordings and the CANable dumps start like this.
    if lines.iter().take(20).any(|line| line.starts_with("time:") || line.starts_with("# recording")) {
        return None;
    }
    // SavvyCAN: a header row naming the columns.
    if lines.iter().take(5).any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains(',') && (lower.contains("time stamp") || lower.contains("timestamp")) && lower.contains("id") && (lower.contains("len") || lower.contains("dlc"))
    }) {
        return Some(Format::SavvyCsv);
    }
    // Vector ASC: its header lines, or failing that frame lines.
    if lines.iter().take(30).any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("base hex") || lower.starts_with("base dec") || lower.starts_with("begin triggerblock")
    }) {
        return Some(Format::VectorAsc);
    }
    if lines.iter().filter(|line| asc_row(line, true).is_some()).count() >= 2 {
        return Some(Format::VectorAsc);
    }
    if lines.iter().any(|line| candump_log_row(line).is_some()) {
        return Some(Format::CandumpLog);
    }
    if lines.iter().any(|line| candump_classic_row(line).is_some()) {
        return Some(Format::CandumpClassic);
    }
    None
}

/// Reads a file in the given foreign format.
pub fn read(text: &str, format: Format) -> Result<Foreign, String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut skipped = Skipped::default();
    let rows = match format {
        Format::CandumpLog => rows_of(text, &mut skipped, candump_log_row, |line| line.starts_with('(')),
        Format::CandumpClassic => rows_of(text, &mut skipped, candump_classic_row, |line| {
            line.contains('[') && line.contains(']')
        }),
        Format::SavvyCsv => savvy_rows(text, &mut skipped),
        Format::VectorAsc => {
            let decimal = text.lines().any(|line| line.trim().to_ascii_lowercase().starts_with("base dec"));
            rows_of(text, &mut skipped, |line| asc_row(line, !decimal), asc_looks_like_a_frame)
        }
        Format::Own => return Err("This is the app's own format.".into()),
    };
    if rows.iter().all(|row| row.t_us.is_none()) && !rows.is_empty() {
        return Err("This log has no time stamps, so it cannot be replayed at its own pace. Record with candump -L (or candump -ta).".into());
    }
    // Use the channel with the most data frames.
    let mut counts: Vec<(String, usize)> = Vec::new();
    for row in rows.iter().filter(|row| row.kind == Kind::Data && !row.extended) {
        match counts.iter_mut().find(|(name, _)| *name == row.channel) {
            Some((_, count)) => *count += 1,
            None => counts.push((row.channel.clone(), 1)),
        }
    }
    let channels = counts.len();
    // The first channel with the most frames.
    let mut best: Option<&(String, usize)> = None;
    for entry in &counts {
        if best.is_none_or(|current| entry.1 > current.1) {
            best = Some(entry);
        }
    }
    let chosen = best.map(|(name, _)| name.clone()).unwrap_or_default();
    let mut kept: Vec<(u64, Frame)> = Vec::new();
    let mut first_us: Option<u64> = None;
    for row in rows {
        if row.extended || row.id > 0x7FF {
            skipped.extended += 1;
        } else if row.kind != Kind::Data {
            skipped.other += 1;
        } else if row.sent {
            skipped.sent += 1;
        } else if row.channel != chosen {
            skipped.other_channel += 1;
        } else if row.data.len() > 8 {
            skipped.other += 1;
        } else if let Some(t_us) = row.t_us {
            let first = *first_us.get_or_insert(t_us);
            kept.push((t_us.saturating_sub(first) / 1000, Frame::new(row.id as u16, &row.data)));
        } else {
            skipped.unreadable += 1;
        }
    }
    Ok(Foreign {
        format,
        frames: kept,
        skipped,
        channel: chosen,
        channels,
    })
}

/// Reads every line that `parse` takes. A line the format's frames look like
/// but `parse` cannot read is counted as unreadable; other lines (headers,
/// comments) are ignored.
fn rows_of(text: &str, skipped: &mut Skipped, parse: impl Fn(&str) -> Option<Row>, looks_like_frame: impl Fn(&str) -> bool) -> Vec<Row> {
    let mut rows = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        match parse(line) {
            Some(row) => rows.push(row),
            None if looks_like_frame(line) => skipped.unreadable += 1,
            None => {}
        }
    }
    rows
}

/// `1731234567.123456` or `0.012345` as microseconds.
fn seconds_to_us(text: &str) -> Option<u64> {
    let text = text.trim();
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut digits: String = fraction.chars().take(6).collect();
    while digits.len() < 6 {
        digits.push('0');
    }
    let whole: u64 = whole.parse().ok()?;
    let fraction: u64 = digits.parse().ok()?;
    whole.checked_mul(1_000_000)?.checked_add(fraction)
}

fn hex_bytes(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok()).collect()
}

/// `(1731234567.123456) can0 280#31000000`, `... 280#R`, `... 12345678#00`.
fn candump_log_row(line: &str) -> Option<Row> {
    let rest = line.strip_prefix('(')?;
    let (stamp, rest) = rest.split_once(')')?;
    let t_us = seconds_to_us(stamp)?;
    let mut parts = rest.split_whitespace();
    let channel = parts.next()?.to_string();
    let frame = parts.next()?;
    if frame.contains("##") {
        // CAN FD: not a classic frame.
        return Some(Row { t_us: Some(t_us), channel, id: 0, extended: false, sent: false, kind: Kind::Error, data: Vec::new() });
    }
    let (id_text, data_text) = frame.split_once('#')?;
    if !(id_text.len() == 3 || id_text.len() == 8) || !id_text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let id = u32::from_str_radix(id_text, 16).ok()?;
    let extended = id_text.len() == 8;
    let (kind, data) = if data_text.starts_with('R') {
        (Kind::Remote, Vec::new())
    } else {
        (Kind::Data, hex_bytes(data_text)?)
    };
    Some(Row { t_us: Some(t_us), channel, id, extended, sent: false, kind, data })
}

/// `(1731234567.123456)  can0  280   [4]  31 00 00 00`, also without the time
/// stamp, with a `RX`/`TX` word before the identifier, and with `remote request`.
fn candump_classic_row(line: &str) -> Option<Row> {
    let (t_us, rest) = match line.strip_prefix('(') {
        Some(rest) => {
            let (stamp, rest) = rest.split_once(')')?;
            (Some(seconds_to_us(stamp)?), rest)
        }
        None => (None, line),
    };
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let dlc_at = tokens.iter().position(|token| token.starts_with('[') && token.ends_with(']') && token.len() >= 3)?;
    if dlc_at < 2 {
        return None;
    }
    let id_text = tokens[dlc_at - 1];
    if !(id_text.len() == 3 || id_text.len() == 8) || !id_text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let dlc: usize = tokens[dlc_at][1..tokens[dlc_at].len() - 1].parse().ok()?;
    let channel = tokens[0].to_string();
    let sent = tokens[1..dlc_at - 1].iter().any(|token| token.eq_ignore_ascii_case("tx"));
    let id = u32::from_str_radix(id_text, 16).ok()?;
    let extended = id_text.len() == 8;
    let after = &tokens[dlc_at + 1..];
    if after.first().is_some_and(|token| token.eq_ignore_ascii_case("remote")) {
        return Some(Row { t_us, channel, id, extended, sent, kind: Kind::Remote, data: Vec::new() });
    }
    if dlc > 8 || after.len() < dlc {
        return None;
    }
    let mut data = Vec::new();
    for token in &after[..dlc] {
        data.push(u8::from_str_radix(token, 16).ok()?);
    }
    Some(Row { t_us, channel, id, extended, sent, kind: Kind::Data, data })
}

/// SavvyCAN's CSV: the columns are found from the header row.
fn savvy_rows(text: &str, skipped: &mut Skipped) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut columns: Option<Columns> = None;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        let Some(found) = &columns else {
            columns = Columns::from_header(&cells);
            continue;
        };
        match found.row(&cells) {
            Some(row) => rows.push(row),
            None => skipped.unreadable += 1,
        }
    }
    rows
}

struct Columns {
    time: usize,
    id: usize,
    extended: Option<usize>,
    dir: Option<usize>,
    bus: Option<usize>,
    len: usize,
    data: Vec<usize>,
}

impl Columns {
    fn from_header(cells: &[&str]) -> Option<Self> {
        let find = |names: &[&str]| cells.iter().position(|cell| names.iter().any(|name| cell.eq_ignore_ascii_case(name)));
        let time = find(&["Time Stamp", "Timestamp", "Time"])?;
        let id = find(&["ID", "Id"])?;
        let len = find(&["LEN", "DLC", "Length"])?;
        let mut data: Vec<(usize, usize)> = Vec::new();
        for (at, cell) in cells.iter().enumerate() {
            let lower = cell.to_ascii_lowercase();
            if let Some(number) = lower.strip_prefix('d').and_then(|n| n.parse::<usize>().ok()) {
                data.push((number, at));
            }
        }
        if data.is_empty() {
            return None;
        }
        data.sort_unstable();
        Some(Self {
            time,
            id,
            extended: find(&["Extended", "Ext"]),
            dir: find(&["Dir", "Direction"]),
            bus: find(&["Bus", "Channel"]),
            len,
            data: data.into_iter().map(|(_, at)| at).collect(),
        })
    }

    fn row(&self, cells: &[&str]) -> Option<Row> {
        let cell = |at: usize| cells.get(at).copied();
        // SavvyCAN writes microseconds as a whole number. A decimal point means seconds.
        let stamp = cell(self.time)?;
        let t_us = if stamp.contains('.') { seconds_to_us(stamp)? } else { stamp.parse().ok()? };
        let id = u32::from_str_radix(cell(self.id)?.trim_start_matches("0x"), 16).ok()?;
        let flagged = self
            .extended
            .and_then(cell)
            .is_some_and(|text| text.eq_ignore_ascii_case("true") || text == "1");
        let sent = self.dir.and_then(cell).is_some_and(|text| text.eq_ignore_ascii_case("tx"));
        let channel = self.bus.and_then(cell).unwrap_or("0").to_string();
        let len: usize = cell(self.len)?.parse().ok()?;
        if len > 8 || len > self.data.len() {
            return Some(Row { t_us: Some(t_us), channel, id, extended: flagged || id > 0x7FF, sent, kind: Kind::Error, data: Vec::new() });
        }
        let mut data = Vec::new();
        for at in &self.data[..len] {
            data.push(u8::from_str_radix(cell(*at)?, 16).ok()?);
        }
        Some(Row { t_us: Some(t_us), channel, id, extended: flagged || id > 0x7FF, sent, kind: Kind::Data, data })
    }
}

/// A frame line, as opposed to a header, a comment, a status or statistics line.
fn asc_looks_like_a_frame(line: &str) -> bool {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    tokens.len() >= 5
        && seconds_to_us(tokens[0]).is_some()
        && tokens[1].bytes().all(|b| b.is_ascii_digit())
        && ["rx", "tx", "txrq"].iter().any(|direction| tokens[3].eq_ignore_ascii_case(direction))
}

/// `   0.012345 1  280  Rx   d 4 31 00 00 00`. `hex` is false for a file whose
/// header says `base dec`. Header and comment lines return `None`.
fn asc_row(line: &str, hex: bool) -> Option<Row> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let t_us = seconds_to_us(tokens.first()?)?;
    if tokens.get(1) == Some(&"CANFD") {
        return Some(Row { t_us: Some(t_us), channel: "0".into(), id: 0, extended: false, sent: false, kind: Kind::Error, data: Vec::new() });
    }
    let channel = tokens.get(1)?;
    if !channel.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let channel = channel.to_string();
    let id_token = *tokens.get(2)?;
    if id_token.eq_ignore_ascii_case("ErrorFrame") {
        return Some(Row { t_us: Some(t_us), channel, id: 0, extended: false, sent: false, kind: Kind::Error, data: Vec::new() });
    }
    let (id_text, extended) = match id_token.strip_suffix(['x', 'X']) {
        Some(text) => (text, true),
        None => (id_token, false),
    };
    let id = u32::from_str_radix(id_text, if hex { 16 } else { 10 }).ok()?;
    let direction = *tokens.get(3)?;
    if !(direction.eq_ignore_ascii_case("rx") || direction.eq_ignore_ascii_case("tx") || direction.eq_ignore_ascii_case("txrq")) {
        return None;
    }
    let sent = !direction.eq_ignore_ascii_case("rx");
    let kind = *tokens.get(4)?;
    if kind.eq_ignore_ascii_case("r") {
        return Some(Row { t_us: Some(t_us), channel, id, extended, sent, kind: Kind::Remote, data: Vec::new() });
    }
    if !kind.eq_ignore_ascii_case("d") {
        return None;
    }
    let dlc: usize = tokens.get(5)?.parse().ok()?;
    if dlc > 8 || tokens.len() < 6 + dlc {
        return None;
    }
    let mut data = Vec::new();
    for token in &tokens[6..6 + dlc] {
        data.push(u8::from_str_radix(token, if hex { 16 } else { 10 }).ok()?);
    }
    Some(Row { t_us: Some(t_us), channel, id, extended, sent, kind: Kind::Data, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_all(text: &str) -> Foreign {
        let format = sniff(text).expect("a foreign format");
        read(text, format).unwrap()
    }

    fn frame(id: u16, bytes: &[u8]) -> Frame {
        Frame::new(id, bytes)
    }

    #[test]
    fn a_candump_log_is_recognised_and_read() {
        let text = "(1731234567.100000) can0 280#31000000\n\
                    (1731234567.104500) can0 181#64000A3A02F6FF00\n\
                    (1731234567.110000) can0 300#\n";
        let found = read_all(text);
        assert_eq!(found.format, Format::CandumpLog);
        assert_eq!(found.frames.len(), 3);
        assert_eq!(found.frames[0], (0, frame(0x280, &[0x31, 0, 0, 0])));
        assert_eq!(found.frames[1].0, 4, "4.5 ms after the first, rounded down");
        assert_eq!(found.frames[1].1.id, 0x181);
        assert_eq!(found.frames[2], (10, frame(0x300, &[])));
        assert_eq!(found.skipped.total(), 0);
        assert!(found.note().starts_with("Experimental: read as candump log"));
    }

    #[test]
    fn a_candump_log_leaves_out_what_the_scooter_would_not_send() {
        let text = "(100.000000) can0 280#31000000\n\
                    (100.001000) can0 1ABCDEF0#0102\n\
                    (100.002000) can0 123#R\n\
                    (100.003000) can0 123##1AABBCCDD\n\
                    (100.004000) can1 181#0102030405060708\n\
                    (100.005000) can0 zzz#00\n\
                    (100.006000) can0 181#0102030405060708\n";
        let found = read_all(text);
        assert_eq!(found.frames.len(), 2, "the first and the last on can0, and can1's lone frame is the other channel: {found:?}");
        assert_eq!(found.skipped.extended, 1);
        assert_eq!(found.skipped.other, 2, "the remote frame and the CAN FD frame");
        assert_eq!(found.skipped.other_channel, 1);
        assert_eq!(found.skipped.unreadable, 1, "the line with an identifier that is not hex: {found:?}");
        assert_eq!(found.channel, "can0");
        assert_eq!(found.channels, 2);
        let note = found.note();
        assert!(note.contains("Channel can0 used, of 2"), "{note}");
        assert!(note.contains("1 29-bit identifier"), "{note}");
    }

    #[test]
    fn a_classic_candump_with_time_stamps_is_read() {
        let text = " (1731234567.100000)  can0  280   [4]  31 00 00 00\n\
                    (1731234567.200000)  can0  181   [8]  64 00 0A 3A 02 F6 FF 00\n\
                    (1731234567.300000)  can0  RX - -  300   [0]\n\
                    (1731234567.400000)  can0  TX - -  280   [4]  31 00 00 00\n\
                    (1731234567.500000)  can0  123   [0]  remote request\n";
        let found = read_all(text);
        assert_eq!(found.format, Format::CandumpClassic);
        let ids: Vec<u16> = found.frames.iter().map(|(_, frame)| frame.id).collect();
        assert_eq!(ids, [0x280, 0x181, 0x300]);
        assert_eq!(found.frames[1].0, 100);
        assert_eq!(found.skipped.sent, 1, "the frame marked TX");
        assert_eq!(found.skipped.other, 1, "the remote request");
    }

    #[test]
    fn a_classic_candump_without_time_stamps_is_refused_with_advice() {
        let text = "  can0  280   [4]  31 00 00 00\n  can0  181   [8]  64 00 0A 3A 02 F6 FF 00\n";
        let format = sniff(text).expect("recognised as classic candump");
        assert_eq!(format, Format::CandumpClassic);
        let err = read(text, format).unwrap_err();
        assert!(err.contains("no time stamps") && err.contains("candump -L"), "{err}");
    }

    #[test]
    fn a_savvycan_csv_is_read_with_microsecond_time_stamps() {
        let text = "Time Stamp,ID,Extended,Dir,Bus,LEN,D1,D2,D3,D4,D5,D6,D7,D8\r\n\
                    1000000,00000280,false,Rx,0,4,31,00,00,00,,,,\r\n\
                    1004500,00000181,false,Rx,0,8,64,00,0A,3A,02,F6,FF,00\r\n\
                    1006000,00000181,false,Tx,0,1,AA,,,,,,,\r\n\
                    1007000,1ABCDEF0,true,Rx,0,2,01,02,,,,,,\r\n\
                    1010000,00000300,false,Rx,0,0,,,,,,,,\r\n";
        let found = read_all(text);
        assert_eq!(found.format, Format::SavvyCsv);
        assert_eq!(found.frames.len(), 3);
        assert_eq!(found.frames[0], (0, frame(0x280, &[0x31, 0, 0, 0])));
        assert_eq!(found.frames[1].0, 4);
        assert_eq!(found.frames[2], (10, frame(0x300, &[])));
        assert_eq!(found.skipped.sent, 1);
        assert_eq!(found.skipped.extended, 1);
    }

    #[test]
    fn a_savvycan_csv_with_seconds_and_shuffled_columns_still_reads() {
        let text = "Bus,ID,LEN,Time Stamp,D1,D2\n0,280,2,12.500000,AA,BB\n0,281,1,12.510000,CC,\n";
        let found = read_all(text);
        assert_eq!(found.frames, [(0, frame(0x280, &[0xAA, 0xBB])), (10, frame(0x281, &[0xCC]))]);
    }

    #[test]
    fn a_vector_asc_is_read_and_its_header_is_ignored() {
        let text = "date Mon Nov 16 10:02:03.123 pm 2026\n\
                    base hex  timestamps absolute\n\
                    internal events logged\n\
                    // version 9.0.0\n\
                    Begin TriggerBlock Mon Nov 16 10:02:03.123 pm 2026\n\
                       0.000000 Start of measurement\n\
                       0.012345 1  280             Rx   d 4 31 00 00 00\n\
                       0.016845 1  181             Rx   d 8 64 00 0A 3A 02 F6 FF 00  Length = 0 BitCount = 0 ID = 385\n\
                       0.020000 1  1234ABCx        Rx   d 2 01 02\n\
                       0.021000 1  280             Tx   d 4 31 00 00 00\n\
                       0.022000 1  280             Rx   r\n\
                       0.023000 1  ErrorFrame\n\
                       0.030000 1  300             Rx   d 0\n\
                    End TriggerBlock\n";
        let found = read_all(text);
        assert_eq!(found.format, Format::VectorAsc);
        let ids: Vec<u16> = found.frames.iter().map(|(_, frame)| frame.id).collect();
        assert_eq!(ids, [0x280, 0x181, 0x300]);
        assert_eq!(found.frames[1].0, 4, "4.5 ms after the first");
        assert_eq!(found.skipped.extended, 1);
        assert_eq!(found.skipped.sent, 1);
        assert_eq!(found.skipped.other, 2, "the remote frame and the error frame");
    }

    #[test]
    fn a_vector_asc_in_decimal_is_read_in_decimal() {
        let text = "base dec  timestamps relative\n   0.100000 1  640  Rx   d 2 49 0\n   0.200000 1  385  Rx   d 1 100\n";
        let found = read_all(text);
        assert_eq!(found.frames[0].1, frame(640, &[49, 0]));
        assert_eq!(found.frames[1].1, frame(385, &[100]));
    }

    #[test]
    fn a_vector_asc_with_two_channels_uses_the_busier_one() {
        let text = "base hex  timestamps absolute\n\
                       0.001000 1  280  Rx   d 1 01\n\
                       0.002000 2  280  Rx   d 1 02\n\
                       0.003000 2  281  Rx   d 1 03\n\
                       0.004000 2  282  Rx   d 1 04\n";
        let found = read_all(text);
        assert_eq!(found.channel, "2");
        assert_eq!(found.channels, 2);
        assert_eq!(found.frames.len(), 3);
        assert_eq!(found.skipped.other_channel, 1);
    }

    #[test]
    fn a_vector_asc_with_no_header_is_still_recognised_from_its_frame_lines() {
        let text = "   0.012345 1  280  Rx   d 4 31 00 00 00\n   0.016845 1  181  Rx   d 1 64\n";
        assert_eq!(sniff(text), Some(Format::VectorAsc));
    }

    #[test]
    fn the_apps_own_formats_are_never_taken_for_a_foreign_one() {
        let own = "# recording COM3 S5 250 kbit/s started 2026-11-13T15:55:01.463Z (UTC)\n\
                   time:15:55:05.917 RX: STDID:00280 data:31 00 00 00\n";
        assert_eq!(sniff(own), None);
        let scenario = "19:50:17.087 << t42120000\\r  [74 34 32]\n";
        assert_eq!(sniff(scenario), None);
        assert_eq!(sniff(""), None);
        assert_eq!(sniff("just some words\nand more\n"), None);
    }

    #[test]
    fn a_byte_order_mark_and_windows_line_endings_do_not_matter() {
        let text = "\u{feff}(100.000000) can0 280#31000000\r\n(100.010000) can0 281#01\r\n";
        let found = read_all(text);
        assert_eq!(found.frames.len(), 2);
        assert_eq!(found.frames[1].0, 10);
    }

    #[test]
    fn time_stamps_are_read_exactly_without_floating_point() {
        assert_eq!(seconds_to_us("1731234567.123456"), Some(1_731_234_567_123_456));
        assert_eq!(seconds_to_us("0.5"), Some(500_000));
        assert_eq!(seconds_to_us("12"), Some(12_000_000));
        assert_eq!(seconds_to_us("1.1234567"), Some(1_123_456), "more than six decimals are cut");
        assert_eq!(seconds_to_us(""), None);
        assert_eq!(seconds_to_us("-1.0"), None);
        assert_eq!(seconds_to_us("1.x"), None);
    }

    #[test]
    fn a_file_with_no_usable_frames_gives_an_empty_result_for_the_caller_to_report() {
        let text = "(1.000000) can0 1ABCDEF0#0102\n(2.000000) can0 123#R\n";
        let found = read_all(text);
        assert!(found.frames.is_empty());
        assert_eq!(found.skipped.extended, 1);
    }
}
