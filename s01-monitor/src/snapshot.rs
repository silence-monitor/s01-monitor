use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use web_time::{SystemTime, UNIX_EPOCH};

use crate::capture::Recorder;
use crate::export::ValueLogger;
use crate::clock::Moment;
use crate::frames::Explorer;
use crate::stored::EventRow;
use crate::signals::{SignalDef, Trace};

/// Samples older than this are dropped. It is the longest chart window.
pub const HISTORY_MS: u64 = 15 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub id: u16,
    pub data: [u8; 8],
    pub len: usize,
}

impl Frame {
    pub fn new(id: u16, bytes: &[u8]) -> Self {
        let mut data = [0u8; 8];
        let len = bytes.len().min(8);
        data[..len].copy_from_slice(&bytes[..len]);
        Self { id, data, len }
    }

    pub fn payload(&self) -> &[u8] {
        &self.data[..self.len]
    }
}

/// Bytes received for one identifier. `len` is the DLC, so a short frame
/// does not invent flags in the missing bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bytes {
    pub data: [u8; 8],
    pub len: usize,
}

impl From<&Frame> for Bytes {
    fn from(frame: &Frame) -> Self {
        Self {
            data: frame.data,
            len: frame.len,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    pub t_ms: u64,
    pub voltage: Option<f32>,
    pub current: Option<f32>,
    pub soc: Option<f32>,
    pub temp_a: Option<f32>,
    pub temp_b: Option<f32>,
    pub temp_c: Option<f32>,
    pub speed: Option<f32>,
    pub range_km: Option<f32>,
    pub rpm: Option<f32>,
    pub motor: Option<f32>,
    pub inverter: Option<f32>,
}

/// Progress and controls of a capture being replayed. The screen changes
/// `paused`, `speed` and `seek_to`; the replay reads them.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplayState {
    pub name: String,
    pub position_ms: u64,
    pub length_ms: u64,
    pub paused: bool,
    pub speed: f32,
    pub finished: bool,
    /// For a recording in another tool's format: that it is experimental, what
    /// it was read as and what was left out. Empty for the app's own formats.
    pub source_note: String,
    /// A moment to jump to, milliseconds into the capture. The screen sets
    /// it; the replay takes it at its next step.
    pub seek_to: Option<u64>,
}

/// What the app has done about the scooter's debug mode since the bus woke.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DebugState {
    /// Nothing to say: the switch is off, no scooter yet, the bus is asleep, or
    /// still looking.
    #[default]
    Idle,
    /// Sent debug on, and waiting for the ECU to report it.
    Sent,
    /// Sent debug on, and the ECU reports it is on.
    Confirmed,
    /// The ECU already reported debug mode when the scooter woke. Nothing sent.
    AlreadyOn,
    /// Sent debug on twice and the ECU never reported it.
    Unconfirmed,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    /// Every identifier seen, and the event markers.
    pub explorer: Explorer,
    /// The signals being read (see `signals.rs`) and what each is doing. The
    /// two lists are the same length and in the same order.
    pub signal_defs: Vec<SignalDef>,
    pub signal_traces: Vec<Trace>,
    /// A log file being written, if recording.
    pub recorder: Option<Arc<Mutex<Recorder>>>,
    /// A values log being written, if logging.
    pub logger: Option<Arc<Mutex<ValueLogger>>>,
    /// Set while a capture is being replayed.
    pub replay: Option<ReplayState>,
    pub pack_id: Option<String>,
    pub clock: Option<String>,
    /// The battery's own clock frame (0x200), when it is marked valid.
    pub battery_clock: Option<Moment>,
    /// What happened to the last request to set the battery clock.
    pub clock_note: String,
    /// The person's debug mode switch (Frames and Flags screens). Off at every
    /// start: the app only listens until it is switched on.
    pub debug_wanted: bool,
    /// Where the app is with the scooter's debug mode (see `slcan.rs`).
    pub debug: DebugState,
    pub voltage: Option<f32>,
    pub current: Option<f32>,
    /// Pack current from 0x181 bytes 6-7, unit 0.1 A. Same sign as `current`.
    pub current_summary: Option<f32>,
    /// Capacity left from 0x197 bytes 0-1, unit 0.01 Ah.
    pub capacity_ah: Option<f32>,
    pub soc: Option<f32>,
    pub temp_a: Option<f32>,
    pub temp_b: Option<f32>,
    pub temp_c: Option<f32>,
    pub cells: [Option<f32>; 14],
    pub counter: Option<u8>,
    pub temp_low: Option<i8>,
    pub temp_high: Option<i8>,
    pub soc_fine: Option<u16>,
    pub remain_mah: Option<u16>,
    pub vsoc: Option<u8>,
    pub soh: Option<u8>,
    pub max_discharge: Option<u16>,
    pub max_charge: Option<i16>,
    pub since_reset: Option<u32>,
    pub charged_ws: Option<u32>,
    pub discharged_ws: Option<u32>,
    pub regenerated_ws: Option<u32>,
    pub speed: Option<f32>,
    pub range_km: Option<f32>,
    pub power_bar: Option<u8>,
    pub drive_flags: Option<u8>,
    pub outside: Option<i8>,
    pub odometer: Option<u32>,
    pub throttle: Option<f32>,
    pub footbrake: Option<bool>,
    pub since_ignition: Option<u32>,
    pub controller: Option<u8>,
    pub mark_char: Option<String>,
    pub mark_stan: Option<String>,
    pub rpm: Option<f32>,
    pub motor: Option<f32>,
    pub inverter: Option<f32>,
    pub temp_third: Option<f32>,
    pub heatsink: Option<f32>,
    pub motor_ctrl: Option<f32>,
    pub cap_voltage: Option<f32>,
    pub battery_current: Option<f32>,
    pub torque_demand: Option<f32>,
    pub torque_actual: Option<f32>,
    pub bms: Option<Bytes>,
    pub balance: Option<Bytes>,
    pub extra: Option<Bytes>,
    pub ready: Option<Bytes>,
    pub io: Option<Bytes>,
    pub ecu_status: Option<Bytes>,
    pub sevcon: Option<Bytes>,
    pub history: VecDeque<Sample>,
    pub error: Option<String>,
    pub battery_values: Vec<String>,
    /// When the battery parameters were last read, Unix milliseconds.
    pub battery_read_ms: Option<u64>,
    /// When the ECU parameters were last read, Unix milliseconds.
    pub ecu_read_ms: Option<u64>,
    pub ecu_values: Vec<String>,
    pub param_note: String,
    pub read_busy: bool,
    pub event_rows: Vec<EventRow>,
    pub event_note: String,
    pub events_ready: bool,
}

impl Snapshot {
    pub fn sample_now(&self) -> Sample {
        Sample {
            t_ms: 0,
            voltage: self.voltage,
            current: self.current,
            soc: self.soc,
            temp_a: self.temp_a,
            temp_b: self.temp_b,
            temp_c: self.temp_c,
            speed: self.speed,
            range_km: self.range_km,
            rpm: self.rpm,
            motor: self.motor,
            inverter: self.inverter,
        }
    }

    /// Counts a received frame and writes it to the log if recording.
    pub fn tap(&mut self, frame: &Frame, t_ms: u64) {
        self.explorer.observe(frame, t_ms);
        for (def, trace) in self.signal_defs.iter().zip(self.signal_traces.iter_mut()) {
            if def.id == frame.id {
                trace.observe(def, frame.payload(), t_ms);
            }
        }
        if let Some(recorder) = &self.recorder {
            lock_shared(recorder).frame(frame, t_ms);
        }
    }

    /// Sets which signals are read. A signal that is unchanged keeps its
    /// history; a new or edited one starts empty. Nothing happens when the
    /// list is the same as before.
    pub fn set_signals(&mut self, defs: &[SignalDef]) {
        if self.signal_defs == defs {
            return;
        }
        let mut kept = std::mem::take(&mut self.signal_traces);
        let old = std::mem::take(&mut self.signal_defs);
        self.signal_traces = defs
            .iter()
            .map(|def| {
                old.iter()
                    .position(|before| before == def)
                    .and_then(|index| kept.get_mut(index))
                    .map(std::mem::take)
                    .unwrap_or_default()
            })
            .collect();
        self.signal_defs = defs.to_vec();
    }

    /// Path, frames written, markers written and any write error of the
    /// recording in progress.
    pub fn recorder_info(&self) -> Option<(String, u64, u32, Option<String>)> {
        let recorder = self.recorder.as_ref()?;
        let rec = lock_shared(recorder);
        Some((rec.path.display().to_string(), rec.frames, rec.marks, rec.error.clone()))
    }

    /// A problem with the browser's storage for the recording in progress.
    pub fn recorder_warning(&self) -> Option<String> {
        let recorder = self.recorder.as_ref()?;
        let rec = lock_shared(recorder);
        rec.storage_warning()
    }

    /// Path, rows written and any write error of the values log in progress.
    pub fn logger_info(&self) -> Option<(String, u64, Option<String>)> {
        let logger = self.logger.as_ref()?;
        let log = lock_shared(logger);
        Some((log.path.display().to_string(), log.rows, log.error.clone()))
    }

    /// Drops a note at the current moment. While replaying, the moment is the
    /// replay's own clock, so the note lands where the capture is up to.
    pub fn mark_now(&mut self, text: &str) {
        let t_ms = if self.replay.is_some() {
            self.explorer.now_ms
        } else {
            now_ms()
        };
        self.mark_at(text, t_ms);
    }

    pub fn mark_at(&mut self, text: &str, t_ms: u64) {
        self.explorer.add_mark(text, t_ms);
        if let Some(recorder) = &self.recorder {
            lock_shared(recorder).mark(text, t_ms);
        }
    }

    pub fn push_sample(&mut self, t_ms: u64) {
        let mut sample = self.sample_now();
        sample.t_ms = t_ms;
        self.history.push_back(sample);
        let keep_from = t_ms.saturating_sub(HISTORY_MS);
        while self
            .history
            .front()
            .is_some_and(|row| row.t_ms < keep_from)
        {
            self.history.pop_front();
        }
        if let Some(logger) = self.logger.clone() {
            lock_shared(&logger).sample(self, t_ms);
        }
    }

    pub fn has_signal(&self) -> bool {
        self.voltage.is_some()
            || self.current.is_some()
            || self.soc.is_some()
            || self.temp_a.is_some()
            || self.speed.is_some()
            || self.rpm.is_some()
            || self.cells.iter().any(Option::is_some)
    }

    pub fn bytes_for(&self, id: u16) -> Option<&Bytes> {
        match id {
            0x184 => self.bms.as_ref(),
            0x18E => self.balance.as_ref(),
            0x191 => self.extra.as_ref(),
            0x293 => self.ready.as_ref(),
            0x294 => self.io.as_ref(),
            0x28A => self.ecu_status.as_ref(),
            0x401 => self.sevcon.as_ref(),
            _ => None,
        }
    }
}

pub fn lock(snapshot: &Mutex<Snapshot>) -> MutexGuard<'_, Snapshot> {
    snapshot.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn lock_shared<T>(shared: &Mutex<T>) -> MutexGuard<'_, T> {
    shared.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Samples inside the selected window, ending at the newest stamp.
pub fn in_window(history: &VecDeque<Sample>, window_ms: u64) -> Vec<Sample> {
    let Some(newest) = history.back().map(|row| row.t_ms) else {
        return Vec::new();
    };
    let from = newest.saturating_sub(window_ms);
    history
        .iter()
        .filter(|row| row.t_ms >= from)
        .cloned()
        .collect()
}

pub fn format_age(newest: u64, t_ms: u64) -> String {
    if t_ms >= newest {
        return "now".into();
    }
    let seconds = newest.saturating_sub(t_ms) / 1000;
    if seconds < 60 {
        format!("{seconds} s")
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_keeps_fifteen_minutes() {
        let mut snapshot = Snapshot::default();
        snapshot.push_sample(0);
        snapshot.push_sample(HISTORY_MS);
        assert_eq!(snapshot.history.len(), 2);
        snapshot.push_sample(HISTORY_MS + 1);
        assert_eq!(snapshot.history.len(), 2);
        assert_eq!(snapshot.history.front().unwrap().t_ms, HISTORY_MS);
    }

    #[test]
    fn a_short_trace_is_not_stretched_to_the_window() {
        let mut history = VecDeque::new();
        history.push_back(Sample {
            t_ms: 1_000,
            ..Sample::default()
        });
        history.push_back(Sample {
            t_ms: 5_000,
            ..Sample::default()
        });
        let rows = in_window(&history, 60_000);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].t_ms - rows[0].t_ms, 4_000);
        assert_eq!(format_age(5_000, 5_000), "now");
        assert_eq!(format_age(5_000, 1_000), "4 s");
    }
    fn signal(name: &str, id: u16) -> SignalDef {
        SignalDef {
            key: format!("signal-{name}"),
            name: name.into(),
            id,
            start: 1,
            count: 1,
            signed: false,
            big_endian: false,
            scale: 1.0,
            offset: 0.0,
            unit: String::new(),
        }
    }

    #[test]
    fn a_signal_follows_only_its_own_identifier() {
        let mut snapshot = Snapshot::default();
        snapshot.set_signals(&[signal("a", 0x182), signal("b", 0x19B)]);
        snapshot.tap(&Frame::new(0x182, &[0, 9]), 100);
        snapshot.tap(&Frame::new(0x19B, &[0, 4]), 110);
        snapshot.tap(&Frame::new(0x500, &[0, 77]), 120);
        assert_eq!(snapshot.signal_traces[0].value, Some(9.0));
        assert_eq!(snapshot.signal_traces[1].value, Some(4.0));
    }

    #[test]
    fn changing_the_signals_keeps_the_history_of_those_left_alone() {
        let mut snapshot = Snapshot::default();
        let a = signal("a", 0x182);
        let b = signal("b", 0x19B);
        snapshot.set_signals(&[a.clone(), b.clone()]);
        snapshot.tap(&Frame::new(0x182, &[0, 5]), 100);
        snapshot.tap(&Frame::new(0x19B, &[0, 6]), 100);
        // The same list again changes nothing.
        snapshot.set_signals(&[a.clone(), b.clone()]);
        assert_eq!(snapshot.signal_traces[0].value, Some(5.0));
        // Dropping the first moves the second up with its history.
        snapshot.set_signals(std::slice::from_ref(&b));
        assert_eq!(snapshot.signal_traces.len(), 1);
        assert_eq!(snapshot.signal_traces[0].value, Some(6.0));
        // An edited signal starts again; an unedited one does not.
        let mut edited = b.clone();
        edited.scale = 2.0;
        snapshot.set_signals(&[edited, a]);
        assert_eq!(snapshot.signal_traces[0].value, None);
        assert_eq!(snapshot.signal_traces[1].value, None);
        snapshot.tap(&Frame::new(0x19B, &[0, 6]), 200);
        assert_eq!(snapshot.signal_traces[0].value, Some(12.0));
        snapshot.set_signals(&[]);
        assert!(snapshot.signal_traces.is_empty());
    }
}
