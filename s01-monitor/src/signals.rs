//! Signals: a way to read any bytes of any frame as a number and watch it,
//! for the frames the app does not decode yet.
//!
//! A signal says where the bytes are (identifier, first byte, how many), how to
//! read them (signed or not, which end comes first) and how to turn the number
//! into a reading (a scale and an offset). Try a guess, watch the line move
//! when you press the horn, and keep it if it is right.

use std::collections::VecDeque;

use crate::snapshot::HISTORY_MS;

/// How many signals can be kept at once.
pub const MAX_SIGNALS: usize = 8;
const MAX_NAME_CHARS: usize = 24;
const MAX_UNIT_CHARS: usize = 8;
/// A reading is kept when it changes, but not more often than this, and in any
/// case this often, so a flat line still has points.
const MIN_GAP_MS: u64 = 50;
const KEEP_EVERY_MS: u64 = 250;
/// The most points one signal remembers, however fast its frames come.
const MAX_POINTS: usize = 10_000;

#[derive(Clone, Debug, PartialEq)]
pub struct SignalDef {
    /// `signal-3` and so on, given when the signal is saved. Empty before.
    pub key: String,
    pub name: String,
    /// The identifier of the frame, 0 to 0x7FF.
    pub id: u16,
    /// The first byte, 0 to 7.
    pub start: u8,
    /// How many bytes, 1 to 4.
    pub count: u8,
    pub signed: bool,
    /// True when the first byte is the most significant.
    pub big_endian: bool,
    pub scale: f32,
    pub offset: f32,
    pub unit: String,
}

/// Text with control characters turned into spaces, trimmed and cut short.
pub fn clean_text(text: &str, most: usize) -> String {
    let spaced: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    spaced.trim().chars().take(most).collect::<String>().trim().to_string()
}

impl SignalDef {
    /// The name and unit as they will be kept.
    pub fn tidy(&mut self) {
        self.name = clean_text(&self.name, MAX_NAME_CHARS);
        self.unit = clean_text(&self.unit, MAX_UNIT_CHARS);
    }

    /// Whether this is a signal that can be read. The text says what is wrong.
    pub fn check(&self) -> Result<(), String> {
        if clean_text(&self.name, MAX_NAME_CHARS).is_empty() {
            return Err("Give the signal a name.".into());
        }
        if self.id > 0x7FF {
            return Err("An identifier is at most 7FF.".into());
        }
        if self.count == 0 || self.count > 4 {
            return Err("Read 1 to 4 bytes.".into());
        }
        if self.start > 7 || usize::from(self.start) + usize::from(self.count) > 8 {
            return Err("Those bytes run past the end of the frame. A frame has 8 bytes, numbered 0 to 7.".into());
        }
        if !self.scale.is_finite() || self.scale == 0.0 || self.scale.abs() > 1e9 {
            return Err("The scale must be a number other than zero.".into());
        }
        if !self.offset.is_finite() || self.offset.abs() > 1e12 {
            return Err("The offset must be a number.".into());
        }
        Ok(())
    }

    /// The bytes as a whole number, or `None` when the frame is too short.
    pub fn raw(&self, data: &[u8]) -> Option<i64> {
        let start = usize::from(self.start);
        let count = usize::from(self.count);
        if count == 0 || count > 4 || data.len() < start + count {
            return None;
        }
        let bytes = &data[start..start + count];
        let mut value: u64 = 0;
        if self.big_endian {
            for byte in bytes {
                value = value << 8 | u64::from(*byte);
            }
        } else {
            for byte in bytes.iter().rev() {
                value = value << 8 | u64::from(*byte);
            }
        }
        if self.signed {
            let unused = 64 - 8 * count as u32;
            Some(((value << unused) as i64) >> unused)
        } else {
            Some(value as i64)
        }
    }

    /// The reading: the number times the scale, plus the offset.
    pub fn scaled(&self, raw: i64) -> f32 {
        raw as f32 * self.scale + self.offset
    }

    /// Where the bytes are and how they are read, in words.
    pub fn describe(&self) -> String {
        let bytes = if self.count == 1 {
            format!("byte {}", self.start)
        } else {
            format!("bytes {}–{}", self.start, self.start + self.count - 1)
        };
        format!(
            "0x{:03X} {bytes}, {}{}",
            self.id,
            if self.signed { "signed" } else { "unsigned" },
            if self.count > 1 {
                if self.big_endian { ", first byte biggest" } else { ", last byte biggest" }
            } else {
                ""
            }
        )
    }
}

/// What a signal is doing right now, and how it has moved.
#[derive(Clone, Debug, Default)]
pub struct Trace {
    pub raw: Option<i64>,
    pub value: Option<f32>,
    /// The whole frame the reading came from.
    pub bytes: [u8; 8],
    pub len: usize,
    pub last_ms: u64,
    pub history: VecDeque<(u64, f32)>,
    kept_ms: u64,
}

impl Trace {
    /// A frame with this signal's identifier arrived.
    pub fn observe(&mut self, def: &SignalDef, data: &[u8], t_ms: u64) {
        let Some(raw) = def.raw(data) else {
            return;
        };
        let value = def.scaled(raw);
        let changed = self.raw != Some(raw);
        self.raw = Some(raw);
        self.value = Some(value);
        self.len = data.len().min(8);
        self.bytes = [0; 8];
        self.bytes[..self.len].copy_from_slice(&data[..self.len]);
        self.last_ms = t_ms;
        let gap = t_ms.saturating_sub(self.kept_ms);
        let due = self.history.is_empty() || gap >= KEEP_EVERY_MS || (changed && gap >= MIN_GAP_MS);
        if !due {
            return;
        }
        self.history.push_back((t_ms, value));
        self.kept_ms = t_ms;
        let keep_from = t_ms.saturating_sub(HISTORY_MS);
        while self.history.front().is_some_and(|(at, _)| *at < keep_from) || self.history.len() > MAX_POINTS {
            self.history.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(start: u8, count: u8, signed: bool, big_endian: bool) -> SignalDef {
        SignalDef {
            key: String::new(),
            name: "test".into(),
            id: 0x182,
            start,
            count,
            signed,
            big_endian,
            scale: 1.0,
            offset: 0.0,
            unit: String::new(),
        }
    }

    #[test]
    fn one_byte_unsigned_and_signed() {
        assert_eq!(def(1, 1, false, false).raw(&[0, 0xFF]), Some(255));
        assert_eq!(def(1, 1, true, false).raw(&[0, 0xFF]), Some(-1));
        assert_eq!(def(0, 1, true, false).raw(&[0x7F]), Some(127));
        assert_eq!(def(0, 1, true, false).raw(&[0x80]), Some(-128));
    }

    #[test]
    fn two_bytes_either_way_round() {
        let frame = [0x34, 0x12, 0x00];
        assert_eq!(def(0, 2, false, false).raw(&frame), Some(0x1234), "last byte biggest");
        assert_eq!(def(0, 2, false, true).raw(&frame), Some(0x3412), "first byte biggest");
        let negative = [0xFE, 0xFF];
        assert_eq!(def(0, 2, true, false).raw(&negative), Some(-2));
        assert_eq!(def(0, 2, false, false).raw(&negative), Some(0xFFFE));
    }

    #[test]
    fn three_and_four_bytes_including_a_negative_number() {
        let frame = [0x01, 0x02, 0x03, 0x04, 0, 0, 0, 0];
        assert_eq!(def(0, 3, false, false).raw(&frame), Some(0x030201));
        assert_eq!(def(0, 4, false, false).raw(&frame), Some(0x04030201));
        assert_eq!(def(0, 4, false, true).raw(&frame), Some(0x01020304));
        assert_eq!(def(0, 3, true, false).raw(&[0xFF, 0xFF, 0xFF]), Some(-1));
        assert_eq!(def(0, 4, true, false).raw(&[0x00, 0x00, 0x00, 0x80]), Some(i64::from(i32::MIN)));
        assert_eq!(def(0, 4, false, false).raw(&[0xFF; 4]), Some(0xFFFF_FFFF));
    }

    #[test]
    fn a_frame_too_short_gives_no_reading() {
        assert_eq!(def(2, 2, false, false).raw(&[1, 2, 3]), None);
        assert_eq!(def(0, 1, false, false).raw(&[]), None);
        assert_eq!(def(7, 1, false, false).raw(&[0; 7]), None);
        assert_eq!(def(7, 1, false, false).raw(&[0; 8]), Some(0));
    }

    #[test]
    fn the_reading_is_the_number_times_the_scale_plus_the_offset() {
        let mut signal = def(0, 2, true, false);
        signal.scale = 0.1;
        signal.offset = -40.0;
        // 0x0258 = 600; 600 * 0.1 - 40 = 20
        let value = signal.scaled(signal.raw(&[0x58, 0x02]).unwrap());
        assert!((value - 20.0).abs() < 1e-4, "{value}");
        signal.scale = -2.0;
        signal.offset = 0.0;
        assert_eq!(signal.scaled(signal.raw(&[5, 0]).unwrap()), -10.0);
    }

    #[test]
    fn a_signal_that_cannot_be_read_says_why() {
        let good = def(2, 2, false, false);
        assert!(good.check().is_ok());
        let mut bad = good.clone();
        bad.name = "  ".into();
        assert!(bad.check().unwrap_err().contains("name"));
        let mut bad = good.clone();
        bad.id = 0x800;
        assert!(bad.check().unwrap_err().contains("7FF"));
        let mut bad = good.clone();
        bad.count = 0;
        assert!(bad.check().is_err());
        bad.count = 5;
        assert!(bad.check().is_err());
        let mut bad = good.clone();
        bad.start = 7;
        bad.count = 2;
        assert!(bad.check().unwrap_err().contains("past the end"));
        let mut bad = good.clone();
        bad.scale = 0.0;
        assert!(bad.check().is_err());
        bad.scale = f32::NAN;
        assert!(bad.check().is_err());
        bad.scale = 1.0;
        bad.offset = f32::INFINITY;
        assert!(bad.check().is_err());
    }

    #[test]
    fn names_and_units_are_tidied() {
        let mut signal = def(0, 1, false, false);
        signal.name = "  a\tb\nc  ".into();
        signal.unit = "very long unit".into();
        signal.tidy();
        assert_eq!(signal.name, "a b c");
        assert_eq!(signal.unit.chars().count(), MAX_UNIT_CHARS);
    }

    #[test]
    fn it_describes_where_the_bytes_are() {
        assert_eq!(def(1, 1, false, false).describe(), "0x182 byte 1, unsigned");
        assert_eq!(def(2, 2, true, false).describe(), "0x182 bytes 2–3, signed, last byte biggest");
        assert_eq!(def(0, 4, false, true).describe(), "0x182 bytes 0–3, unsigned, first byte biggest");
    }

    #[test]
    fn a_trace_keeps_a_change_but_not_every_repeat() {
        let signal = def(0, 1, false, false);
        let mut trace = Trace::default();
        trace.observe(&signal, &[5], 1_000);
        assert_eq!(trace.value, Some(5.0));
        // The same value ten times in 100 ms adds nothing.
        for step in 1..=10 {
            trace.observe(&signal, &[5], 1_000 + step * 10);
        }
        assert_eq!(trace.history.len(), 1);
        // A change is kept once the minimum gap has passed.
        trace.observe(&signal, &[6], 1_200);
        assert_eq!(trace.history.len(), 2);
        // A change faster than the minimum gap is shown as the latest value but not stored.
        trace.observe(&signal, &[7], 1_205);
        assert_eq!(trace.value, Some(7.0));
        assert_eq!(trace.history.len(), 2);
        // A flat line still gets a point every so often.
        trace.observe(&signal, &[7], 1_205 + KEEP_EVERY_MS);
        assert_eq!(trace.history.len(), 3);
        assert_eq!(trace.bytes[0], 7);
        assert_eq!(trace.len, 1);
    }

    #[test]
    fn a_trace_forgets_what_is_older_than_the_chart_window_and_stays_bounded() {
        let signal = def(0, 1, false, false);
        let mut trace = Trace::default();
        trace.observe(&signal, &[1], 0);
        trace.observe(&signal, &[2], HISTORY_MS + 1_000);
        assert_eq!(trace.history.len(), 1);
        assert_eq!(trace.history.front().unwrap().1, 2.0);
        // Many fast changes never grow it past the cap.
        let mut busy = Trace::default();
        for step in 0..60_000u64 {
            busy.observe(&signal, &[(step % 251) as u8], step * MIN_GAP_MS);
        }
        assert!(busy.history.len() <= MAX_POINTS);
    }

    #[test]
    fn a_short_frame_leaves_the_trace_as_it_was() {
        let signal = def(2, 2, false, false);
        let mut trace = Trace::default();
        trace.observe(&signal, &[0, 0, 1, 0], 10);
        trace.observe(&signal, &[0, 0], 20);
        assert_eq!(trace.raw, Some(1));
        assert_eq!(trace.last_ms, 10);
    }
}
