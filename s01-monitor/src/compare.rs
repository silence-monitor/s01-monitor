//! Compare two moments: which bytes of which identifiers differ between what
//! the bus said at one marker and what it said at another.
//!
//! This is how an unknown frame gets its meaning. Press the horn, drop a
//! marker; release it, drop another; then compare the two and see the byte and
//! the bit that moved.

use crate::frames::IdBytes;

/// One byte that is not the same in the two moments. `None` means that
/// moment had no such byte: the identifier had not been seen, or its frame
/// was shorter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diff {
    pub id: u16,
    pub byte: usize,
    pub a: Option<u8>,
    pub b: Option<u8>,
}

impl Diff {
    /// The bits that differ, when the byte is there in both moments.
    pub fn bits(&self) -> Option<u8> {
        Some(self.a? ^ self.b?)
    }
}

/// Every byte that differs, in identifier order and then byte order.
pub fn diff(a: &[IdBytes], b: &[IdBytes]) -> Vec<Diff> {
    let mut ids: Vec<u16> = a.iter().chain(b).map(|state| state.id).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut out = Vec::new();
    for id in ids {
        let in_a = a.iter().find(|state| state.id == id);
        let in_b = b.iter().find(|state| state.id == id);
        let length = in_a.map_or(0, |state| state.len).max(in_b.map_or(0, |state| state.len));
        for byte in 0..length {
            let before = in_a.and_then(|state| (byte < state.len).then(|| state.data[byte]));
            let after = in_b.and_then(|state| (byte < state.len).then(|| state.data[byte]));
            if before != after {
                out.push(Diff {
                    id,
                    byte,
                    a: before,
                    b: after,
                });
            }
        }
    }
    out
}

/// The number of different identifiers in a list of differences.
pub fn identifiers(diffs: &[Diff]) -> usize {
    let mut ids: Vec<u16> = diffs.iter().map(|diff| diff.id).collect();
    ids.dedup();
    ids.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::Explorer;
    use crate::snapshot::Frame;

    fn state(id: u16, bytes: &[u8]) -> IdBytes {
        let mut data = [0u8; 8];
        data[..bytes.len()].copy_from_slice(bytes);
        IdBytes {
            id,
            len: bytes.len(),
            data,
        }
    }

    #[test]
    fn identical_moments_have_no_differences() {
        let a = [state(0x280, &[0x21, 0x08]), state(0x300, &[0, 82])];
        assert!(diff(&a, &a).is_empty());
    }

    #[test]
    fn one_changed_byte_is_reported_with_both_values_and_the_bit_that_moved() {
        let a = [state(0x280, &[0x21, 0x08, 0x00]), state(0x300, &[0, 82])];
        let b = [state(0x280, &[0x21, 0x08, 0x04]), state(0x300, &[0, 82])];
        let found = diff(&a, &b);
        assert_eq!(
            found,
            [Diff {
                id: 0x280,
                byte: 2,
                a: Some(0x00),
                b: Some(0x04)
            }]
        );
        assert_eq!(found[0].bits(), Some(0b0000_0100));
    }

    #[test]
    fn an_identifier_seen_in_only_one_moment_differs_in_every_byte() {
        let a = [state(0x280, &[1])];
        let b = [state(0x280, &[1]), state(0x19B, &[9, 0x0A])];
        let found = diff(&a, &b);
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|d| d.id == 0x19B && d.a.is_none() && d.b.is_some()));
        assert_eq!(found[0].bits(), None);
        assert_eq!(identifiers(&found), 1);
    }

    #[test]
    fn a_longer_frame_differs_in_the_extra_bytes_only() {
        let a = [state(0x600, &[1, 2])];
        let b = [state(0x600, &[1, 2, 3, 4])];
        let found = diff(&a, &b);
        assert_eq!(found.iter().map(|d| d.byte).collect::<Vec<_>>(), [2, 3]);
        assert!(found.iter().all(|d| d.a.is_none()));
    }

    #[test]
    fn differences_come_in_identifier_then_byte_order() {
        let a = [state(0x300, &[0, 0]), state(0x100, &[0, 0])];
        let b = [state(0x300, &[1, 1]), state(0x100, &[1, 1])];
        let order: Vec<(u16, usize)> = diff(&a, &b).iter().map(|d| (d.id, d.byte)).collect();
        assert_eq!(order, [(0x100, 0), (0x100, 1), (0x300, 0), (0x300, 1)]);
    }

    #[test]
    fn a_marker_remembers_the_bus_as_it_was_when_it_was_dropped() {
        let mut explorer = Explorer::default();
        explorer.observe(&Frame::new(0x280, &[0x21, 0x08, 0x00]), 1_000);
        explorer.add_mark("horn off", 1_100);
        explorer.observe(&Frame::new(0x280, &[0x21, 0x08, 0x04]), 2_000);
        explorer.add_mark("horn on", 2_100);
        explorer.observe(&Frame::new(0x280, &[0x21, 0x08, 0x00]), 3_000);
        let off = &explorer.marks[0].state;
        let on = &explorer.marks[1].state;
        let found = diff(off, on);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].id, found[0].byte, found[0].bits()), (0x280, 2, Some(0x04)));
        // A later frame does not rewrite an earlier marker.
        assert_eq!(explorer.marks[0].state[0].data[2], 0x00);
    }

    #[test]
    fn a_counter_is_noisy_and_a_switch_is_not() {
        let mut explorer = Explorer::default();
        for step in 0..100u64 {
            // byte 0 counts up on every frame; byte 1 changes once.
            let switch = if step < 50 { 0 } else { 1 };
            explorer.observe(&Frame::new(0x297, &[step as u8, switch, 7]), step * 10);
        }
        let stats = &explorer.ids[&0x297];
        assert!(stats.is_noisy(0), "the counter changes on every frame");
        assert!(!stats.is_noisy(1), "the switch changed once");
        assert!(!stats.is_noisy(2), "this byte never changed");
        assert!(!stats.is_noisy(9), "no such byte");
        assert_eq!(stats.changes[0], 99);
        assert_eq!(stats.changes[1], 1);
    }

    #[test]
    fn a_byte_is_not_called_noisy_before_enough_frames_have_been_seen() {
        let mut explorer = Explorer::default();
        for step in 0..5u64 {
            explorer.observe(&Frame::new(0x297, &[step as u8]), step);
        }
        assert!(!explorer.ids[&0x297].is_noisy(0));
    }
}
