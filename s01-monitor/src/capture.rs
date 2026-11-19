//! Capture logs: writing one while the app runs, and reading one back.
//!
//! The written format is the same as the `dump-*.log` captures:
//!
//! ```text
//! # comment
//! time:17:45:30.977 RX: STDID:00280 data:00 00 00 00
//! ```
//!
//! An event marker is a comment line, so older tools skip it:
//!
//! ```text
//! # MARK 17:46:02.310 pressed reverse
//! ```
//!
//! The reader also takes the scenario logs (`19:50:17.087 << t4015000000000A\r`).
//! Remote request frames (`data:RTR`) and sent lines are skipped.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::formats::{self, Format};
use crate::sink::Sink;
use crate::slcan;
use crate::snapshot::Frame;

const DAY_MS: u64 = 86_400_000;

/// Writes frames and markers to a log file as they arrive.
#[derive(Debug)]
pub struct Recorder {
    out: Sink,
    pub path: PathBuf,
    pub frames: u64,
    pub marks: u32,
    pub error: Option<String>,
    flushed_at: u64,
}

impl Recorder {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn create(path: &Path, source: &str, t_ms: u64) -> io::Result<Self> {
        Self::begin(Sink::file(path)?, path.to_path_buf(), source, t_ms)
    }

    /// A recording kept in memory, for a browser. `name` is the file name it
    /// will be downloaded as.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn in_memory(name: &str, source: &str, t_ms: u64) -> io::Result<Self> {
        Self::begin(Sink::memory(), PathBuf::from(name), source, t_ms)
    }

    /// A recording kept piece by piece in a browser's storage, so it does not
    /// sit in memory. `name` is the file name it will be downloaded as.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn chunked(name: &str, source: &str, t_ms: u64, store: Box<dyn crate::sink::ChunkStore>) -> io::Result<Self> {
        let id = format!("rec-{t_ms}");
        Self::begin(Sink::chunked(store, &id, name), PathBuf::from(name), source, t_ms)
    }

    fn begin(mut out: Sink, path: PathBuf, source: &str, t_ms: u64) -> io::Result<Self> {
        writeln!(
            out,
            "# recording {} S5 250 kbit/s started {} (UTC)",
            source,
            iso_utc(t_ms)
        )?;
        writeln!(out, "# written by s01-monitor; times are UTC time of day")?;
        out.flush()?;
        Ok(Self {
            out,
            path,
            frames: 0,
            marks: 0,
            error: None,
            flushed_at: t_ms,
        })
    }

    /// For a recording kept in a browser's storage: its id and what has not gone
    /// to the store yet, to be saved along with the rest.
    pub fn chunk_tail(&mut self) -> Option<(String, Vec<u8>)> {
        self.finish();
        self.out.take_tail()
    }

    /// A problem the browser's storage has reported. The recording carries on.
    pub fn storage_warning(&self) -> Option<String> {
        self.out.store_warning()
    }

    /// Ends the recording and returns its bytes when it was kept in memory.
    pub fn take_bytes(&mut self) -> Option<Vec<u8>> {
        self.finish();
        self.out.take_memory()
    }

    pub fn frame(&mut self, frame: &Frame, t_ms: u64) {
        let line = frame_line(frame, t_ms);
        self.write(&line, t_ms);
        self.frames += 1;
    }

    pub fn mark(&mut self, text: &str, t_ms: u64) {
        let line = mark_line(text, t_ms);
        self.write(&line, t_ms);
        self.marks += 1;
    }

    pub fn finish(&mut self) {
        if let Err(err) = self.out.flush() {
            self.error.get_or_insert(format!("Could not write the log: {err}"));
        }
    }

    fn write(&mut self, line: &str, t_ms: u64) {
        if self.error.is_some() {
            return;
        }
        if let Err(err) = writeln!(self.out, "{line}") {
            self.error = Some(format!("Could not write the log: {err}"));
            return;
        }
        if t_ms.abs_diff(self.flushed_at) >= 1000 {
            self.flushed_at = t_ms;
            self.finish();
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.out.flush();
    }
}

pub fn frame_line(frame: &Frame, t_ms: u64) -> String {
    let mut line = format!("time:{} RX: STDID:{:05X} data:", clock(t_ms), frame.id);
    for (index, byte) in frame.payload().iter().enumerate() {
        if index > 0 {
            line.push(' ');
        }
        line.push_str(&format!("{byte:02X}"));
    }
    line
}

pub fn mark_line(text: &str, t_ms: u64) -> String {
    let text: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    format!("# MARK {} {}", clock(t_ms), text.trim())
}

/// `HH:MM:SS.mmm`, UTC time of day.
pub fn clock(t_ms: u64) -> String {
    let day = t_ms % DAY_MS;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        day / 3_600_000,
        day / 60_000 % 60,
        day / 1000 % 60,
        day % 1000
    )
}

/// `2026-10-31T12:00:00.000Z` from Unix milliseconds.
pub fn iso_utc(t_ms: u64) -> String {
    let days = (t_ms / DAY_MS) as i64;
    // Civil date from a day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}T{}Z", clock(t_ms))
}

/// File name stamp such as `20261031-1203`, UTC.
pub fn file_stamp(t_ms: u64) -> String {
    let iso = iso_utc(t_ms);
    format!(
        "{}{}{}-{}{}",
        &iso[0..4],
        &iso[5..7],
        &iso[8..10],
        &iso[11..13],
        &iso[14..16]
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Frame(Frame),
    Mark(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Milliseconds after the first line that carries a time.
    pub t_ms: u64,
    pub item: Item,
}

#[derive(Clone, Debug, Default)]
pub struct Capture {
    pub entries: Vec<Entry>,
    pub frames: usize,
    pub length_ms: u64,
    /// The format the file was in. Another tool's format is experimental.
    pub format: Format,
    /// For another tool's format: what was read and what was left out, in a
    /// sentence for the screen. Empty for the app's own formats.
    pub note: String,
}

pub fn parse(text: &str) -> Result<Capture, String> {
    // Another tool's format, recognised from its first lines (experimental).
    if let Some(format) = formats::sniff(text) {
        let found = formats::read(text, format)?;
        if found.frames.is_empty() {
            let why = found.skipped.text();
            return Err(if why.is_empty() {
                format!("No CAN frames found in this file (read as {}).", format.label())
            } else {
                format!("No usable CAN frames in this file (read as {}). Left out: {why}.", format.label())
            });
        }
        let note = found.note();
        let mut capture = Capture::default();
        for (t_ms, frame) in found.frames {
            // A frame stamped slightly early is held at the previous time so the
            // replay never runs backwards.
            let t_ms = capture.entries.last().map_or(t_ms, |last| t_ms.max(last.t_ms));
            capture.entries.push(Entry { t_ms, item: Item::Frame(frame) });
        }
        capture.frames = capture.entries.len();
        capture.length_ms = capture.entries.last().map_or(0, |entry| entry.t_ms);
        capture.format = format;
        capture.note = note;
        return Ok(capture);
    }
    let mut timed: Vec<(u64, Item)> = Vec::new();
    let mut previous = 0_u64;
    let mut days = 0_u64;
    for line in text.lines() {
        let Some((time_of_day, item)) = parse_line(line) else {
            continue;
        };
        // Midnight: the clock jumps back by most of a day.
        let mut absolute = days * DAY_MS + time_of_day;
        if absolute + DAY_MS / 2 < previous {
            days += 1;
            absolute += DAY_MS;
        }
        previous = absolute;
        timed.push((absolute, item));
    }
    let start = timed.first().map_or(0, |row| row.0);
    let mut capture = Capture::default();
    for (absolute, item) in timed {
        if matches!(item, Item::Frame(_)) {
            capture.frames += 1;
        }
        // A line stamped slightly early (never expected) is held at the
        // previous time so the replay never runs backwards.
        let t_ms = absolute.saturating_sub(start);
        let t_ms = capture.entries.last().map_or(t_ms, |last| t_ms.max(last.t_ms));
        capture.entries.push(Entry { t_ms, item });
    }
    capture.length_ms = capture.entries.last().map_or(0, |entry| entry.t_ms);
    if capture.frames == 0 {
        return Err("No CAN frames found in this file.".into());
    }
    Ok(capture)
}

fn parse_line(line: &str) -> Option<(u64, Item)> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix('#') {
        let rest = rest.trim_start();
        let rest = rest.strip_prefix("MARK ")?;
        let (stamp, note) = rest.split_once(' ').unwrap_or((rest, ""));
        return Some((time_of_day(stamp)?, Item::Mark(note.trim().to_string())));
    }
    if let Some(rest) = line.strip_prefix("time:") {
        // time:HH:MM:SS.mmm RX: STDID:00181 data:xx xx
        let (stamp, rest) = rest.split_once(' ')?;
        let rest = rest.trim_start().strip_prefix("RX:")?.trim_start();
        let rest = rest.strip_prefix("STDID:")?;
        let (id, rest) = rest.split_once(' ')?;
        let data = rest.trim_start().strip_prefix("data:")?.trim();
        let id = u16::from_str_radix(id, 16).ok()?;
        if id > 0x7FF || data.eq_ignore_ascii_case("RTR") {
            return None;
        }
        let mut bytes = Vec::new();
        for part in data.split_whitespace() {
            bytes.push(u8::from_str_radix(part, 16).ok()?);
        }
        if bytes.len() > 8 {
            return None;
        }
        return Some((time_of_day(stamp)?, Item::Frame(Frame::new(id, &bytes))));
    }
    // Scenario log: 19:50:17.087 << t4015000000000A\r  [..]
    let (stamp, rest) = line.split_once(' ')?;
    let rest = rest.trim_start().strip_prefix("<<")?.trim_start();
    let token = rest.split(|c: char| c.is_whitespace() || c == '\\').next()?;
    let frame = slcan::parse_line(token.as_bytes())?;
    Some((time_of_day(stamp)?, Item::Frame(frame)))
}

fn time_of_day(stamp: &str) -> Option<u64> {
    let (hms, millis) = stamp.split_once('.')?;
    let mut parts = hms.split(':');
    let hours: u64 = parts.next()?.parse().ok()?;
    let minutes: u64 = parts.next()?.parse().ok()?;
    let seconds: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || hours > 23 || minutes > 59 || seconds > 59 {
        return None;
    }
    let digits = millis.get(..3.min(millis.len()))?;
    let value: u64 = digits.parse().ok()?;
    let millis = value * 10_u64.pow(3 - digits.len() as u32);
    Some(((hours * 60 + minutes) * 60 + seconds) * 1000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_dump_line_exactly_as_the_captures_write_it() {
        let text = "# dump started\ntime:17:45:30.991 RX: STDID:00197 data:EA 26 00 00 E8 1F 64 64\n";
        let capture = parse(text).unwrap();
        assert_eq!(capture.frames, 1);
        let Item::Frame(frame) = &capture.entries[0].item else {
            panic!("not a frame");
        };
        assert_eq!(frame.id, 0x197);
        assert_eq!(frame.payload(), [0xEA, 0x26, 0, 0, 0xE8, 0x1F, 0x64, 0x64]);
    }

    #[test]
    fn a_file_recorded_by_the_units_own_sd_card_replays_as_an_app_recording() {
        // The lines firmware/main/sd_card.cpp writes (see sd_paths.h, can_line).
        let text = "# recording from the unit's SD card, 250 kbit/s, started 2026-11-19 21:45:01\n\
                    # times are the clock time of the device that set the unit's clock, or time since the unit started\n\
                    time:21:45:01.000 RX: STDID:00280 data:11 00 00 00\n\
                    # 3 frames were lost here (the recorder fell behind)\n\
                    time:21:45:01.010 RX: STDID:007FF data:\n\
                    time:21:45:01.020 RX: STDID:001AA data:00 01 AB FF 00 00 00 00\n";
        let capture = parse(text).unwrap();
        assert_eq!(capture.frames, 3);
        assert_eq!(capture.length_ms, 20);
        assert_eq!(capture.format, Format::Own);
        let Item::Frame(frame) = &capture.entries[2].item else {
            panic!("not a frame");
        };
        assert_eq!(frame.id, 0x1AA);
        assert_eq!(frame.payload(), [0x00, 0x01, 0xAB, 0xFF, 0, 0, 0, 0]);
    }

    #[test]
    fn a_written_line_reads_back_the_same() {
        let frame = Frame::new(0x1AA, &[0x18, 0x01, 0xB0, 0xFF]);
        let line = frame_line(&frame, 63_930_991);
        assert_eq!(line, "time:17:45:30.991 RX: STDID:001AA data:18 01 B0 FF");
        let back = parse(&line).unwrap();
        assert_eq!(back.entries[0].item, Item::Frame(frame));
    }

    #[test]
    fn markers_survive_a_round_trip_and_keep_their_place() {
        let text = format!(
            "{}\n{}\n{}\n",
            frame_line(&Frame::new(0x280, &[0, 0, 0, 0]), 1_000),
            mark_line("pressed reverse", 3_500),
            frame_line(&Frame::new(0x280, &[4, 0, 0, 0]), 4_000),
        );
        let capture = parse(&text).unwrap();
        assert_eq!(capture.entries.len(), 3);
        assert_eq!(capture.entries[1].t_ms, 2_500);
        assert_eq!(capture.entries[1].item, Item::Mark("pressed reverse".into()));
        assert_eq!(capture.length_ms, 3_000);
    }

    #[test]
    fn remote_requests_and_sent_lines_are_skipped() {
        let text = "time:17:45:40.529 RX: STDID:001B0 data:RTR\n\
                    # >> C\\r\n\
                    19:49:35.627 >> C\\r  [43 0d]\n\
                    time:17:45:40.600 RX: STDID:00280 data:00 00 00 00\n";
        let capture = parse(text).unwrap();
        assert_eq!(capture.entries.len(), 1);
    }

    #[test]
    fn reads_the_scenario_log_layout() {
        let text = "19:50:17.087 << t4015000000000A\\r  [74 34]\n\
                    19:50:17.187 << t184845820100040A0000\\r\n";
        let capture = parse(text).unwrap();
        assert_eq!(capture.frames, 2);
        assert_eq!(capture.length_ms, 100);
        let Item::Frame(frame) = &capture.entries[1].item else {
            panic!("not a frame");
        };
        assert_eq!(frame.id, 0x184);
    }

    #[test]
    fn the_clock_wraps_at_midnight() {
        let text = "time:23:59:59.900 RX: STDID:00280 data:00\ntime:00:00:00.100 RX: STDID:00280 data:00\n";
        let capture = parse(text).unwrap();
        assert_eq!(capture.length_ms, 200);
    }

    #[test]
    fn a_file_without_frames_is_refused() {
        assert!(parse("# nothing here\n").is_err());
    }

    #[test]
    fn dates_come_out_right() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_utc(1_793_448_180_000), "2026-10-31T12:03:00.000Z");
        assert_eq!(file_stamp(1_793_448_180_000), "20261031-1203");
    }

    #[test]
    fn marker_notes_stay_on_one_line() {
        assert_eq!(mark_line("a\nb", 0), "# MARK 00:00:00.000 a b");
    }
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Default, Clone)]
    struct Store {
        began: Arc<Mutex<Vec<(String, String)>>>,
        chunks: Arc<Mutex<Vec<String>>>,
    }

    impl crate::sink::ChunkStore for Store {
        fn begin(&mut self, id: &str, name: &str) {
            self.began.lock().unwrap().push((id.into(), name.into()));
        }
        fn append(&mut self, _id: &str, _seq: u32, text: &str) {
            self.chunks.lock().unwrap().push(text.into());
        }
        fn status(&self, _id: &str) -> Option<String> {
            None
        }
    }

    #[test]
    fn a_recording_kept_in_a_browsers_storage_reads_back_the_same_as_one_kept_in_memory() {
        let start = 1_793_448_180_000;
        let store = Store::default();
        let mut stored = Recorder::chunked("recording-x.log", "test", start, Box::new(store.clone())).unwrap();
        let mut memory = Recorder::in_memory("recording-x.log", "test", start).unwrap();
        for i in 0..3000u64 {
            let frame = Frame::new(0x181, &[i as u8, 1, 2, 3, 4, 5, 6, 7]);
            let t = start + i * 10;
            stored.frame(&frame, t);
            memory.frame(&frame, t);
            if i == 700 {
                stored.mark("pressed reverse", t);
                memory.mark("pressed reverse", t);
            }
        }
        assert_eq!(
            store.began.lock().unwrap().as_slice(),
            [("rec-1793448180000".to_string(), "recording-x.log".to_string())]
        );
        let (id, tail) = stored.chunk_tail().expect("a chunked recording has a tail");
        assert_eq!(id, "rec-1793448180000");
        let chunks = store.chunks.lock().unwrap().clone();
        assert!(!chunks.is_empty(), "a recording this long has gone to the store in pieces");
        let mut text: String = chunks.concat();
        text.push_str(&String::from_utf8(tail).unwrap());
        let expected = String::from_utf8(memory.take_bytes().unwrap()).unwrap();
        assert_eq!(text, expected, "pieces and tail are exactly what memory would have held");
        let capture = parse(&text).unwrap();
        assert_eq!(capture.frames, 3000);
        assert_eq!(stored.frames, 3000);
        assert_eq!(stored.marks, 1);
        // A memory recording has no tail to hand over.
        assert!(memory.chunk_tail().is_none());
    }
    #[test]
    fn another_tools_recording_becomes_a_capture_marked_experimental() {
        let text = "(1731234567.100000) can0 280#31000000\n\
                    (1731234567.200000) can0 181#64000A3A02F6FF00\n\
                    (1731234568.100000) can0 1ABCDEF0#0102\n";
        let capture = parse(text).unwrap();
        assert_eq!(capture.format, Format::CandumpLog);
        assert_eq!(capture.frames, 2);
        assert_eq!(capture.length_ms, 100);
        assert_eq!(capture.entries[0].item, Item::Frame(Frame::new(0x280, &[0x31, 0, 0, 0])));
        assert!(capture.note.starts_with("Experimental: read as candump log"), "{}", capture.note);
        assert!(capture.note.contains("1 29-bit identifier"), "{}", capture.note);
    }

    #[test]
    fn the_apps_own_formats_are_read_as_before_with_no_note() {
        let own = parse("# recording x\ntime:17:45:30.991 RX: STDID:00197 data:EA 26\n").unwrap();
        assert_eq!(own.format, Format::Own);
        assert!(own.note.is_empty());
        let scenario = parse("19:50:17.087 << t4015000000000A\\r  [74]\n").unwrap();
        assert_eq!(scenario.format, Format::Own);
        assert!(scenario.note.is_empty());
    }

    #[test]
    fn another_tools_file_with_nothing_usable_says_why() {
        let err = parse("(1.000000) can0 1ABCDEF0#0102\n(2.000000) can0 123#R\n").unwrap_err();
        assert!(err.contains("candump log") && err.contains("29-bit identifier"), "{err}");
        let err = parse("  can0  280   [4]  31 00 00 00\n").unwrap_err();
        assert!(err.contains("no time stamps"), "{err}");
        assert!(parse("nothing like a log\n").is_err());
    }

    #[test]
    fn every_foreign_format_replays_through_the_same_capture() {
        let candump = "(10.000000) can0 280#31000000\n(10.020000) can0 281#01\n";
        let classic = "(10.000000)  can0  280   [4]  31 00 00 00\n(10.020000)  can0  281   [1]  01\n";
        let savvy = "Time Stamp,ID,Extended,Dir,Bus,LEN,D1,D2,D3,D4\n10000000,00000280,false,Rx,0,4,31,00,00,00\n10020000,00000281,false,Rx,0,1,01,,,\n";
        let asc = "base hex  timestamps absolute\n   10.000000 1  280  Rx   d 4 31 00 00 00\n   10.020000 1  281  Rx   d 1 01\n";
        let mut lengths = Vec::new();
        for text in [candump, classic, savvy, asc] {
            let capture = parse(text).unwrap();
            assert!(capture.format != Format::Own);
            assert_eq!(capture.frames, 2);
            assert_eq!(capture.entries[0].t_ms, 0);
            assert_eq!(capture.entries[1].t_ms, 20);
            assert_eq!(capture.entries[1].item, Item::Frame(Frame::new(0x281, &[0x01])));
            lengths.push(capture.length_ms);
        }
        assert_eq!(lengths, [20, 20, 20, 20]);
    }
}
