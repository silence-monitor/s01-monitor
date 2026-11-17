use web_time::Duration;

use crate::clock::Moment;
use crate::params::{self, Kind, Section, Wire};

/// The event log is read this many records at a time. Asking for all of it at
/// once made the battery send about 900 frames a second, and that made the motor
/// controller report a fault (CAN_NOTES.md, `0x1ED` / `0x1EE`).
pub const PAGE_RECORDS: u32 = 10;
/// The pause before the next page. One second was tried on the scooter.
pub const PAGE_PAUSE: Duration = Duration::from_millis(1000);

/// One event-log record as it is shown and saved, in this order: time, code,
/// data, text, record number, and the raw bytes of its three frames. The first
/// four are what the screen and the saved log have always had; the last two let
/// a saved log be checked against the bus.
pub type EventRow = [String; 6];

const PACK: Wire = Wire {
    index: 0x21,
    first: 1,
    last: 4,
    kind: Kind::Int,
    scale: 0,
    signed: false,
    slices: 1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    Battery,
    Ecu,
    Events,
    /// Set the battery clock to this moment. This is not a read, and the
    /// session handles it without a `Job`: it is the one write the app makes.
    SetClock(Moment),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Wait,
    Send(String),
    /// Send this line after a pause.
    SendAfter(String, Duration),
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Device {
    Battery,
    Ecu,
    Events,
}

pub struct Job {
    device: Device,
    values: Vec<String>,
    index: u8,
    slice: u8,
    ascii: String,
    pack_id: Option<String>,
    done: bool,
    records: Vec<EventRow>,
    partial: Vec<[u8; 8]>,
    /// How many records the page now being read is to bring, and how many have come.
    page_asked: u32,
    page_got: u32,
    /// The index the next record must have, once one has been seen. The log
    /// comes in steps of one, downwards; anything else means a frame was lost.
    expected: Option<u32>,
    /// The controller's fault bytes (0x411 bytes 2-3) when the read began.
    fault_at_start: Option<u16>,
    fault: bool,
    out_of_order: bool,
    /// Replies from the device so far. The session watches this to see when
    /// the device has gone quiet.
    heard: u32,
}

impl Job {
    pub fn begin(ask: Ask) -> Self {
        let device = match ask {
            Ask::Battery => Device::Battery,
            Ask::Ecu => Device::Ecu,
            // A clock set has no job; the session never makes one for it.
            Ask::Events | Ask::SetClock(_) => Device::Events,
        };
        let values = vec!["—".to_string(); params::len(Self::sections_for(device))];
        let mut job = Self {
            device,
            values,
            index: 0,
            slice: 0,
            ascii: String::new(),
            pack_id: None,
            done: false,
            records: Vec::new(),
            partial: Vec::new(),
            page_asked: PAGE_RECORDS,
            page_got: 0,
            expected: None,
            fault_at_start: None,
            fault: false,
            out_of_order: false,
            heard: 0,
        };
        if device != Device::Events {
            job.skip();
        }
        job
    }

    pub fn opening_line(&self) -> Option<String> {
        if self.done {
            return None;
        }
        Some(match self.device {
            Device::Battery => battery_line(self.index),
            Device::Ecu => ecu_line(self.index),
            Device::Events => event_open_line(),
        })
    }

    pub fn on_frame(&mut self, id: u16, data: &[u8]) -> Reply {
        if self.done {
            return Reply::Done;
        }
        if self.device == Device::Events {
            return self.on_event(id, data);
        }
        let reply_id = if self.device == Device::Battery { 0x235 } else { 0x31C };
        let expected = self.index.wrapping_add(self.slice);
        if id != reply_id || data.first().copied() != Some(expected) {
            return Reply::Wait;
        }
        self.heard = self.heard.saturating_add(1);
        if self.is_pack() {
            self.pack_id = Some(raw_int(&PACK, data).map_or_else(|| "—".to_string(), |value| value.to_string()));
            return self.advance();
        }
        if self.first_wire().is_some_and(|wire| wire.kind == Kind::Ascii) {
            let wire = self.first_wire().expect("ascii row");
            self.ascii.push_str(&ascii_piece(data, self.slice, wire.slices.max(1), wire.last));
            if self.slice + 1 < wire.slices.max(1) {
                self.slice += 1;
                return Reply::Send(self.request_line());
            }
            let text = self.ascii.clone();
            self.store_text(&text);
            self.ascii.clear();
            return self.advance();
        }
        self.store_numbers(data);
        self.advance()
    }

    pub fn values(&self) -> &[String] {
        &self.values
    }

    pub fn records(&self) -> &[EventRow] {
        &self.records
    }

    pub fn done_note(&self) -> String {
        match self.device {
            Device::Battery => match &self.pack_id {
                Some(id) => format!("Battery parameters read. Pack id {id}."),
                None => "Battery parameters read.".into(),
            },
            Device::Ecu => "ECU parameters read.".into(),
            Device::Events if self.fault => {
                "The motor controller reported a fault, so the read was stopped; switch the scooter off and on again to clear it."
                    .into()
            }
            Device::Events if self.out_of_order => {
                "The event log came back out of order. Showing what was read.".into()
            }
            Device::Events => "Event log read.".into(),
        }
    }

    /// How many replies this read has had from the device.
    pub fn replies(&self) -> u32 {
        self.heard
    }

    /// The request to send again when a parameter's reply did not come. An
    /// event-log read is never sent again.
    pub fn retry_line(&self) -> Option<String> {
        if self.done || self.device == Device::Events {
            return None;
        }
        Some(self.request_line())
    }

    /// What to say when the device stopped answering and the read was given up.
    pub fn stalled_note(&self) -> String {
        match self.device {
            Device::Battery => "The battery stopped answering. Showing what was read.".into(),
            Device::Ecu => "The ECU stopped answering. Showing what was read.".into(),
            Device::Events if self.records.is_empty() => "The event log did not answer.".into(),
            Device::Events => "The event log stopped part way. Showing what was read.".into(),
        }
    }

    pub fn reading_note(&self) -> &'static str {
        match self.device {
            Device::Battery => "Reading battery parameters.",
            Device::Ecu => "Reading ECU parameters.",
            Device::Events => "Reading the event log.",
        }
    }

    pub fn ask(&self) -> Ask {
        match self.device {
            Device::Battery => Ask::Battery,
            Device::Ecu => Ask::Ecu,
            Device::Events => Ask::Events,
        }
    }

    /// The event log comes a page at a time: the first request brings the newest
    /// ten, and after each page the next ten are asked for, after a pause. The
    /// read ends at record 0, when a record is out of step, or when the
    /// controller reports a fault.
    fn on_event(&mut self, id: u16, data: &[u8]) -> Reply {
        if id == 0x411 {
            return self.watch_fault(data);
        }
        if id != 0x1EE {
            return Reply::Wait;
        }
        self.heard = self.heard.saturating_add(1);
        let mut frame = [0u8; 8];
        let len = data.len().min(8);
        frame[..len].copy_from_slice(&data[..len]);
        self.partial.push(frame);
        if self.partial.len() < 3 {
            return Reply::Wait;
        }
        let record = event_record(&self.partial[0], &self.partial[1], &self.partial[2]);
        self.partial.clear();
        if self.expected.is_some_and(|next| next != record.index) {
            self.out_of_order = true;
            self.done = true;
            return Reply::Done;
        }
        let index = record.index;
        self.records.push([
            record.time,
            record.code,
            record.data,
            record.text,
            record.index.to_string(),
            record.raw,
        ]);
        self.page_got += 1;
        if index == 0 {
            self.done = true;
            return Reply::Done;
        }
        self.expected = Some(index - 1);
        if self.page_got >= self.page_asked {
            let start = index - 1;
            self.page_asked = PAGE_RECORDS.min(start + 1);
            self.page_got = 0;
            return Reply::SendAfter(event_page_line(start, self.page_asked), PAGE_PAUSE);
        }
        Reply::Wait
    }

    /// Stops the read if the controller's fault bytes change to something other
    /// than none while it runs. A fault that was already there is not new.
    fn watch_fault(&mut self, data: &[u8]) -> Reply {
        let (Some(low), Some(high)) = (data.get(2), data.get(3)) else {
            return Reply::Wait;
        };
        let code = u16::from_le_bytes([*low, *high]);
        match self.fault_at_start {
            None => self.fault_at_start = Some(code),
            Some(before) if code != before && code != 0 => {
                self.fault = true;
                self.done = true;
                return Reply::Done;
            }
            Some(_) => {}
        }
        Reply::Wait
    }

    fn advance(&mut self) -> Reply {
        self.slice = 0;
        if self.index == 255 {
            self.done = true;
            return Reply::Done;
        }
        self.index += 1;
        self.skip();
        if self.done {
            Reply::Done
        } else {
            Reply::Send(self.request_line())
        }
    }

    fn request_line(&self) -> String {
        let which = self.index.wrapping_add(self.slice);
        if self.device == Device::Battery {
            battery_line(which)
        } else {
            ecu_line(which)
        }
    }

    fn skip(&mut self) {
        while self.index < 255 && !self.wanted(self.index) {
            self.index += 1;
        }
        if self.index == 255 {
            self.done = true;
        }
    }

    fn wanted(&self, index: u8) -> bool {
        self.has_row(index) || (self.device == Device::Battery && index == 0x21)
    }

    fn has_row(&self, index: u8) -> bool {
        self.sections().iter().any(|section| {
            section.rows.iter().any(|row| row.wire.is_some_and(|wire| wire.index == index))
        })
    }

    fn is_pack(&self) -> bool {
        self.device == Device::Battery && self.index == 0x21 && !self.has_row(0x21)
    }

    fn first_wire(&self) -> Option<Wire> {
        for section in self.sections() {
            for row in section.rows {
                if let Some(wire) = row.wire {
                    if wire.index == self.index {
                        return Some(wire);
                    }
                }
            }
        }
        None
    }

    fn store_numbers(&mut self, data: &[u8]) {
        let mut flat = 0;
        for section in self.sections() {
            for row in section.rows {
                if let Some(wire) = row.wire {
                    if wire.index == self.index && wire.kind != Kind::Ascii {
                        self.values[flat] = show_value(&wire, data);
                    }
                }
                flat += 1;
            }
        }
    }

    fn store_text(&mut self, text: &str) {
        let mut flat = 0;
        for section in self.sections() {
            for row in section.rows {
                if row.wire.is_some_and(|wire| wire.index == self.index && wire.kind == Kind::Ascii) {
                    self.values[flat] = text.to_string();
                }
                flat += 1;
            }
        }
    }

    fn sections(&self) -> &'static [Section] {
        Self::sections_for(self.device)
    }

    fn sections_for(device: Device) -> &'static [Section] {
        match device {
            Device::Battery => params::battery(),
            Device::Ecu => params::ecu(),
            Device::Events => &[],
        }
    }
}

pub fn battery_line(index: u8) -> String {
    format!("t2341{index:02X}\r")
}

pub fn ecu_line(index: u8) -> String {
    format!("t31B8{index:02X}00000000000000\r")
}

pub fn event_open_line() -> String {
    "t1ED0\r".into()
}

/// Debug mode on, sent when the bus wakes: `0x28A`, eight bytes, byte 3 = 1 and nothing else. Never the god-mode
/// bit (byte 0) or the output bits (bytes 1 and 2).
pub const DEBUG_ON_LINE: &str = "t28A80000000100000000\r";
/// Debug mode off: the same frame with all zeros, sent when the app disconnects.
pub const DEBUG_OFF_LINE: &str = "t28A80000000000000000\r";

/// A request for `count` records counting down from index `start`: two 16-bit
/// numbers, low byte first (seen on the scooter on 9 Nov 2026).
pub fn event_page_line(start: u32, count: u32) -> String {
    let first = start as u16;
    let how_many = count as u16;
    format!(
        "t1ED4{:02X}{:02X}{:02X}{:02X}\r",
        first as u8,
        (first >> 8) as u8,
        how_many as u8,
        (how_many >> 8) as u8
    )
}

pub fn show_value(wire: &Wire, data: &[u8]) -> String {
    if wire.kind == Kind::Float {
        let Some(value) = raw_float(wire, data) else {
            return "—".into();
        };
        return format!("{:.3}", value * scale_factor(wire.scale));
    }
    let Some(value) = raw_int(wire, data) else {
        return "—".into();
    };
    match wire.kind {
        Kind::Hex => format!("{:X}", value as u32),
        Kind::Bin => format!("{:b}", value as u32),
        Kind::Ascii => String::new(),
        Kind::Int | Kind::Float => format!("{:.3}", value as f64 * scale_factor(wire.scale)),
    }
}

pub fn ascii_piece(data: &[u8], slice: u8, slices: u8, last: u8) -> String {
    let end = if slice + 1 >= slices.max(1) { last } else { 7 };
    let mut text = String::new();
    for index in 1..=end {
        let Some(byte) = data.get(index as usize).copied() else {
            break;
        };
        if byte != 0 {
            text.push(byte as char);
        }
    }
    text
}

pub struct EventRecord {
    pub index: u32,
    pub time: String,
    pub code: String,
    pub data: String,
    pub text: String,
    /// The bytes of the three frames, as hex, with `|` between the frames.
    pub raw: String,
}

pub fn event_record(first: &[u8], second: &[u8], third: &[u8]) -> EventRecord {
    let index = u32::from(first.first().copied().unwrap_or(0) >> 4) + (u32::from(first.get(1).copied().unwrap_or(0)) << 4);
    let time = format!(
        "{:02}/{:02}/{:02} {:02}:{:02}:{:02}",
        at(first, 2),
        at(first, 3),
        at(first, 4),
        at(first, 5),
        at(first, 6),
        at(first, 7)
    );
    let code_hex = format!("{:02X}{:02X}", at(second, 3), at(second, 2));
    let data = format!("{}{}", hex_backward(second, 4, 4), hex_backward(third, 2, 4));
    let text = params::event_text(&code_hex).to_string();
    let raw = [first, second, third].map(hex_line).join(" | ");
    EventRecord {
        index,
        time,
        code: format!("0x{code_hex}"),
        data,
        text,
        raw,
    }
}

/// A saved-log line for one record: its six columns with tabs between.
pub fn event_row_line(row: &EventRow) -> String {
    row.join("\t")
}

fn hex_line(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect::<Vec<_>>().join(" ")
}

fn at(data: &[u8], index: usize) -> u8 {
    data.get(index).copied().unwrap_or(0)
}

fn hex_backward(data: &[u8], from: usize, len: usize) -> String {
    let mut text = String::new();
    for step in 0..len {
        text.push_str(&format!("{:02X}", at(data, from + len - step - 1)));
    }
    text
}

fn scale_factor(scale: i8) -> f64 {
    10f64.powi(i32::from(scale))
}

fn raw_int(wire: &Wire, data: &[u8]) -> Option<i64> {
    let start = wire.first as usize;
    let width = wire.last as usize - start + 1;
    if data.len() < start + width {
        return None;
    }
    Some(match width {
        1 if wire.signed => data[start] as i8 as i64,
        1 => data[start] as i64,
        2 => {
            let value = u16::from_le_bytes([data[start], data[start + 1]]);
            if wire.signed { value as i16 as i64 } else { value as i64 }
        }
        3 => u32::from_le_bytes([data[start], data[start + 1], data[start + 2], 0]) as i64,
        _ => {
            let value = u32::from_le_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
            ]);
            if wire.signed { value as i32 as i64 } else { value as i64 }
        }
    })
}

fn raw_float(wire: &Wire, data: &[u8]) -> Option<f64> {
    let start = wire.first as usize;
    if data.len() < start + 4 {
        return None;
    }
    let bytes: [u8; 4] = data[start..start + 4].try_into().ok()?;
    Some(f64::from(f32::from_le_bytes(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule in firmware/main/request_filter.h, written out again here so
    /// that a change on either side that breaks the other is caught.
    fn firmware_allows(line: &str) -> bool {
        let text = line.trim();
        if let Some(rest) = text.strip_prefix('r') {
            return rest.get(..3).and_then(|id| u16::from_str_radix(id, 16).ok()) == Some(0x183);
        }
        let Some(frame) = crate::slcan::parse_line(text.as_bytes()) else {
            return false;
        };
        match frame.id {
            0x234 => frame.len == 1,
            0x31B => frame.len == 8 && frame.data[1..8].iter().all(|byte| *byte == 0),
            0x1ED => frame.len == 0 || frame.len == 4,
            // Debug mode on or off, and nothing else on this identifier.
            0x28A => frame.len == 8 && (frame.data == [0, 0, 0, 1, 0, 0, 0, 0] || frame.data == [0; 8]),
            // The clock: 04, then hour, minute, second, day, month, year, BCD and in range.
            0x201 => {
                let bcd = |byte: u8, low: u32, high: u32| {
                    let (tens, ones) = (u32::from(byte >> 4), u32::from(byte & 0x0F));
                    tens <= 9 && ones <= 9 && (low..=high).contains(&(tens * 10 + ones))
                };
                let d = frame.data;
                frame.len == 7
                    && d[0] == 0x04
                    && bcd(d[1], 0, 23)
                    && bcd(d[2], 0, 59)
                    && bcd(d[3], 0, 59)
                    && bcd(d[4], 1, 31)
                    && bcd(d[5], 1, 12)
                    && bcd(d[6], 0, 99)
            }
            _ => false,
        }
    }

    #[test]
    fn every_request_the_app_can_send_is_one_the_firmware_lets_through() {
        for index in 0..=255u8 {
            assert!(firmware_allows(&battery_line(index)), "battery read {index}");
            assert!(firmware_allows(&ecu_line(index)), "ECU read {index}");
        }
        assert!(firmware_allows(&event_open_line()));
        for start in (0..5000u32).step_by(7) {
            assert!(firmware_allows(&event_page_line(start, PAGE_RECORDS)), "event page from {start}");
            assert!(firmware_allows(&event_page_line(start, 1)), "event page of one from {start}");
        }
        assert!(firmware_allows("r1838\r"), "the pack id request");
        assert!(firmware_allows(DEBUG_ON_LINE), "debug on");
        assert!(firmware_allows(DEBUG_OFF_LINE), "debug off");
        // Every moment the app can set the clock to makes a frame the firmware lets through.
        for year in 2000..=2099u16 {
            for month in 1..=12u8 {
                for day in [1u8, 15, 28, 29, 30, 31] {
                    for (hour, minute, second) in [(0u8, 0u8, 0u8), (13, 37, 42), (23, 59, 59)] {
                        let moment = crate::clock::Moment { year, month, day, hour, minute, second };
                        if let Some(line) = moment.set_line() {
                            assert!(firmware_allows(&line), "clock frame {line:?}");
                        }
                    }
                }
            }
        }
        // And the firmware refuses a clock frame that is not a real date or time.
        for line in [
            "t201704242828091126\r",
            "t201704226028091126\r",
            "t201704222860091126\r",
            "t201704222828001126\r",
            "t201704222828321126\r",
            "t201704222828090026\r",
            "t201704222828091326\r",
            "t20170422282809112A\r",
            "t201705222828091126\r",
            "t20160422282809112600\r",
            "t2016042228280911\r",
        ] {
            assert!(!firmware_allows(line), "{line:?}");
        }
        // The helper really does refuse what the firmware refuses.
        assert!(!firmware_allows("t31B80501000000000000\r"), "an ECU write");
        // Only the two debug frames pass on 0x28A. God mode (byte 0), the output
        // buttons (bytes 1 and 2) and any other bit are refused, and so is a
        // frame of another length.
        for line in [
            "t28A80100000100000000\r",
            "t28A80001000100000000\r",
            "t28A80000010100000000\r",
            "t28A80000000200000000\r",
            "t28A80000000100000001\r",
            "t28A80100000000000000\r",
            "t28A80000800000000000\r",
            "t28A40000000100000000\r",
            "t28A0\r",
            "t28A90000000100000000FF\r",
        ] {
            assert!(!firmware_allows(line), "{line:?}");
        }
        assert!(!firmware_allows("t2342AABB\r"), "a battery frame of two bytes");
        assert!(!firmware_allows("r2808\r"), "another remote frame");
    }

    #[test]
    fn request_lines_match_the_read_frames() {
        assert_eq!(battery_line(0x01), "t234101\r");
        assert_eq!(ecu_line(0x02), "t31B80200000000000000\r");
        assert_eq!(event_open_line(), "t1ED0\r");
    }

    #[test]
    fn shared_index_integers_use_three_decimals() {
        let mut job = Job::begin(Ask::Battery);
        assert_eq!(job.opening_line().as_deref(), Some("t234101\r"));
        let next = job.on_frame(0x235, &[0x01, 0x40, 0x9C, 0x00, 0, 0, 0, 0]);
        assert_eq!(job.values()[0], "40.000");
        assert_eq!(next, Reply::Send("t234102\r".into()));
        let _ = job.on_frame(0x235, &[0x02, 0x40, 0x9C, 0x00, 0x10, 0x27, 0x00, 0]);
        assert_eq!(job.values()[1], "40.000");
        assert_eq!(job.values()[2], "10.000");
    }

    #[test]
    fn an_ascii_slice_keeps_its_byte_span() {
        let early = ascii_piece(b"\x02WVWZZZ1", 0, 3, 3);
        let last = ascii_piece(b"\x041234567", 2, 3, 3);
        assert_eq!(early, "WVWZZZ1");
        assert_eq!(last, "123");
    }

    #[test]
    fn one_event_record_and_its_page_request() {
        let record = event_record(
            &[0x20, 0x01, 19, 10, 26, 14, 2, 11],
            &[0, 0, 0x22, 0x10, 0x01, 0x02, 0x03, 0x04],
            &[0, 0, 0x0A, 0x0B, 0x0C, 0x0D, 0, 0],
        );
        assert_eq!(record.index, 18);
        assert_eq!(record.time, "19/10/26 14:02:11");
        assert_eq!(record.code, "0x1022");
        assert_eq!(record.text, "Key on");
        assert_eq!(record.data, "040302010D0C0B0A");
        // The bytes of all three frames, for a saved log to be checked against the bus.
        assert_eq!(
            record.raw,
            "20 01 13 0A 1A 0E 02 0B | 00 00 22 10 01 02 03 04 | 00 00 0A 0B 0C 0D 00 00"
        );
        // The two numbers are the start and the count, low byte first. These two
        // lines are the ones sent on the scooter on 9 Nov 2026.
        assert_eq!(event_page_line(19, 10), "t1ED413000A00\r");
        assert_eq!(event_page_line(19, 5), "t1ED413000500\r");
        assert_eq!(event_page_line(0x0123, 10), "t1ED423010A00\r");
        assert_eq!(
            event_record(&[0, 0, 0, 0, 0, 0, 0, 0], &[0, 0, 0x08, 0x22, 0, 0, 0, 0], &[0; 8]).text,
            "SCD"
        );
    }

    /// The three frames of the record with this index. Returns the reply to the
    /// last one.
    fn feed_record(job: &mut Job, index: u32) -> Reply {
        let frames = [
            [(index as u8 & 0x0F) << 4, (index >> 4) as u8, 26, 10, 5, 11, 37, 0],
            [0, 0, 0x22, 0x10, 0, 0, 0, 0],
            [0u8; 8],
        ];
        let mut last = Reply::Wait;
        for (offset, frame) in frames.iter().enumerate() {
            last = job.on_frame(0x1EE, frame);
            if offset < 2 {
                assert_eq!(last, Reply::Wait, "a record is three frames");
            }
        }
        last
    }

    #[test]
    fn the_event_log_is_asked_for_ten_records_at_a_time_with_a_pause() {
        let mut job = Job::begin(Ask::Events);
        assert_eq!(job.opening_line().as_deref(), Some("t1ED0\r"));
        // The first request brings the newest ten, here 157 down to 148.
        for index in (149..=157).rev() {
            assert_eq!(feed_record(&mut job, index), Reply::Wait);
        }
        // After the tenth, the next ten: start at 147, ten records, after a pause.
        assert_eq!(
            feed_record(&mut job, 148),
            Reply::SendAfter(event_page_line(147, 10), PAGE_PAUSE)
        );
        assert_eq!(PAGE_PAUSE, Duration::from_millis(1000));
        for index in (138..=146).rev() {
            assert_eq!(feed_record(&mut job, index + 1), Reply::Wait, "records 147 down to 139");
        }
        assert_eq!(job.records().len(), 19);
        assert_eq!(job.reading_note(), "Reading the event log.");
        assert_eq!(job.on_frame(0x181, &[0]), Reply::Wait);
    }

    #[test]
    fn each_row_carries_its_record_number_and_raw_bytes_after_the_four_old_columns() {
        let mut job = Job::begin(Ask::Events);
        feed_record(&mut job, 157);
        feed_record(&mut job, 156);
        let row = &job.records()[0];
        assert_eq!(row[4], "157", "the record number");
        assert_eq!(job.records()[1][4], "156");
        assert_eq!(row[1], "0x1022", "the code is still the second column");
        assert!(row[5].starts_with("D0 09 1A 0A 05 0B 25 00 | 00 00 22 10"), "{}", row[5]);
        // The saved line keeps the first four columns where they were.
        let line = event_row_line(row);
        let parts: Vec<&str> = line.split('\t').collect();
        assert_eq!(parts.len(), 6);
        assert_eq!(parts[0], row[0]);
        assert_eq!(parts[3], "Key on");
        assert_eq!(parts[4], "157");
        assert_eq!(parts[5], row[5]);
    }

    #[test]
    fn a_whole_log_is_read_in_pages_and_ends_at_record_zero() {
        // A log of 25 records, 24 down to 0.
        let mut job = Job::begin(Ask::Events);
        let mut asked = vec![job.opening_line().unwrap()];
        let mut next = 24i64;
        let mut reply = Reply::Wait;
        while next >= 0 {
            reply = feed_record(&mut job, next as u32);
            if let Reply::SendAfter(line, wait) = &reply {
                assert_eq!(*wait, PAGE_PAUSE);
                asked.push(line.clone());
            }
            next -= 1;
        }
        assert_eq!(reply, Reply::Done, "record 0 ends the read");
        // The first request, then (14, 10) and then the last page, which is short.
        assert_eq!(asked, ["t1ED0\r", "t1ED40E000A00\r", "t1ED404000500\r"]);
        assert_eq!(job.records().len(), 25);
        assert_eq!(job.done_note(), "Event log read.");
        assert_eq!(job.on_frame(0x1EE, &[0; 8]), Reply::Done, "nothing more is taken in");
    }

    #[test]
    fn a_log_shorter_than_one_page_ends_at_record_zero() {
        let mut job = Job::begin(Ask::Events);
        for index in (1..=6).rev() {
            assert_eq!(feed_record(&mut job, index), Reply::Wait);
        }
        assert_eq!(feed_record(&mut job, 0), Reply::Done);
        assert_eq!(job.records().len(), 7);
    }

    #[test]
    fn a_record_out_of_step_stops_the_read() {
        let mut job = Job::begin(Ask::Events);
        assert_eq!(feed_record(&mut job, 50), Reply::Wait);
        // 49 is expected next. A lost frame or a repeat would put another number here.
        assert_eq!(feed_record(&mut job, 47), Reply::Done);
        assert_eq!(job.records().len(), 1, "the odd record is not kept");
        assert_eq!(job.done_note(), "The event log came back out of order. Showing what was read.");
        let mut repeat = Job::begin(Ask::Events);
        assert_eq!(feed_record(&mut repeat, 50), Reply::Wait);
        assert_eq!(feed_record(&mut repeat, 50), Reply::Done);
    }

    #[test]
    fn a_new_controller_fault_stops_the_read() {
        let mut job = Job::begin(Ask::Events);
        // The controller's frame comes every half second. The first one sets what is normal.
        assert_eq!(job.on_frame(0x411, &[0, 0, 0, 0, 9, 0, 16, 0]), Reply::Wait);
        assert_eq!(feed_record(&mut job, 30), Reply::Wait);
        assert_eq!(job.on_frame(0x411, &[0, 0, 0, 0, 9, 0, 16, 0]), Reply::Wait, "no change");
        // 0x4E80, as seen on the scooter.
        assert_eq!(job.on_frame(0x411, &[0, 0, 0x80, 0x4E, 9, 0, 16, 0]), Reply::Done);
        assert_eq!(
            job.done_note(),
            "The motor controller reported a fault, so the read was stopped; switch the scooter off and on again to clear it."
        );
        assert_eq!(job.records().len(), 1, "what was read is kept");
    }

    #[test]
    fn a_fault_that_was_already_there_does_not_stop_the_read() {
        let mut job = Job::begin(Ask::Events);
        assert_eq!(job.on_frame(0x411, &[0, 0, 0x80, 0x4E, 9, 0, 16, 0]), Reply::Wait);
        assert_eq!(job.on_frame(0x411, &[0, 0, 0x80, 0x4E, 9, 0, 17, 0]), Reply::Wait);
        // Cleared by a power cycle in the middle: still not a reason to stop.
        assert_eq!(job.on_frame(0x411, &[0, 0, 0, 0, 9, 0, 17, 0]), Reply::Wait);
        assert_eq!(feed_record(&mut job, 12), Reply::Wait);
        // A short or odd 0x411 frame is ignored.
        assert_eq!(job.on_frame(0x411, &[0, 0]), Reply::Wait);
        let mut other = Job::begin(Ask::Battery);
        assert_eq!(other.on_frame(0x411, &[0, 0, 0x80, 0x4E]), Reply::Wait, "only an event read watches it");
    }

    #[test]
    fn a_parameter_waits_for_its_own_reply() {
        let mut job = Job::begin(Ask::Battery);
        assert_eq!(job.on_frame(0x235, &[0x00, 1, 2, 3, 4, 5, 6, 7]), Reply::Wait);
        assert_eq!(job.on_frame(0x31C, &[0x01, 1, 2, 3, 4, 5, 6, 7]), Reply::Wait);
        assert_eq!(job.opening_line().as_deref(), Some("t234101\r"));
        assert_eq!(job.reading_note(), "Reading battery parameters.");
        let next = job.on_frame(0x235, &[0x01, 0x40, 0x9C, 0x00, 0, 0, 0, 0]);
        assert_eq!(next, Reply::Send("t234102\r".into()));
    }
}
