//! What the bus is sending, identifier by identifier, whether or not the app
//! decodes it. This feeds the Frames screen and the event markers.

use std::collections::BTreeMap;

use crate::decode;
use crate::snapshot::{Frame, Snapshot};

/// A byte that changed within this long counts as recently changed.
pub const RECENT_MS: u64 = 1500;

/// An undecoded identifier is tested again on its first few frames, because
/// some fields only react to particular values.
const RETEST_FRAMES: u64 = 8;

/// The bytes of the newest frame seen for one identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdBytes {
    pub id: u16,
    pub len: usize,
    pub data: [u8; 8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub t_ms: u64,
    pub text: String,
    /// What every identifier last said at the moment the marker was dropped,
    /// so two markers can be compared later (see `compare.rs`).
    pub state: Vec<IdBytes>,
}

#[derive(Clone, Debug)]
pub struct IdStats {
    pub count: u64,
    pub len: usize,
    pub data: [u8; 8],
    pub first_ms: u64,
    pub last_ms: u64,
    /// Smoothed time between frames.
    pub period_ms: Option<f32>,
    pub changed_ms: [Option<u64>; 8],
    /// How many times each byte has differed from the frame before it.
    pub changes: [u32; 8],
    /// Bit n is set once byte n has ever differed from the one before it.
    pub varied: u8,
    pub decoded: bool,
}

/// A byte that changes on most frames is a counter, a timer or a clock. It
/// would show up in every comparison, so a comparison can leave it out.
const NOISY_SHARE: f32 = 0.2;
const NOISY_AFTER_FRAMES: u64 = 20;

impl IdStats {
    /// Whether byte `index` changes on a large share of the frames, once
    /// enough have been seen to tell.
    pub fn is_noisy(&self, index: usize) -> bool {
        let Some(changes) = self.changes.get(index) else {
            return false;
        };
        self.count >= NOISY_AFTER_FRAMES && *changes as f32 / (self.count - 1) as f32 > NOISY_SHARE
    }

    pub fn rate(&self) -> Option<f32> {
        self.period_ms.filter(|period| *period > 0.0).map(|period| 1000.0 / period)
    }

    pub fn changed_recently(&self, index: usize, now_ms: u64) -> bool {
        self.changed_ms
            .get(index)
            .copied()
            .flatten()
            .is_some_and(|at| now_ms.saturating_sub(at) <= RECENT_MS)
    }

    pub fn has_varied(&self, index: usize) -> bool {
        index < 8 && self.varied & (1 << index) != 0
    }
}

#[derive(Clone, Debug, Default)]
pub struct Explorer {
    pub ids: BTreeMap<u16, IdStats>,
    pub marks: Vec<Mark>,
    pub total: u64,
    /// Time of the newest frame, on the same clock as the frame times.
    pub now_ms: u64,
}

impl Explorer {
    pub fn observe(&mut self, frame: &Frame, t_ms: u64) {
        self.total += 1;
        self.now_ms = t_ms;
        let Some(stats) = self.ids.get_mut(&frame.id) else {
            self.ids.insert(
                frame.id,
                IdStats {
                    count: 1,
                    len: frame.len,
                    data: frame.data,
                    first_ms: t_ms,
                    last_ms: t_ms,
                    period_ms: None,
                    changed_ms: [None; 8],
                    changes: [0; 8],
                    varied: 0,
                    decoded: is_decoded(frame),
                },
            );
            return;
        };
        let gap = t_ms.saturating_sub(stats.last_ms) as f32;
        if gap > 0.0 {
            stats.period_ms = Some(match stats.period_ms {
                Some(period) => period * 0.8 + gap * 0.2,
                None => gap,
            });
        }
        let mut changed = false;
        for index in 0..frame.len.min(stats.len) {
            if frame.data[index] != stats.data[index] {
                stats.changed_ms[index] = Some(t_ms);
                stats.changes[index] = stats.changes[index].saturating_add(1);
                stats.varied |= 1 << index;
                changed = true;
            }
        }
        stats.len = frame.len;
        stats.data = frame.data;
        stats.last_ms = t_ms;
        stats.count += 1;
        if !stats.decoded && changed && stats.count <= RETEST_FRAMES {
            stats.decoded = is_decoded(frame);
        }
    }

    /// The newest bytes of every identifier seen so far.
    pub fn state(&self) -> Vec<IdBytes> {
        self.ids
            .iter()
            .map(|(id, stats)| IdBytes {
                id: *id,
                len: stats.len,
                data: stats.data,
            })
            .collect()
    }

    pub fn add_mark(&mut self, text: &str, t_ms: u64) {
        let state = self.state();
        self.marks.push(Mark {
            t_ms,
            text: text.trim().to_string(),
            state,
        });
    }

    pub fn undecoded(&self) -> usize {
        self.ids.values().filter(|stats| !stats.decoded).count()
    }
}

/// True when the decoder does something with this frame: it changes the
/// snapshot, or keeps the raw bytes for the Flags screen.
pub fn is_decoded(frame: &Frame) -> bool {
    let mut probe = Snapshot::default();
    decode::apply(&mut probe, frame);
    format!("{probe:?}") != format!("{:?}", Snapshot::default())
}

/// Short names for identifiers that are decoded or well understood.
pub fn label(id: u16) -> &'static str {
    match id {
        0x181 => "Pack summary",
        0x183 => "Pack ID",
        0x184 => "BMS flags",
        0x185..=0x188 => "Cell voltages",
        0x189 => "Pack current, temperatures",
        0x190 => "Fine charge, health",
        0x197 => "Capacity left",
        0x1AA => "Charge and discharge limits",
        0x1AC => "Time since reset",
        0x1C0 | 0x1C1 => "Energy counters",
        0x280 => "Drive state",
        0x300 => "Speed and range",
        0x310 => "Dash text",
        0x371 => "Controller bus voltage",
        0x391 => "Motor speed",
        0x422 | 0x431 => "Controller current and torque",
        0x500 => "Outside temperature, odometer",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(id: u16, bytes: &[u8]) -> Frame {
        Frame::new(id, bytes)
    }

    #[test]
    fn counts_rate_and_changed_bytes() {
        let mut explorer = Explorer::default();
        explorer.observe(&frame(0x600, &[1, 2, 3]), 1_000);
        explorer.observe(&frame(0x600, &[1, 9, 3]), 1_100);
        explorer.observe(&frame(0x600, &[1, 9, 3]), 1_200);
        let stats = &explorer.ids[&0x600];
        assert_eq!(stats.count, 3);
        assert_eq!(stats.len, 3);
        assert!(!stats.has_varied(0));
        assert!(stats.has_varied(1));
        let rate = stats.rate().unwrap();
        assert!((rate - 10.0).abs() < 0.5, "rate {rate}");
        assert!(stats.changed_recently(1, 1_200));
        assert!(!stats.changed_recently(0, 1_200));
        assert!(!stats.changed_recently(1, 1_100 + RECENT_MS + 1));
        assert_eq!(explorer.total, 3);
    }

    #[test]
    fn a_new_length_does_not_invent_changes() {
        let mut explorer = Explorer::default();
        explorer.observe(&frame(0x600, &[1, 2]), 0);
        explorer.observe(&frame(0x600, &[1, 2, 7, 7]), 100);
        assert_eq!(explorer.ids[&0x600].varied, 0);
        assert_eq!(explorer.ids[&0x600].len, 4);
    }

    #[test]
    fn known_identifiers_are_decoded_and_unknown_ones_are_not() {
        let mut explorer = Explorer::default();
        explorer.observe(&frame(0x181, &[1, 80, 0, 0, 0x3A, 0x02, 0, 0]), 0);
        explorer.observe(&frame(0x184, &[0, 0, 0, 0, 0, 0, 0, 0]), 0);
        explorer.observe(&frame(0x7E0, &[1, 2, 3, 4]), 0);
        assert!(explorer.ids[&0x181].decoded);
        assert!(explorer.ids[&0x184].decoded);
        assert!(!explorer.ids[&0x7E0].decoded);
        assert_eq!(explorer.undecoded(), 1);
    }

    #[test]
    fn markers_keep_their_order_and_text() {
        let mut explorer = Explorer::default();
        explorer.add_mark("  pressed reverse ", 10);
        explorer.add_mark("side stand down", 20);
        assert_eq!(explorer.marks[0].text, "pressed reverse");
        assert_eq!(explorer.marks[1].t_ms, 20);
    }
}
